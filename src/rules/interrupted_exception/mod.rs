use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use opentelemetry::KeyValue;
use serde_sarif::sarif::Result as SarifResult;

use crate::dataflow::worklist::{
    InstructionStep, WorklistSemantics, WorklistState, analyze_method,
};
use crate::engine::AnalysisContext;
use crate::ir::{Instruction, InstructionKind, Method};
use crate::opcodes;
use crate::rules::{Rule, RuleMetadata, method_location_with_line, result_message};

/// Rule that ensures InterruptedException handlers restore interrupt status.
#[derive(Default)]
pub(crate) struct InterruptedExceptionRule;

crate::register_rule!(InterruptedExceptionRule);

/// Program-point state used to enumerate instructions reachable from a handler.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct ReachableInstructionState {
    block_start: u32,
    instruction_index: usize,
}

impl WorklistState for ReachableInstructionState {
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

/// Dataflow callbacks for collecting instruction offsets reachable from a handler entry.
struct ReachableInstructionSemantics {
    handler_pc: u32,
}

impl WorklistSemantics for ReachableInstructionSemantics {
    type State = ReachableInstructionState;
    type Finding = u32;

    fn initial_states(&self, _method: &Method) -> Vec<Self::State> {
        vec![ReachableInstructionState {
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
        Ok(InstructionStep::continue_path().with_finding(instruction.offset))
    }
}

impl Rule for InterruptedExceptionRule {
    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: "INTERRUPTED_EXCEPTION_NOT_RESTORED",
            name: "InterruptedException not properly handled",
            description: "Restore interrupt status when catching InterruptedException",
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
                        let mut handled_handlers = BTreeSet::new();
                        for handler in &method.exception_handlers {
                            if !is_interrupted_exception_handler(handler.catch_type.as_deref()) {
                                continue;
                            }
                            if !handled_handlers.insert(handler.handler_pc) {
                                continue;
                            }
                            let instructions =
                                collect_reachable_instructions(method, handler.handler_pc)?;
                            if !handler_restores_interrupt(&instructions) {
                                let message = result_message(format!(
                                    "InterruptedException is caught but interrupt status is not restored in {}.{}{}",
                                    class.name, method.name, method.descriptor
                                ));
                                let line = method.line_for_offset(handler.handler_pc);
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

fn is_interrupted_exception_handler(catch_type: Option<&str>) -> bool {
    matches!(
        catch_type,
        Some("java/lang/InterruptedException")
            | Some("java/lang/Exception")
            | Some("java/lang/Throwable")
    )
}

fn collect_reachable_instructions<'a>(
    method: &'a Method,
    handler_pc: u32,
) -> Result<Vec<&'a Instruction>> {
    let semantics = ReachableInstructionSemantics { handler_pc };
    let instruction_offsets = analyze_method(method, &semantics)?;
    let mut instruction_map: BTreeMap<u32, &Instruction> = BTreeMap::new();
    for block in &method.cfg.blocks {
        for instruction in &block.instructions {
            instruction_map.insert(instruction.offset, instruction);
        }
    }

    Ok(instruction_offsets
        .into_iter()
        .filter_map(|offset| instruction_map.get(&offset).copied())
        .collect())
}

fn handler_restores_interrupt(instructions: &[&Instruction]) -> bool {
    let mut seen_current_thread = false;
    for instruction in instructions {
        if instruction.opcode == opcodes::ATHROW {
            return true;
        }
        let InstructionKind::Invoke(call) = &instruction.kind else {
            continue;
        };
        if call.owner == "java/lang/Thread"
            && call.name == "currentThread"
            && call.descriptor == "()Ljava/lang/Thread;"
        {
            seen_current_thread = true;
            continue;
        }
        if call.owner == "java/lang/Thread" && call.name == "interrupt" && call.descriptor == "()V"
        {
            if seen_current_thread {
                return true;
            }
        }
    }
    false
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
            .filter(|result| {
                result.rule_id.as_deref() == Some("INTERRUPTED_EXCEPTION_NOT_RESTORED")
            })
            .filter_map(|result| result.message.text.clone())
            .collect()
    }

    #[test]
    fn interrupted_exception_reports_missing_restore() {
        let sources = vec![SourceFile {
            path: "com/example/ClassAlpha.java".to_string(),
            contents: r#"
package com.example;

public class ClassAlpha {
    public void methodOne() {
        try {
            Thread.sleep(10);
        } catch (InterruptedException varOne) {
            System.out.println("Interrupted");
        }
    }
}
"#
            .to_string(),
        }];
        let messages = analyze_sources(sources);
        assert!(messages.iter().any(|msg| msg.contains("interrupt status")));
    }

    #[test]
    fn interrupted_exception_allows_restore() {
        let sources = vec![SourceFile {
            path: "com/example/ClassBeta.java".to_string(),
            contents: r#"
package com.example;

public class ClassBeta {
    public void methodTwo() {
        try {
            Thread.sleep(10);
        } catch (InterruptedException varOne) {
            Thread.currentThread().interrupt();
        }
    }
}
"#
            .to_string(),
        }];
        let messages = analyze_sources(sources);
        assert!(messages.is_empty());
    }

    #[test]
    fn interrupted_exception_allows_rethrow() {
        let sources = vec![SourceFile {
            path: "com/example/ClassGamma.java".to_string(),
            contents: r#"
package com.example;

public class ClassGamma {
    public void methodThree() {
        try {
            Thread.sleep(10);
        } catch (InterruptedException varOne) {
            throw new RuntimeException(varOne);
        }
    }
}
"#
            .to_string(),
        }];
        let messages = analyze_sources(sources);
        assert!(messages.is_empty());
    }

    #[test]
    fn interrupted_exception_allows_throw_declaration() {
        let sources = vec![SourceFile {
            path: "com/example/ClassEta.java".to_string(),
            contents: r#"
package com.example;

public class ClassEta {
    public void methodThree() throws InterruptedException {
        Thread.sleep(10);
    }
}
"#
            .to_string(),
        }];
        let messages = analyze_sources(sources);
        assert!(messages.is_empty());
    }

    #[test]
    fn interrupted_exception_reports_multi_catch() {
        let sources = vec![SourceFile {
            path: "com/example/ClassDelta.java".to_string(),
            contents: r#"
package com.example;

public class ClassDelta {
    public void methodFour() {
        try {
            Thread.sleep(10);
        } catch (InterruptedException | IllegalArgumentException varOne) {
            System.out.println(varOne);
        }
    }
}
"#
            .to_string(),
        }];
        let messages = analyze_sources(sources);
        assert!(messages.iter().any(|msg| msg.contains("interrupt status")));
    }

    #[test]
    fn interrupted_exception_reports_throwable_catch() {
        let sources = vec![SourceFile {
            path: "com/example/ClassTheta.java".to_string(),
            contents: r#"
package com.example;

public class ClassTheta {
    public void methodFive() {
        try {
            Thread.sleep(10);
        } catch (Throwable varOne) {
            System.out.println(varOne);
        }
    }
}
"#
            .to_string(),
        }];
        let messages = analyze_sources(sources);
        assert!(messages.iter().any(|msg| msg.contains("interrupt status")));
    }

    #[test]
    fn interrupted_exception_reports_exception_catch() {
        let sources = vec![SourceFile {
            path: "com/example/ClassEpsilon.java".to_string(),
            contents: r#"
package com.example;

public class ClassEpsilon {
    public void methodFive() {
        try {
            Thread.sleep(10);
        } catch (Exception varOne) {
            System.out.println(varOne);
        }
    }
}
"#
            .to_string(),
        }];
        let messages = analyze_sources(sources);
        assert!(messages.iter().any(|msg| msg.contains("interrupt status")));
    }

    #[test]
    fn interrupted_exception_allows_logging_and_restore() {
        let sources = vec![SourceFile {
            path: "com/example/ClassIota.java".to_string(),
            contents: r#"
package com.example;

public class ClassIota {
    public void methodSix() {
        try {
            Thread.sleep(10);
        } catch (InterruptedException varOne) {
            System.out.println(varOne);
            Thread.currentThread().interrupt();
        }
    }
}
"#
            .to_string(),
        }];
        let messages = analyze_sources(sources);
        assert!(messages.is_empty());
    }

    #[test]
    fn interrupted_exception_allows_conditional_restore() {
        let sources = vec![SourceFile {
            path: "com/example/ClassKappa.java".to_string(),
            contents: r#"
package com.example;

public class ClassKappa {
    public void methodSeven(boolean flagOne) {
        try {
            Thread.sleep(10);
        } catch (InterruptedException varOne) {
            if (flagOne) {
                Thread.currentThread().interrupt();
            }
        }
    }
}
"#
            .to_string(),
        }];
        let messages = analyze_sources(sources);
        assert!(messages.is_empty());
    }

    #[test]
    fn interrupted_exception_allows_finally_restore() {
        let sources = vec![SourceFile {
            path: "com/example/ClassZeta.java".to_string(),
            contents: r#"
package com.example;

public class ClassZeta {
    public void methodSix() {
        boolean flagOne = false;
        try {
            Thread.sleep(10);
        } catch (InterruptedException varOne) {
            flagOne = true;
        } finally {
            if (flagOne) {
                Thread.currentThread().interrupt();
            }
        }
    }
}
"#
            .to_string(),
        }];
        let messages = analyze_sources(sources);
        assert!(messages.is_empty());
    }
}
