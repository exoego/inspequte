use anyhow::Result;
use opentelemetry::KeyValue;
use serde_sarif::sarif::Result as SarifResult;

use crate::engine::AnalysisContext;
use crate::rules::{Rule, RuleMetadata, method_location_with_line, result_message};

/// Rule that detects String case conversion calls without an explicit Locale.
#[derive(Default)]
pub(crate) struct StringCaseWithoutLocaleRule;

crate::register_rule!(StringCaseWithoutLocaleRule);

impl Rule for StringCaseWithoutLocaleRule {
    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: "STRING_CASE_WITHOUT_LOCALE",
            name: "String case conversion without explicit locale",
            description: "String.toLowerCase()/toUpperCase() calls without Locale argument",
        }
    }

    fn run(&self, context: &AnalysisContext) -> Result<Vec<SarifResult>> {
        let mut results = Vec::new();
        for class in &context.classes {
            if !context.is_analysis_target_class(class) {
                continue;
            }
            let mut attributes = vec![KeyValue::new("inspequte.class", class.name.clone())];
            if let Some(uri) = context.class_artifact_uri(class) {
                attributes.push(KeyValue::new("inspequte.artifact_uri", uri));
            }
            let class_results =
                context.with_span("class", &attributes, || -> Result<Vec<SarifResult>> {
                    let mut class_results = Vec::new();
                    for method in &class.methods {
                        let artifact_uri = context.class_artifact_uri(class);
                        for call in &method.calls {
                            if is_locale_less_case_call(call) {
                                let message = result_message(format!(
                                    "String case conversion in {}.{}{} uses default locale; pass Locale.ROOT (or an explicit Locale) to make behavior deterministic.",
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

fn is_locale_less_case_call(call: &crate::ir::CallSite) -> bool {
    call.owner == "java/lang/String"
        && call.descriptor == "()Ljava/lang/String;"
        && matches!(call.name.as_str(), "toLowerCase" | "toUpperCase")
}

#[cfg(test)]
mod tests {
    use crate::test_harness::{JvmTestHarness, Language, SourceFile};

    fn analyze_sources(sources: Vec<SourceFile>) -> Vec<String> {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let output = harness
            .compile_and_analyze(Language::Java, &sources, &[])
            .expect("run harness analysis");

        output
            .results
            .iter()
            .filter(|result| result.rule_id.as_deref() == Some("STRING_CASE_WITHOUT_LOCALE"))
            .filter_map(|result| result.message.text.clone())
            .collect()
    }

    #[test]
    fn reports_to_lower_and_to_upper_without_locale() {
        let sources = vec![SourceFile {
            path: "com/example/ClassA.java".to_string(),
            contents: r#"
package com.example;

class ClassA {
    String methodX(String varOne) {
        String varTwo = varOne.toLowerCase();
        String varThree = varOne.toUpperCase();
        return varTwo + varThree;
    }
}
"#
            .to_string(),
        }];

        let messages = analyze_sources(sources);

        assert_eq!(
            messages.len(),
            2,
            "expected two findings, got: {messages:?}"
        );
    }

    #[test]
    fn does_not_report_locale_aware_overloads() {
        let sources = vec![SourceFile {
            path: "com/example/ClassA.java".to_string(),
            contents: r#"
package com.example;

import java.util.Locale;

class ClassA {
    String methodX(String varOne) {
        String varTwo = varOne.toLowerCase(Locale.ROOT);
        String varThree = varOne.toUpperCase(Locale.ROOT);
        return varTwo + varThree;
    }
}
"#
            .to_string(),
        }];

        let messages = analyze_sources(sources);

        assert!(
            messages.is_empty(),
            "expected no findings, got: {messages:?}"
        );
    }

    #[test]
    fn reports_only_locale_less_call_in_mixed_case() {
        let sources = vec![SourceFile {
            path: "com/example/ClassA.java".to_string(),
            contents: r#"
package com.example;

import java.util.Locale;

class ClassA {
    String methodX(String varOne) {
        String varTwo = varOne.toLowerCase();
        String varThree = varOne.toUpperCase(Locale.ROOT);
        return varTwo + varThree;
    }
}
"#
            .to_string(),
        }];

        let messages = analyze_sources(sources);

        assert_eq!(messages.len(), 1, "expected one finding, got: {messages:?}");
        assert!(messages[0].contains("uses default locale"));
    }
}
