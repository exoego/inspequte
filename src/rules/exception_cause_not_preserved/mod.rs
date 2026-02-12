use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use anyhow::Result;
use opentelemetry::KeyValue;
use serde_sarif::sarif::Result as SarifResult;

use crate::dataflow::worklist::{
    BlockEndStep, InstructionStep, WorklistSemantics, WorklistState, analyze_method,
};
use crate::descriptor::{ReturnKind, method_param_count, method_return_kind};
use crate::engine::AnalysisContext;
use crate::ir::{CallKind, CallSite, Instruction, InstructionKind, Method};
use crate::opcodes;
use crate::rules::{Rule, RuleMetadata, method_location_with_line, result_message};

const MAX_TRACKED_STACK_DEPTH: usize = 24;
const MAX_TRACKED_ALLOCATIONS: usize = 4;

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
    locals: BTreeMap<usize, Value>,
    preserved_allocations: BTreeSet<u32>,
}

impl WorklistState for ExecutionState {
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

/// Dataflow callbacks for catch-handler symbolic execution.
struct HandlerSemantics {
    handler_pc: u32,
    debug_enabled: bool,
    stack_depth_dumped: Cell<bool>,
}

impl HandlerSemantics {
    fn new(handler_pc: u32) -> Self {
        Self {
            handler_pc,
            debug_enabled: debug_stack_dump_enabled(),
            stack_depth_dumped: Cell::new(false),
        }
    }
}

impl WorklistSemantics for HandlerSemantics {
    type State = ExecutionState;
    type Finding = u32;

    fn initial_states(&self, _method: &Method) -> Vec<Self::State> {
        vec![ExecutionState {
            block_start: self.handler_pc,
            instruction_index: 0,
            stack: vec![Value::Caught],
            locals: BTreeMap::new(),
            preserved_allocations: BTreeSet::new(),
        }]
    }

    fn canonicalize_state(&self, state: &mut Self::State) {
        canonicalize_state(state);
    }

    fn transfer_instruction(
        &self,
        method: &Method,
        instruction: &Instruction,
        state: &mut Self::State,
    ) -> Result<InstructionStep<Self::Finding>> {
        if is_return_opcode(instruction.opcode) {
            apply_stack_effect(method, instruction, state)?;
            return Ok(InstructionStep::terminate_path());
        }

        if instruction.opcode == opcodes::ATHROW {
            let thrown = pop_or_unknown(&mut state.stack);
            if let Value::New(allocation_offset) = thrown
                && !state.preserved_allocations.contains(&allocation_offset)
            {
                return Ok(InstructionStep::terminate_path().with_finding(instruction.offset));
            }
            return Ok(InstructionStep::terminate_path());
        }

        apply_stack_effect(method, instruction, state)?;
        prune_preserved_allocations(state);
        if self.debug_enabled
            && !self.stack_depth_dumped.get()
            && state.stack.len() >= MAX_TRACKED_STACK_DEPTH
        {
            dump_stack_depth(method, self.handler_pc, instruction, state);
            self.stack_depth_dumped.set(true);
        }

        Ok(InstructionStep::continue_path())
    }

    fn on_block_end(
        &self,
        _method: &Method,
        state: &Self::State,
        successors: &[u32],
    ) -> Result<BlockEndStep<Self::State, Self::Finding>> {
        Ok(BlockEndStep::follow_all_successors(state, successors))
    }
}

fn handler_offsets(method: &Method) -> Vec<u32> {
    let mut offsets = BTreeSet::new();
    for handler in &method.exception_handlers {
        offsets.insert(handler.handler_pc);
    }
    offsets.into_iter().collect()
}

fn analyze_handler(method: &Method, handler_pc: u32) -> Result<Vec<u32>> {
    let semantics = HandlerSemantics::new(handler_pc);
    let findings = analyze_method(method, &semantics)?;
    Ok(findings
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
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
                push_stack(state, Value::Other);
                return Ok(());
            };
            push_stack(
                state,
                state.locals.get(&index).copied().unwrap_or(Value::Other),
            );
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
            let value = pop_or_unknown(&mut state.stack);
            match value {
                Value::Caught | Value::New(_) => {
                    state.locals.insert(index, value);
                }
                Value::Other => {
                    state.locals.remove(&index);
                }
            }
        }
        opcodes::NEW => {
            push_stack(state, Value::New(instruction.offset));
        }
        opcodes::DUP => {
            if let Some(value) = state.stack.last().copied() {
                push_stack(state, value);
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
            push_stack(state, Value::Other);
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
            push_stack(state, Value::Other);
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
        // Primitive and non-reference loads.
        0x15..=0x18 | 0x1a..=0x29 => {
            push_stack(state, Value::Other);
        }
        // Primitive array loads.
        0x2e..=0x31 | 0x33..=0x35 => {
            pop_n(&mut state.stack, 2);
            push_stack(state, Value::Other);
        }
        // Primitive stores.
        0x36 | 0x38 | 0x3b..=0x3e | 0x43..=0x46 => {
            pop_n(&mut state.stack, 1);
        }
        0x37 | 0x39 | 0x3f..=0x42 | 0x47..=0x4a => {
            pop_n(&mut state.stack, 2);
        }
        // Primitive array stores.
        0x4f..=0x52 | 0x54..=0x56 => {
            pop_n(&mut state.stack, 3);
        }
        // Stack shuffling opcodes.
        0x5a..=0x5e => {
            push_stack(state, Value::Other);
        }
        0x5f => {
            let right = pop_or_unknown(&mut state.stack);
            let left = pop_or_unknown(&mut state.stack);
            push_stack(state, right);
            push_stack(state, left);
        }
        // Primitive arithmetic.
        0x60..=0x73 | 0x78..=0x83 | 0x94..=0x98 => {
            pop_n(&mut state.stack, 2);
            push_stack(state, Value::Other);
        }
        0x74..=0x77 | 0x85..=0x93 => {
            pop_n(&mut state.stack, 1);
            push_stack(state, Value::Other);
        }
        // iinc has no stack effect.
        0x84 => {}
        // Legacy subroutine opcodes.
        opcodes::JSR | opcodes::JSR_W => {
            push_stack(state, Value::Other);
        }
        opcodes::GOTO | opcodes::GOTO_W => {}
        // Field access.
        0xb2 => {
            push_stack(state, Value::Other);
        }
        0xb3 => {
            pop_n(&mut state.stack, 1);
        }
        0xb4 => {
            pop_n(&mut state.stack, 1);
            push_stack(state, Value::Other);
        }
        0xb5 => {
            pop_n(&mut state.stack, 2);
        }
        // INVOKEDYNAMIC callsites are currently not decoded into CallSite.
        opcodes::INVOKEDYNAMIC => {
            pop_n(&mut state.stack, 1);
            push_stack(state, Value::Other);
        }
        // Array/type/monitor opcodes.
        opcodes::NEWARRAY | opcodes::ANEWARRAY | opcodes::ARRAYLENGTH | 0xc0 | 0xc1 => {
            pop_n(&mut state.stack, 1);
            push_stack(state, Value::Other);
        }
        0xc2 | 0xc3 => {
            pop_n(&mut state.stack, 1);
        }
        opcodes::MULTIANEWARRAY => {
            let dims = method
                .bytecode
                .get(instruction.offset as usize + 3)
                .copied()
                .unwrap_or(1);
            pop_n(&mut state.stack, dims as usize);
            push_stack(state, Value::Other);
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
        push_stack(state, value);
    }

    Ok(())
}

fn push_stack(state: &mut ExecutionState, value: Value) {
    // Keep the state space finite even when unsupported opcodes appear inside loops.
    if state.stack.len() >= MAX_TRACKED_STACK_DEPTH {
        state.stack.remove(0);
    }
    state.stack.push(value);
}

fn prune_preserved_allocations(state: &mut ExecutionState) {
    let mut live_allocations = BTreeSet::new();
    for value in state.stack.iter().chain(state.locals.values()) {
        if let Value::New(offset) = value {
            live_allocations.insert(*offset);
        }
    }
    let tracked_allocations: BTreeSet<u32> = live_allocations
        .iter()
        .rev()
        .take(MAX_TRACKED_ALLOCATIONS)
        .copied()
        .collect();

    for value in &mut state.stack {
        if let Value::New(offset) = *value
            && !tracked_allocations.contains(&offset)
        {
            *value = Value::Other;
        }
    }
    state.locals.retain(|_, value| match *value {
        Value::Caught => true,
        Value::New(offset) => tracked_allocations.contains(&offset),
        Value::Other => false,
    });
    state
        .preserved_allocations
        .retain(|offset| tracked_allocations.contains(offset));
}

fn debug_stack_dump_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("INSPEQUTE_DEBUG_EXCEPTION_CAUSE_STACK")
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

fn dump_stack_depth(
    method: &Method,
    handler_pc: u32,
    instruction: &Instruction,
    state: &ExecutionState,
) {
    if !debug_stack_dump_enabled() {
        return;
    }

    eprintln!(
        "exception_cause_not_preserved debug: stack depth reached limit method={}{} handler_pc={} offset={} opcode=0x{:02x} depth={} top={:?}",
        method.name,
        method.descriptor,
        handler_pc,
        instruction.offset,
        instruction.opcode,
        state.stack.len(),
        state.stack.iter().rev().take(8).collect::<Vec<_>>()
    );
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

fn pop_or_unknown(stack: &mut Vec<Value>) -> Value {
    stack.pop().unwrap_or(Value::Other)
}

fn pop_n(stack: &mut Vec<Value>, count: usize) {
    for _ in 0..count {
        pop_or_unknown(stack);
    }
}

fn canonicalize_state(state: &mut ExecutionState) {
    let mut mapping: BTreeMap<u32, u32> = BTreeMap::new();
    let mut next_id = 0u32;

    for value in &state.stack {
        if let Value::New(offset) = *value {
            mapping.entry(offset).or_insert_with(|| {
                let id = next_id;
                next_id += 1;
                id
            });
        }
    }
    for value in state.locals.values() {
        if let Value::New(offset) = *value {
            mapping.entry(offset).or_insert_with(|| {
                let id = next_id;
                next_id += 1;
                id
            });
        }
    }
    for offset in &state.preserved_allocations {
        mapping.entry(*offset).or_insert_with(|| {
            let id = next_id;
            next_id += 1;
            id
        });
    }

    if mapping.is_empty() {
        return;
    }

    for value in &mut state.stack {
        if let Value::New(offset) = *value
            && let Some(mapped) = mapping.get(&offset)
        {
            *value = Value::New(*mapped);
        }
    }
    for value in state.locals.values_mut() {
        if let Value::New(offset) = *value
            && let Some(mapped) = mapping.get(&offset)
        {
            *value = Value::New(*mapped);
        }
    }
    state.preserved_allocations = state
        .preserved_allocations
        .iter()
        .filter_map(|offset| mapping.get(offset).copied())
        .collect();
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

    #[test]
    fn exception_cause_reports_after_primitive_loop_in_catch() {
        let sources = vec![SourceFile {
            path: "com/example/ClassH.java".to_string(),
            contents: r#"
package com.example;

public class ClassH {
    public void methodEight(int varTwo) {
        try {
            MethodX();
        } catch (Exception varOne) {
            int varThree = 0;
            while (varThree < varTwo) {
                varThree++;
            }
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
}
