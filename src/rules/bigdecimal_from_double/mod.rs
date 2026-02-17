use anyhow::Result;
use opentelemetry::KeyValue;
use serde_sarif::sarif::Result as SarifResult;

use crate::engine::AnalysisContext;
use crate::rules::{Rule, RuleMetadata, method_location_with_line, result_message};

/// Rule that detects BigDecimal constructor calls that accept double values.
#[derive(Default)]
pub(crate) struct BigDecimalFromDoubleRule;

crate::register_rule!(BigDecimalFromDoubleRule);

impl Rule for BigDecimalFromDoubleRule {
    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: "BIGDECIMAL_FROM_DOUBLE",
            name: "BigDecimal from double",
            description: "BigDecimal constructors with double can introduce precision surprises",
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
                context.with_span("rule.class", &attributes, || -> Result<Vec<SarifResult>> {
                    let mut class_results = Vec::new();
                    for method in &class.methods {
                        for call in &method.calls {
                            if !is_bigdecimal_double_constructor(&call.owner, &call.name, &call.descriptor)
                            {
                                continue;
                            }
                            let message = result_message(
                                "BigDecimal constructed from double can lose precision. Use BigDecimal.valueOf(double) or a decimal string constructor.",
                            );
                            let line = method.line_for_offset(call.offset);
                            let artifact_uri = context.class_artifact_uri(class);
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

fn is_bigdecimal_double_constructor(owner: &str, name: &str, descriptor: &str) -> bool {
    owner == "java/math/BigDecimal"
        && name == "<init>"
        && matches!(descriptor, "(D)V" | "(DLjava/math/MathContext;)V")
}

#[cfg(test)]
mod tests {
    use crate::test_harness::{JvmTestHarness, Language, SourceFile};

    fn analyze_messages(sources: Vec<SourceFile>) -> Vec<String> {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let output = harness
            .compile_and_analyze(Language::Java, &sources, &[])
            .expect("run harness analysis");

        output
            .results
            .iter()
            .filter(|result| result.rule_id.as_deref() == Some("BIGDECIMAL_FROM_DOUBLE"))
            .filter_map(|result| result.message.text.clone())
            .collect()
    }

    #[test]
    fn bigdecimal_from_double_reports_constructor_with_double_literal() {
        let sources = vec![SourceFile {
            path: "example/ClassA.java".to_string(),
            contents: r#"
package example;

import java.math.BigDecimal;

public class ClassA {
    public BigDecimal MethodX() {
        return new BigDecimal(0.1d);
    }
}
"#
            .to_string(),
        }];

        let messages = analyze_messages(sources);
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("can lose precision"));
    }

    #[test]
    fn bigdecimal_from_double_ignores_value_of() {
        let sources = vec![SourceFile {
            path: "example/ClassB.java".to_string(),
            contents: r#"
package example;

import java.math.BigDecimal;

public class ClassB {
    public BigDecimal MethodY() {
        return BigDecimal.valueOf(0.1d);
    }
}
"#
            .to_string(),
        }];

        let messages = analyze_messages(sources);
        assert!(
            messages.is_empty(),
            "expected no findings, got {messages:?}"
        );
    }

    #[test]
    fn bigdecimal_from_double_ignores_string_constructor() {
        let sources = vec![SourceFile {
            path: "example/ClassC.java".to_string(),
            contents: r#"
package example;

import java.math.BigDecimal;

public class ClassC {
    public BigDecimal MethodZ() {
        return new BigDecimal("0.1");
    }
}
"#
            .to_string(),
        }];

        let messages = analyze_messages(sources);
        assert!(
            messages.is_empty(),
            "expected no findings, got {messages:?}"
        );
    }

    #[test]
    fn bigdecimal_from_double_reports_constructor_with_math_context() {
        let sources = vec![SourceFile {
            path: "example/ClassD.java".to_string(),
            contents: r#"
package example;

import java.math.BigDecimal;
import java.math.MathContext;

public class ClassD {
    public BigDecimal MethodW(double varOne) {
        return new BigDecimal(varOne, MathContext.DECIMAL64);
    }
}
"#
            .to_string(),
        }];

        let messages = analyze_messages(sources);
        assert_eq!(messages.len(), 1);
    }
}
