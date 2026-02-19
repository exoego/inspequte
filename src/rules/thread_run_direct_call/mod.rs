use anyhow::Result;
use opentelemetry::KeyValue;
use serde_sarif::sarif::Result as SarifResult;

use crate::engine::AnalysisContext;
use crate::ir::{CallKind, CallSite, Method};
use crate::rules::{Rule, RuleMetadata, method_location_with_line, result_message};

/// Rule that detects direct calls to `Thread.run()`.
#[derive(Default)]
pub(crate) struct ThreadRunDirectCallRule;

crate::register_rule!(ThreadRunDirectCallRule);

impl Rule for ThreadRunDirectCallRule {
    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: "THREAD_RUN_DIRECT_CALL",
            name: "Thread.run direct call",
            description: "Direct Thread.run() calls execute synchronously on the current thread",
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
                            if !is_thread_run_call(call) {
                                continue;
                            }
                            if is_allowed_super_run_call(method, call) {
                                continue;
                            }

                            let message = result_message(format!(
                                "Avoid direct Thread.run() in {}.{}{}; call start() for asynchronous execution.",
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
                    Ok(class_results)
                })?;
            results.extend(class_results);
        }
        Ok(results)
    }
}

fn is_thread_run_call(call: &CallSite) -> bool {
    call.owner == "java/lang/Thread" && call.name == "run" && call.descriptor == "()V"
}

fn is_allowed_super_run_call(method: &Method, call: &CallSite) -> bool {
    method.name == "run"
        && method.descriptor == "()V"
        && call.kind == CallKind::Special
        && is_thread_run_call(call)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::test_harness::{JvmTestHarness, Language, SourceFile};

    fn thread_run_messages(output: &crate::engine::EngineOutput) -> Vec<String> {
        output
            .results
            .iter()
            .filter(|result| result.rule_id.as_deref() == Some("THREAD_RUN_DIRECT_CALL"))
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
    fn thread_run_direct_call_reports_run_call() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/ClassA.java".to_string(),
            contents: r#"
package com.example;
public class ClassA {
    public void methodX() {
        Thread varOne = new Thread(() -> {});
        varOne.run();
    }
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, &sources, &[]);
        let messages = thread_run_messages(&output);
        assert!(
            messages
                .iter()
                .any(|msg| msg.contains("Avoid direct Thread.run()")),
            "expected THREAD_RUN_DIRECT_CALL finding, got {messages:?}"
        );
    }

    #[test]
    fn thread_run_direct_call_ignores_start_call() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/ClassB.java".to_string(),
            contents: r#"
package com.example;
public class ClassB {
    public void methodY() {
        Thread varOne = new Thread(() -> {});
        varOne.start();
    }
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, &sources, &[]);
        let messages = thread_run_messages(&output);
        assert!(
            messages.is_empty(),
            "did not expect THREAD_RUN_DIRECT_CALL finding for Thread.start(): {messages:?}"
        );
    }

    #[test]
    fn thread_run_direct_call_ignores_super_run_in_override() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/ClassC.java".to_string(),
            contents: r#"
package com.example;
public class ClassC extends Thread {
    @Override
    public void run() {
        super.run();
    }
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, &sources, &[]);
        let messages = thread_run_messages(&output);
        assert!(
            messages.is_empty(),
            "did not expect THREAD_RUN_DIRECT_CALL finding for super.run(): {messages:?}"
        );
    }

    #[test]
    fn thread_run_direct_call_ignores_classpath_calls() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");

        let dependency_sources = vec![SourceFile {
            path: "com/example/ClassB.java".to_string(),
            contents: r#"
package com.example;
public class ClassB {
    public void methodY() {
        Thread varOne = new Thread(() -> {});
        varOne.run();
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
        let messages = thread_run_messages(&analysis);
        assert!(
            messages.is_empty(),
            "classpath classes must be out of scope for THREAD_RUN_DIRECT_CALL: {messages:?}"
        );
    }
}
