use anyhow::Result;
use opentelemetry::KeyValue;
use serde_sarif::sarif::Result as SarifResult;

use crate::engine::AnalysisContext;
use crate::rules::{Rule, RuleMetadata, method_location_with_line, result_message};

/// Rule that detects direct Java deserialization entry-point calls.
#[derive(Default)]
pub(crate) struct DeserializationReadObjectCallRule;

crate::register_rule!(DeserializationReadObjectCallRule);

impl Rule for DeserializationReadObjectCallRule {
    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: "DESERIALIZATION_READ_OBJECT_CALL",
            name: "ObjectInputStream deserialization call",
            description: "readObject/readUnshared are high-risk Java deserialization entry points",
        }
    }

    fn run(&self, context: &AnalysisContext) -> Result<Vec<SarifResult>> {
        let mut results = Vec::new();
        for class in context.analysis_target_classes() {
            let mut attributes = vec![KeyValue::new("inspequte.class", class.name.clone())];
            if let Some(uri) = context.class_artifact_uri(class) {
                attributes.push(KeyValue::new("inspequte.artifact_uri", uri));
            }
            let class_results =
                context.with_span("scan.class", &attributes, || -> Result<Vec<SarifResult>> {
                    let mut class_results = Vec::new();
                    let artifact_uri = context.class_artifact_uri(class);
                    for method in &class.methods {
                        for call in &method.calls {
                            if is_deserialization_entry(&call.owner, &call.name, &call.descriptor)
                            {
                                let message = result_message(format!(
                                    "Avoid ObjectInputStream deserialization call in {}.{}{}; use safer formats or strict deserialization controls.",
                                    class.name, method.name, method.descriptor
                                ));
                                let line = method.line_for_offset(call.offset);
                                let location = method_location_with_line(
                                    &class.name,
                                    &method.name,
                                    &method.descriptor,
                                    artifact_uri.as_deref(),
                                    line,
                                );
                                class_results.push(
                                    SarifResult::builder()
                                        .message(message)
                                        .locations(vec![location])
                                        .build(),
                                );
                            }
                        }
                    }
                    Ok(class_results)
                })?;
            results.extend(class_results);
        }

        Ok(results)
    }
}

fn is_deserialization_entry(owner: &str, name: &str, descriptor: &str) -> bool {
    owner == "java/io/ObjectInputStream"
        && matches!(
            (name, descriptor),
            ("readObject", "()Ljava/lang/Object;") | ("readUnshared", "()Ljava/lang/Object;")
        )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::test_harness::{JvmTestHarness, Language, SourceFile};

    fn deserialize_messages(output: &crate::engine::EngineOutput) -> Vec<String> {
        output
            .results
            .iter()
            .filter(|result| result.rule_id.as_deref() == Some("DESERIALIZATION_READ_OBJECT_CALL"))
            .filter_map(|result| result.message.text.clone())
            .collect()
    }

    fn compile_and_analyze(
        harness: &JvmTestHarness,
        sources: &[SourceFile],
        classpath: &[PathBuf],
    ) -> crate::engine::EngineOutput {
        harness
            .compile_and_analyze(Language::Java, sources, classpath)
            .expect("run harness analysis")
    }

    #[test]
    fn deserialization_read_object_call_reports_read_object() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/ClassA.java".to_string(),
            contents: r#"
package com.example;
import java.io.ObjectInputStream;
public class ClassA {
    public Object methodX(ObjectInputStream varOne) throws Exception {
        return varOne.readObject();
    }
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, &sources, &[]);
        let messages = deserialize_messages(&output);
        assert!(
            messages
                .iter()
                .any(|msg| msg.contains("Avoid ObjectInputStream deserialization call")),
            "expected DESERIALIZATION_READ_OBJECT_CALL finding for readObject, got {messages:?}"
        );
    }

    #[test]
    fn deserialization_read_object_call_reports_read_unshared() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/ClassB.java".to_string(),
            contents: r#"
package com.example;
import java.io.ObjectInputStream;
public class ClassB {
    public Object methodY(ObjectInputStream varOne) throws Exception {
        return varOne.readUnshared();
    }
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, &sources, &[]);
        let messages = deserialize_messages(&output);
        assert!(
            messages
                .iter()
                .any(|msg| msg.contains("Avoid ObjectInputStream deserialization call")),
            "expected DESERIALIZATION_READ_OBJECT_CALL finding for readUnshared, got {messages:?}"
        );
    }

    #[test]
    fn deserialization_read_object_call_ignores_non_deserialization_reads() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/ClassC.java".to_string(),
            contents: r#"
package com.example;
import java.io.DataInputStream;
public class ClassC {
    public int methodZ(DataInputStream varOne) throws Exception {
        return varOne.readInt();
    }
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, &sources, &[]);
        let messages = deserialize_messages(&output);
        assert!(
            messages.is_empty(),
            "did not expect DESERIALIZATION_READ_OBJECT_CALL finding for non-deserialization API: {messages:?}"
        );
    }

    #[test]
    fn deserialization_read_object_call_ignores_classpath_calls() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");

        let dependency_sources = vec![SourceFile {
            path: "com/example/ClassB.java".to_string(),
            contents: r#"
package com.example;
import java.io.ObjectInputStream;
public class ClassB {
    public Object methodY(ObjectInputStream varOne) throws Exception {
        return varOne.readObject();
    }
}
"#
            .to_string(),
        }];
        let dependency_output = harness
            .compile(Language::Java, &dependency_sources, &[])
            .expect("compile dependency classes");

        let app_sources = vec![SourceFile {
            path: "com/example/ClassA.java".to_string(),
            contents: r#"
package com.example;
public class ClassA {
    public void methodX() {}
}
"#
            .to_string(),
        }];
        let app_output = harness
            .compile(
                Language::Java,
                &app_sources,
                &[dependency_output.classes_dir().to_path_buf()],
            )
            .expect("compile app classes");

        let analysis = harness
            .analyze(
                app_output.classes_dir(),
                &[dependency_output.classes_dir().to_path_buf()],
            )
            .expect("run harness analysis");
        let messages = deserialize_messages(&analysis);
        assert!(
            messages.is_empty(),
            "classpath classes must be out of scope for DESERIALIZATION_READ_OBJECT_CALL: {messages:?}"
        );
    }
}
