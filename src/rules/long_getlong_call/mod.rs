use anyhow::Result;
use opentelemetry::KeyValue;
use serde_sarif::sarif::Result as SarifResult;

use crate::engine::AnalysisContext;
use crate::rules::{Rule, RuleMetadata, method_location_with_line, result_message};

/// Rule that detects direct Long.getLong calls.
#[derive(Default)]
pub(crate) struct LongGetlongCallRule;

crate::register_rule!(LongGetlongCallRule);

impl Rule for LongGetlongCallRule {
    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: "LONG_GETLONG_CALL",
            name: "Long.getLong call",
            description: "Long.getLong reads system properties, not numeric input strings",
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
                            if is_long_getlong_call(&call.owner, &call.name, &call.descriptor) {
                                let message = result_message(format!(
                                    "Avoid Long.getLong() in {}.{}{}; use Long.parseLong()/valueOf() for numeric parsing or keep it only for system property reads.",
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

fn is_long_getlong_call(owner: &str, name: &str, descriptor: &str) -> bool {
    owner == "java/lang/Long"
        && name == "getLong"
        && matches!(
            descriptor,
            "(Ljava/lang/String;)Ljava/lang/Long;"
                | "(Ljava/lang/String;J)Ljava/lang/Long;"
                | "(Ljava/lang/String;Ljava/lang/Long;)Ljava/lang/Long;"
        )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::test_harness::{JvmTestHarness, Language, SourceFile};

    fn getlong_messages(output: &crate::engine::EngineOutput) -> Vec<String> {
        output
            .results
            .iter()
            .filter(|result| result.rule_id.as_deref() == Some("LONG_GETLONG_CALL"))
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
    fn long_getlong_call_reports_single_arg_usage() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/ClassA.java".to_string(),
            contents: r#"
package com.example;
public class ClassA {
    public Long methodX(String varOne) {
        return Long.getLong(varOne);
    }
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, &sources, &[]);
        let messages = getlong_messages(&output);
        assert!(
            messages
                .iter()
                .any(|msg| msg.contains("Avoid Long.getLong()")),
            "expected LONG_GETLONG_CALL finding, got {messages:?}"
        );
    }

    #[test]
    fn long_getlong_call_reports_string_long_overload() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/ClassB.java".to_string(),
            contents: r#"
package com.example;
public class ClassB {
    public Long methodY(String varOne) {
        return Long.getLong(varOne, 10L);
    }
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, &sources, &[]);
        let messages = getlong_messages(&output);
        assert!(
            messages
                .iter()
                .any(|msg| msg.contains("Avoid Long.getLong()")),
            "expected LONG_GETLONG_CALL finding for (String,long), got {messages:?}"
        );
    }

    #[test]
    fn long_getlong_call_ignores_parse_long_usage() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/ClassC.java".to_string(),
            contents: r#"
package com.example;
public class ClassC {
    public long methodZ(String varOne) {
        return Long.parseLong(varOne);
    }
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, &sources, &[]);
        let messages = getlong_messages(&output);
        assert!(
            messages.is_empty(),
            "did not expect LONG_GETLONG_CALL finding for parseLong(): {messages:?}"
        );
    }

    #[test]
    fn long_getlong_call_ignores_classpath_calls() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");

        let dependency_sources = vec![SourceFile {
            path: "com/example/ClassB.java".to_string(),
            contents: r#"
package com.example;
public class ClassB {
    public Long methodY(String varOne) {
        return Long.getLong(varOne);
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
        let messages = getlong_messages(&analysis);
        assert!(
            messages.is_empty(),
            "classpath classes must be out of scope for LONG_GETLONG_CALL: {messages:?}"
        );
    }
}
