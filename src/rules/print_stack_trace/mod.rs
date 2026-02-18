use anyhow::Result;
use opentelemetry::KeyValue;
use serde_sarif::sarif::Result as SarifResult;

use crate::engine::AnalysisContext;
use crate::rules::{Rule, RuleMetadata, method_location_with_line, result_message};

/// Rule that detects direct calls to `Throwable.printStackTrace`.
#[derive(Default)]
pub(crate) struct PrintStackTraceRule;

crate::register_rule!(PrintStackTraceRule);

impl Rule for PrintStackTraceRule {
    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: "PRINT_STACK_TRACE",
            name: "Direct printStackTrace call",
            description: "Throwable.printStackTrace should be replaced with structured logging",
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
                            if is_print_stack_trace_call(&call.owner, &call.name, &call.descriptor)
                            {
                                let message = result_message(format!(
                                    "Avoid printStackTrace() in {}.{}{}; log exceptions with context instead.",
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

fn is_print_stack_trace_call(owner: &str, name: &str, descriptor: &str) -> bool {
    if name != "printStackTrace" {
        return false;
    }
    if !matches!(
        descriptor,
        "()V" | "(Ljava/io/PrintStream;)V" | "(Ljava/io/PrintWriter;)V"
    ) {
        return false;
    }
    owner == "java/lang/Throwable"
        || owner.ends_with("/Throwable")
        || owner.ends_with("Exception")
        || owner.ends_with("Error")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::test_harness::{JvmTestHarness, Language, SourceFile};

    fn print_stack_trace_messages(output: &crate::engine::EngineOutput) -> Vec<String> {
        output
            .results
            .iter()
            .filter(|result| result.rule_id.as_deref() == Some("PRINT_STACK_TRACE"))
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
    fn print_stack_trace_reports_no_arg_call() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/ClassA.java".to_string(),
            contents: r#"
package com.example;
public class ClassA {
    public void methodX(Exception varOne) {
        varOne.printStackTrace();
    }
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, &sources, &[]);
        let messages = print_stack_trace_messages(&output);
        assert!(
            messages
                .iter()
                .any(|msg| msg.contains("Avoid printStackTrace()")),
            "expected PRINT_STACK_TRACE finding for no-arg overload, got {messages:?}"
        );
    }

    #[test]
    fn print_stack_trace_reports_print_writer_overload() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/ClassB.java".to_string(),
            contents: r#"
package com.example;
import java.io.PrintWriter;
public class ClassB {
    public void methodY(Exception varOne) {
        varOne.printStackTrace(new PrintWriter(System.err));
    }
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, &sources, &[]);
        let messages = print_stack_trace_messages(&output);
        assert!(
            messages
                .iter()
                .any(|msg| msg.contains("Avoid printStackTrace()")),
            "expected PRINT_STACK_TRACE finding for PrintWriter overload, got {messages:?}"
        );
    }

    #[test]
    fn print_stack_trace_ignores_unrelated_stack_api() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/ClassC.java".to_string(),
            contents: r#"
package com.example;
public class ClassC {
    public StackTraceElement[] methodZ() {
        return Thread.currentThread().getStackTrace();
    }
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, &sources, &[]);
        let messages = print_stack_trace_messages(&output);
        assert!(
            messages.is_empty(),
            "did not expect PRINT_STACK_TRACE finding for Thread.getStackTrace: {messages:?}"
        );
    }

    #[test]
    fn print_stack_trace_ignores_classpath_calls() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let dependency_sources = vec![SourceFile {
            path: "com/example/ClassB.java".to_string(),
            contents: r#"
package com.example;
public class ClassB {
    public void methodY(Exception varOne) {
        varOne.printStackTrace();
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
        let messages = print_stack_trace_messages(&analysis);
        assert!(
            messages.is_empty(),
            "classpath classes must be out of scope for PRINT_STACK_TRACE: {messages:?}"
        );
    }
}
