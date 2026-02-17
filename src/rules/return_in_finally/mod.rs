use std::collections::BTreeSet;

use anyhow::Result;
use opentelemetry::KeyValue;
use serde_sarif::sarif::Result as SarifResult;

use crate::dataflow::worklist::{
    InstructionStep, WorklistSemantics, WorklistState, analyze_method,
};
use crate::engine::AnalysisContext;
use crate::ir::{Instruction, Method};
use crate::opcodes;
use crate::rules::{Rule, RuleMetadata, method_location_with_line, result_message};

/// Rule that detects return statements executed inside finally blocks.
#[derive(Default)]
pub(crate) struct ReturnInFinallyRule;

crate::register_rule!(ReturnInFinallyRule);

/// Program-point state used for finally-handler return scanning.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct ReturnScanState {
    block_start: u32,
    instruction_index: usize,
}

impl WorklistState for ReturnScanState {
    fn block_start(&self) -> u32 {
        self.block_start
    }

    fn instruction_index(&self) -> usize {
        self.instruction_index
    }

    fn set_position(&mut self, block_start: u32, instruction_index: usize) {
        self.block_start = block_start;
        self.instruction_index = instruction_index;
    }
}

/// Dataflow callbacks that extract return instruction offsets from a handler region.
struct ReturnScanSemantics {
    handler_pc: u32,
}

impl WorklistSemantics for ReturnScanSemantics {
    type State = ReturnScanState;
    type Finding = u32;

    fn initial_states(&self, _method: &Method) -> Vec<Self::State> {
        vec![ReturnScanState {
            block_start: self.handler_pc,
            instruction_index: 0,
        }]
    }

    fn transfer_instruction(
        &self,
        _method: &Method,
        instruction: &Instruction,
        _state: &mut Self::State,
    ) -> Result<InstructionStep<Self::Finding>> {
        if is_return_opcode(instruction.opcode) {
            return Ok(InstructionStep::continue_path().with_finding(instruction.offset));
        }
        Ok(InstructionStep::continue_path())
    }
}

impl Rule for ReturnInFinallyRule {
    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: "RETURN_IN_FINALLY",
            name: "Return in finally",
            description: "Return statements in finally blocks override exceptions or prior returns",
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
                        if method.bytecode.is_empty() {
                            continue;
                        }

                        let handler_offsets = finally_handler_offsets(method);
                        if handler_offsets.is_empty() {
                            continue;
                        }

                        let mut seen_offsets = BTreeSet::new();

                        for handler_pc in handler_offsets {
                            for instruction_offset in return_offsets_in_handler(method, handler_pc)? {
                                if !seen_offsets.insert(instruction_offset) {
                                    continue;
                                }
                                let message = result_message(
                                    "Return in finally overrides exceptions or prior returns. Move the return outside the finally block or return after the try/finally.",
                                );
                                let line = method.line_for_offset(instruction_offset);
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
                    }
                    Ok(class_results)
                })?;

            results.extend(class_results);
        }
        Ok(results)
    }
}

fn finally_handler_offsets(method: &Method) -> Vec<u32> {
    let mut offsets: Vec<u32> = method
        .exception_handlers
        .iter()
        .filter(|handler| handler.catch_type.is_none())
        .map(|handler| handler.handler_pc)
        .collect();
    offsets.sort();
    offsets.dedup();
    offsets
}

fn return_offsets_in_handler(method: &Method, handler_pc: u32) -> Result<Vec<u32>> {
    let semantics = ReturnScanSemantics { handler_pc };
    let findings = analyze_method(method, &semantics)?;
    Ok(findings
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}

fn is_return_opcode(opcode: u8) -> bool {
    matches!(
        opcode,
        opcodes::IRETURN
            | opcodes::LRETURN
            | opcodes::FRETURN
            | opcodes::DRETURN
            | opcodes::ARETURN
            | opcodes::RETURN
    )
}

#[cfg(test)]
mod tests {
    use crate::engine::EngineOutput;
    use crate::test_harness::{JvmTestHarness, Language, SourceFile};

    fn return_messages(output: &EngineOutput) -> Vec<String> {
        output
            .results
            .iter()
            .filter(|result| result.rule_id.as_deref() == Some("RETURN_IN_FINALLY"))
            .filter_map(|result| result.message.text.clone())
            .collect()
    }

    #[test]
    fn return_in_finally_reports_return_in_finally_block() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "example/ClassA.java".to_string(),
            contents: r#"
package example;

public class ClassA {
    int MethodX() {
        try {
            throw new RuntimeException("fail");
        } finally {
            return 1;
        }
    }
}
"#
            .to_string(),
        }];

        let analysis = harness
            .compile_and_analyze(Language::Java, &sources, &[])
            .expect("compile and analyze");

        let messages = return_messages(&analysis);
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("Return in finally overrides exceptions"));
    }

    #[test]
    fn return_in_finally_ignores_return_after_try_finally() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "example/ClassB.java".to_string(),
            contents: r#"
package example;

public class ClassB {
    int MethodY() {
        int varOne;
        try {
            varOne = 1;
        } finally {
            varOne = 2;
        }
        return varOne;
    }
}
"#
            .to_string(),
        }];

        let analysis = harness
            .compile_and_analyze(Language::Java, &sources, &[])
            .expect("compile and analyze");

        let messages = return_messages(&analysis);
        assert!(
            messages.is_empty(),
            "expected no RETURN_IN_FINALLY, got {messages:?}"
        );
    }

    #[test]
    fn return_in_finally_reports_return_overriding_prior_return() {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let sources = vec![SourceFile {
            path: "example/ClassC.java".to_string(),
            contents: r#"
package example;

public class ClassC {
    int MethodZ() {
        try {
            return 1;
        } finally {
            return 2;
        }
    }
}
"#
            .to_string(),
        }];

        let analysis = harness
            .compile_and_analyze(Language::Java, &sources, &[])
            .expect("compile and analyze");

        let messages = return_messages(&analysis);
        assert_eq!(messages.len(), 1);
    }
}
