use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::Result;
use opentelemetry::KeyValue;
use serde_sarif::sarif::Result as SarifResult;

use crate::descriptor::{ReturnKind, method_param_count, method_return_kind};
use crate::engine::AnalysisContext;
use crate::ir::{BasicBlock, CallKind, CallSite, Instruction, InstructionKind, Method};
use crate::opcodes;
use crate::rules::{Rule, RuleMetadata, method_location_with_line, result_message};

/// Rule that detects catch handlers that drop the original exception cause.
#[derive(Default)]
pub(crate) struct ExceptionCauseNotPreservedRule;

crate::register_rule!(ExceptionCauseNotPreservedRule);

impl Rule for ExceptionCauseNotPreservedRule {
    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: "EXCEPTION_CAUSE_NOT_PRESERVED",
            name: "Exception cause not preserved",
            description: "Catch handlers that throw new exceptions without preserving the cause",
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
                context.with_span("rule.class", &attributes, || -> Result<Vec<SarifResult>> {
                    let mut class_results = Vec::new();
                    for method in &class.methods {
                        if method.bytecode.is_empty() {
                            continue;
                        }

                        let mut seen_findings = BTreeSet::new();
                        for handler_pc in handler_offsets(method) {
                            for throw_offset in analyze_handler(method, handler_pc)? {
                                if !seen_findings.insert((handler_pc, throw_offset)) {
                                    continue;
                                }

                                let message = result_message(
                                    "Catch handler throws a new exception without preserving the original cause; pass the caught exception as a cause or call initCause/addSuppressed before throwing.",
                                );
                                let line = method.line_for_offset(throw_offset);
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Value {
    Unknown,
    Other,
    Caught,
    New(u32),
}

/// Symbolic execution state at a specific instruction position.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct ExecutionState {
    block_start: u32,
    instruction_index: usize,
    stack: Vec<Value>,
    locals: Vec<Value>,
    preserved_allocations: BTreeSet<u32>,
}

fn handler_offsets(method: &Method) -> Vec<u32> {
    let mut offsets = BTreeSet::new();
    for handler in &method.exception_handlers {
        offsets.insert(handler.handler_pc);
    }
    offsets.into_iter().collect()
}

fn analyze_handler(method: &Method, handler_pc: u32) -> Result<Vec<u32>> {
    let block_map = block_map(method);
    if !block_map.contains_key(&handler_pc) {
        return Ok(Vec::new());
    }

    let successor_map = successor_map(method);
    let mut queue = VecDeque::new();
    let mut visited = BTreeSet::new();
    let mut findings = BTreeSet::new();

    queue.push_back(ExecutionState {
        block_start: handler_pc,
        instruction_index: 0,
        stack: vec![Value::Caught],
        locals: Vec::new(),
        preserved_allocations: BTreeSet::new(),
    });

    while let Some(state) = queue.pop_front() {
        if !visited.insert(state.clone()) {
            continue;
        }

        let Some(block) = block_map.get(&state.block_start) else {
            continue;
        };

        if state.instruction_index >= block.instructions.len() {
            enqueue_successors(&mut queue, &successor_map, state);
            continue;
        }

        let instruction = &block.instructions[state.instruction_index];
        let mut next_state = state.clone();
        next_state.instruction_index += 1;

        if is_return_opcode(instruction.opcode) {
            apply_stack_effect(method, instruction, &mut next_state)?;
            continue;
        }

        if instruction.opcode == opcodes::ATHROW {
            let thrown = pop_or_unknown(&mut next_state.stack);
            if let Value::New(allocation_offset) = thrown {
                if !next_state
                    .preserved_allocations
                    .contains(&allocation_offset)
                {
                    findings.insert(instruction.offset);
                }
            }
            continue;
        }

        apply_stack_effect(method, instruction, &mut next_state)?;

        if next_state.instruction_index < block.instructions.len() {
            queue.push_back(next_state);
        } else {
            enqueue_successors(&mut queue, &successor_map, next_state);
        }
    }

    Ok(findings.into_iter().collect())
}

fn apply_stack_effect(
    method: &Method,
    instruction: &Instruction,
    state: &mut ExecutionState,
) -> Result<()> {
    match instruction.opcode {
        opcodes::ALOAD
        | opcodes::ALOAD_0
        | opcodes::ALOAD_1
        | opcodes::ALOAD_2
        | opcodes::ALOAD_3 => {
            let Some(index) = local_index_for(method, instruction) else {
                state.stack.push(Value::Unknown);
                return Ok(());
            };
            ensure_local(&mut state.locals, index);
            state.stack.push(state.locals[index]);
        }
        opcodes::ASTORE
        | opcodes::ASTORE_0
        | opcodes::ASTORE_1
        | opcodes::ASTORE_2
        | opcodes::ASTORE_3 => {
            let Some(index) = local_index_for(method, instruction) else {
                pop_or_unknown(&mut state.stack);
                return Ok(());
            };
            ensure_local(&mut state.locals, index);
            state.locals[index] = pop_or_unknown(&mut state.stack);
        }
        opcodes::NEW => {
            state.stack.push(Value::New(instruction.offset));
        }
        opcodes::DUP => {
            if let Some(value) = state.stack.last().copied() {
                state.stack.push(value);
            }
        }
        opcodes::POP => {
            pop_or_unknown(&mut state.stack);
        }
        opcodes::POP2 => {
            pop_or_unknown(&mut state.stack);
            pop_or_unknown(&mut state.stack);
        }
        opcodes::AALOAD => {
            pop_or_unknown(&mut state.stack);
            pop_or_unknown(&mut state.stack);
            state.stack.push(Value::Other);
        }
        opcodes::AASTORE => {
            pop_or_unknown(&mut state.stack);
            pop_or_unknown(&mut state.stack);
            pop_or_unknown(&mut state.stack);
        }
        opcodes::ACONST_NULL
        | opcodes::ICONST_M1
        | opcodes::ICONST_0
        | opcodes::ICONST_1
        | opcodes::ICONST_2
        | opcodes::ICONST_3
        | opcodes::ICONST_4
        | opcodes::ICONST_5
        | opcodes::BIPUSH
        | opcodes::SIPUSH
        | opcodes::LDC
        | opcodes::LDC_W
        | opcodes::LDC2_W => {
            state.stack.push(Value::Other);
        }
        opcodes::IFEQ
        | opcodes::IFNE
        | opcodes::IFLT
        | opcodes::IFGE
        | opcodes::IFGT
        | opcodes::IFLE
        | opcodes::IFNULL
        | opcodes::IFNONNULL
        | opcodes::TABLESWITCH
        | opcodes::LOOKUPSWITCH => {
            pop_or_unknown(&mut state.stack);
        }
        opcodes::IF_ICMPEQ
        | opcodes::IF_ICMPNE
        | opcodes::IF_ICMPLT
        | opcodes::IF_ICMPGE
        | opcodes::IF_ICMPGT
        | opcodes::IF_ICMPLE
        | opcodes::IF_ACMPEQ
        | opcodes::IF_ACMPNE => {
            pop_or_unknown(&mut state.stack);
            pop_or_unknown(&mut state.stack);
        }
        _ => {}
    }

    if let InstructionKind::Invoke(call) = &instruction.kind {
        handle_invoke(call, state)?;
    }

    Ok(())
}

fn handle_invoke(call: &CallSite, state: &mut ExecutionState) -> Result<()> {
    let param_count = method_param_count(&call.descriptor)?;
    let mut args = Vec::with_capacity(param_count);
    for _ in 0..param_count {
        args.push(pop_or_unknown(&mut state.stack));
    }

    let receiver = if call.kind == CallKind::Static {
        None
    } else {
        Some(pop_or_unknown(&mut state.stack))
    };

    let has_caught_argument = args.iter().any(|value| matches!(value, Value::Caught));

    if call.name == "<init>" {
        if let Some(Value::New(allocation_offset)) = receiver {
            if has_caught_argument {
                state.preserved_allocations.insert(allocation_offset);
            }
        }
        return Ok(());
    }

    let mut return_value = match method_return_kind(&call.descriptor)? {
        ReturnKind::Void => None,
        ReturnKind::Primitive | ReturnKind::Reference => Some(Value::Other),
    };

    if call.name == "initCause" {
        if has_caught_argument {
            if let Some(Value::New(allocation_offset)) = receiver {
                state.preserved_allocations.insert(allocation_offset);
            }
        }
        return_value = receiver;
    } else if call.name == "addSuppressed"
        && has_caught_argument
        && let Some(Value::New(allocation_offset)) = receiver
    {
        state.preserved_allocations.insert(allocation_offset);
    }

    if let Some(value) = return_value {
        state.stack.push(value);
    }

    Ok(())
}

fn enqueue_successors(
    queue: &mut VecDeque<ExecutionState>,
    successor_map: &BTreeMap<u32, Vec<u32>>,
    state: ExecutionState,
) {
    if let Some(successors) = successor_map.get(&state.block_start) {
        for successor in successors {
            let mut successor_state = state.clone();
            successor_state.block_start = *successor;
            successor_state.instruction_index = 0;
            queue.push_back(successor_state);
        }
    }
}

fn block_map(method: &Method) -> BTreeMap<u32, &BasicBlock> {
    let mut map = BTreeMap::new();
    for block in &method.cfg.blocks {
        map.insert(block.start_offset, block);
    }
    map
}

fn successor_map(method: &Method) -> BTreeMap<u32, Vec<u32>> {
    let mut map: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for edge in &method.cfg.edges {
        map.entry(edge.from).or_default().push(edge.to);
    }
    for targets in map.values_mut() {
        targets.sort();
        targets.dedup();
    }
    map
}

fn local_index_for(method: &Method, instruction: &Instruction) -> Option<usize> {
    let offset = instruction.offset as usize;
    match instruction.opcode {
        opcodes::ASTORE | opcodes::ALOAD => method
            .bytecode
            .get(offset + 1)
            .copied()
            .map(|value| value as usize),
        opcodes::ASTORE_0 | opcodes::ALOAD_0 => Some(0),
        opcodes::ASTORE_1 | opcodes::ALOAD_1 => Some(1),
        opcodes::ASTORE_2 | opcodes::ALOAD_2 => Some(2),
        opcodes::ASTORE_3 | opcodes::ALOAD_3 => Some(3),
        _ => None,
    }
}

fn ensure_local(locals: &mut Vec<Value>, index: usize) {
    if locals.len() <= index {
        locals.resize(index + 1, Value::Unknown);
    }
}

fn pop_or_unknown(stack: &mut Vec<Value>) -> Value {
    stack.pop().unwrap_or(Value::Unknown)
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
    use crate::test_harness::{JvmTestHarness, Language, SourceFile};

    fn analyze_sources(sources: Vec<SourceFile>) -> Vec<String> {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let output = harness
            .compile_and_analyze(Language::Java, &sources, &[])
            .expect("run harness analysis");
        output
            .results
            .iter()
            .filter(|result| result.rule_id.as_deref() == Some("EXCEPTION_CAUSE_NOT_PRESERVED"))
            .filter_map(|result| result.message.text.clone())
            .collect()
    }

    #[test]
    fn exception_cause_reports_missing_cause() {
        let sources = vec![SourceFile {
            path: "com/example/ClassA.java".to_string(),
            contents: r#"
package com.example;

public class ClassA {
    public void methodOne() {
        try {
            MethodX();
        } catch (Exception varOne) {
            throw new RuntimeException("failed");
        }
    }

    private void MethodX() {
        throw new IllegalStateException("boom");
    }
}
"#
            .to_string(),
        }];

        let messages = analyze_sources(sources);
        assert!(messages.iter().any(|msg| msg.contains("original cause")));
    }

    #[test]
    fn exception_cause_allows_constructor_cause() {
        let sources = vec![SourceFile {
            path: "com/example/ClassB.java".to_string(),
            contents: r#"
package com.example;

public class ClassB {
    public void methodTwo() {
        try {
            MethodX();
        } catch (Exception varOne) {
            throw new RuntimeException("failed", varOne);
        }
    }

    private void MethodX() {
        throw new IllegalStateException("boom");
    }
}
"#
            .to_string(),
        }];

        let messages = analyze_sources(sources);
        assert!(messages.is_empty());
    }

    #[test]
    fn exception_cause_allows_rethrow() {
        let sources = vec![SourceFile {
            path: "com/example/ClassC.java".to_string(),
            contents: r#"
package com.example;

public class ClassC {
    public void methodThree() throws Exception {
        try {
            MethodX();
        } catch (Exception varOne) {
            throw varOne;
        }
    }

    private void MethodX() {
        throw new IllegalStateException("boom");
    }
}
"#
            .to_string(),
        }];

        let messages = analyze_sources(sources);
        assert!(messages.is_empty());
    }

    #[test]
    fn exception_cause_allows_init_cause() {
        let sources = vec![SourceFile {
            path: "com/example/ClassD.java".to_string(),
            contents: r#"
package com.example;

public class ClassD {
    public void methodFour() {
        try {
            MethodX();
        } catch (Exception varOne) {
            RuntimeException varTwo = new RuntimeException("failed");
            varTwo.initCause(varOne);
            throw varTwo;
        }
    }

    private void MethodX() {
        throw new IllegalStateException("boom");
    }
}
"#
            .to_string(),
        }];

        let messages = analyze_sources(sources);
        assert!(messages.is_empty());
    }

    #[test]
    fn exception_cause_reports_path_without_preserve() {
        let sources = vec![SourceFile {
            path: "com/example/ClassE.java".to_string(),
            contents: r#"
package com.example;

public class ClassE {
    public void methodFive(boolean varTwo) {
        try {
            MethodX();
        } catch (Exception varOne) {
            RuntimeException varThree = new RuntimeException("failed");
            if (varTwo) {
                varThree.initCause(varOne);
            }
            throw varThree;
        }
    }

    private void MethodX() {
        throw new IllegalStateException("boom");
    }
}
"#
            .to_string(),
        }];

        let messages = analyze_sources(sources);
        assert!(messages.iter().any(|msg| msg.contains("original cause")));
    }

    #[test]
    fn exception_cause_allows_add_suppressed() {
        let sources = vec![SourceFile {
            path: "com/example/ClassF.java".to_string(),
            contents: r#"
package com.example;

public class ClassF {
    public void methodSix() {
        try {
            MethodX();
        } catch (Exception varOne) {
            RuntimeException varTwo = new RuntimeException("failed");
            varTwo.addSuppressed(varOne);
            throw varTwo;
        }
    }

    private void MethodX() {
        throw new IllegalStateException("boom");
    }
}
"#
            .to_string(),
        }];

        let messages = analyze_sources(sources);
        assert!(messages.is_empty());
    }

    #[test]
    fn exception_cause_ignores_external_exception_instance() {
        let sources = vec![SourceFile {
            path: "com/example/ClassG.java".to_string(),
            contents: r#"
package com.example;

public class ClassG {
    public void methodSeven() {
        RuntimeException varTwo = new RuntimeException("failed");
        try {
            MethodX();
        } catch (Exception varOne) {
            throw varTwo;
        }
    }

    private void MethodX() {
        throw new IllegalStateException("boom");
    }
}
"#
            .to_string(),
        }];

        let messages = analyze_sources(sources);
        assert!(messages.is_empty());
    }
}
