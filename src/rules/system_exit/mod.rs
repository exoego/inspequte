use anyhow::Result;
use opentelemetry::KeyValue;
use serde_sarif::sarif::Result as SarifResult;

use crate::engine::AnalysisContext;
use crate::rules::{Rule, RuleMetadata, method_location_with_line, result_message};

/// Rule that detects direct JVM termination via `System.exit(int)`.
#[derive(Default)]
pub(crate) struct SystemExitRule;

crate::register_rule!(SystemExitRule);

impl Rule for SystemExitRule {
    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: "SYSTEM_EXIT",
            name: "System.exit call",
            description: "Direct calls to System.exit(int) terminate the JVM abruptly",
        }
    }

    fn run(&self, context: &AnalysisContext) -> Result<Vec<SarifResult>> {
        let mut results = Vec::new();
        for class in context.analysis_target_classes() {
            let has_main_with_args = class
                .methods
                .iter()
                .any(|method| is_java_entrypoint_main_with_args(method));
            let mut attributes = vec![KeyValue::new("inspequte.class", class.name.clone())];
            if let Some(uri) = context.class_artifact_uri(class) {
                attributes.push(KeyValue::new("inspequte.artifact_uri", uri));
            }
            let class_results =
                context.with_span("scan.class", &attributes, || -> Result<Vec<SarifResult>> {
                    let mut class_results = Vec::new();
                    let artifact_uri = context.class_artifact_uri(class);
                    for method in &class.methods {
                        if is_allowed_main_method(method, has_main_with_args) {
                            continue;
                        }
                        for call in &method.calls {
                            if is_process_termination_call(&call.owner, &call.name, &call.descriptor)
                            {
                                let message = result_message(format!(
                                    "Avoid System.exit() in {}.{}{}; return an error or throw an exception instead.",
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

fn is_process_termination_call(owner: &str, name: &str, descriptor: &str) -> bool {
    if owner == "java/lang/System" && name == "exit" && descriptor == "(I)V" {
        return true;
    }
    owner == "kotlin/system/ProcessKt"
        && name == "exitProcess"
        && matches!(descriptor, "(I)Ljava/lang/Void;" | "(I)V")
}

fn is_java_entrypoint_main_with_args(method: &crate::ir::Method) -> bool {
    method.name == "main"
        && method.descriptor == "([Ljava/lang/String;)V"
        && method.access.is_public
        && method.access.is_static
}

fn is_kotlin_top_level_main_without_args(method: &crate::ir::Method) -> bool {
    method.name == "main"
        && method.descriptor == "()V"
        && method.access.is_public
        && method.access.is_static
}

fn is_allowed_main_method(method: &crate::ir::Method, has_main_with_args: bool) -> bool {
    is_java_entrypoint_main_with_args(method)
        || (has_main_with_args && is_kotlin_top_level_main_without_args(method))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::test_harness::{JvmTestHarness, Language, SourceFile};

    fn system_exit_messages(output: &crate::engine::EngineOutput) -> Vec<String> {
        output
            .results
            .iter()
            .filter(|result| result.rule_id.as_deref() == Some("SYSTEM_EXIT"))
            .filter_map(|result| result.message.text.clone())
            .collect()
    }

    fn compile_and_analyze(
        harness: &JvmTestHarness,
        language: Language,
        sources: &[SourceFile],
        classpath: &[PathBuf],
    ) -> crate::engine::EngineOutput {
        harness
            .compile_and_analyze(language, sources, classpath)
            .expect("run harness analysis")
    }

    #[test]
    fn system_exit_reports_direct_call() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/ClassA.java".to_string(),
            contents: r#"
package com.example;
public class ClassA {
    public void methodX(boolean varOne) {
        if (varOne) {
            System.exit(1);
        }
    }
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, Language::Java, &sources, &[]);
        let messages = system_exit_messages(&output);

        assert!(
            messages
                .iter()
                .any(|msg| msg.contains("Avoid System.exit()")),
            "expected SYSTEM_EXIT finding, got {messages:?}"
        );
    }

    #[test]
    fn system_exit_ignores_non_exit_system_calls() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/ClassB.java".to_string(),
            contents: r#"
package com.example;
public class ClassB {
    public String methodY() {
        return System.lineSeparator();
    }
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, Language::Java, &sources, &[]);
        let messages = system_exit_messages(&output);

        assert!(
            messages.is_empty(),
            "did not expect SYSTEM_EXIT finding for non-exit System call: {messages:?}"
        );
    }

    #[test]
    fn system_exit_allows_call_in_main_method() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/ClassC.java".to_string(),
            contents: r#"
package com.example;
public class ClassC {
    public static void main(String[] varOne) {
        System.exit(0);
    }
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, Language::Java, &sources, &[]);
        let messages = system_exit_messages(&output);

        assert!(
            messages.is_empty(),
            "did not expect SYSTEM_EXIT finding inside main: {messages:?}"
        );
    }

    #[test]
    fn system_exit_reports_kotlin_exit_process_outside_main() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/file_a.kt".to_string(),
            contents: r#"
package com.example

import kotlin.system.exitProcess

fun methodX(varOne: Boolean) {
    if (varOne) {
        exitProcess(1)
    }
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, Language::Kotlin, &sources, &[]);
        let messages = system_exit_messages(&output);

        assert!(
            messages
                .iter()
                .any(|msg| msg.contains("Avoid System.exit()")),
            "expected SYSTEM_EXIT finding for Kotlin exitProcess, got {messages:?}"
        );
    }

    #[test]
    fn system_exit_reports_kotlin_exit_process_zero_outside_main() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/file_c.kt".to_string(),
            contents: r#"
package com.example

import kotlin.system.exitProcess

fun methodY(varOne: Boolean) {
    if (varOne) {
        exitProcess(0)
    }
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, Language::Kotlin, &sources, &[]);
        let messages = system_exit_messages(&output);

        assert!(
            messages
                .iter()
                .any(|msg| msg.contains("Avoid System.exit()")),
            "expected SYSTEM_EXIT finding for Kotlin exitProcess(0), got {messages:?}"
        );
    }

    #[test]
    fn system_exit_allows_kotlin_top_level_main() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "com/example/file_b.kt".to_string(),
            contents: r#"
package com.example

import kotlin.system.exitProcess

fun main() {
    exitProcess(0)
}
"#
            .to_string(),
        }];

        let output = compile_and_analyze(&harness, Language::Kotlin, &sources, &[]);
        let messages = system_exit_messages(&output);

        assert!(
            messages.is_empty(),
            "did not expect SYSTEM_EXIT finding in Kotlin top-level main: {messages:?}"
        );
    }

    #[test]
    fn system_exit_ignores_classpath_calls() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");

        let dependency_sources = vec![SourceFile {
            path: "com/example/ClassB.java".to_string(),
            contents: r#"
package com.example;
public class ClassB {
    public void methodY() {
        System.exit(2);
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
        let messages = system_exit_messages(&analysis);

        assert!(
            messages.is_empty(),
            "classpath classes must be out of scope for SYSTEM_EXIT: {messages:?}"
        );
    }
}
