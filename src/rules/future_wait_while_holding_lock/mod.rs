use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use anyhow::{Context, Result};
use jdescriptor::{MethodDescriptor, TypeDescriptor};
use opentelemetry::KeyValue;
use serde_sarif::sarif::Result as SarifResult;

use crate::dataflow::worklist::{
    InstructionStep, WorklistSemantics, WorklistState, analyze_method,
};
use crate::descriptor::{ReturnKind, method_return_kind};
use crate::engine::AnalysisContext;
use crate::ir::{CallKind, CallSite, FieldRef, Instruction, InstructionKind, Method};
use crate::opcodes;
use crate::rules::{Rule, RuleMetadata, method_location_with_line, result_message};

/// Rule that detects future waits while the current method still holds a lock.
#[derive(Default)]
pub(crate) struct FutureWaitWhileHoldingLockRule;

crate::register_rule!(FutureWaitWhileHoldingLockRule);

impl Rule for FutureWaitWhileHoldingLockRule {
    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: "FUTURE_WAIT_WHILE_HOLDING_LOCK",
            name: "Future wait while holding lock",
            description: "Blocking Future waits should not happen while a lock is still held",
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
                    let artifact_uri = context.class_artifact_uri(class);
                    for method in &class.methods {
                        if method.bytecode.is_empty() {
                            continue;
                        }
                        if !method_needs_analysis(method) {
                            continue;
                        }
                        for offset in reportable_wait_offsets(method)? {
                            let message = result_message(
                                "Do not wait on a Future while holding a lock; release the lock before calling get()/join(), or move the wait outside the synchronized or locked section.",
                            );
                            let line = method.line_for_offset(offset);
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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Value {
    Unknown,
    Other,
    This,
    Lock(LockIdentity),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum LockIdentity {
    StaticField {
        owner: String,
        name: String,
        descriptor: String,
    },
    ThisField {
        owner: String,
        name: String,
        descriptor: String,
    },
    Param(usize),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct WaitState {
    block_start: u32,
    instruction_index: usize,
    monitor_depth: u8,
    held_locks: BTreeSet<LockIdentity>,
    locals: Vec<Value>,
    stack: Vec<Value>,
}

impl WorklistState for WaitState {
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct WaitObservation {
    offset: u32,
    lock_held: bool,
}

/// Worklist semantics that record whether a wait site is reached with a lock held on every path.
struct FutureWaitSemantics {
    initial_locals: Vec<Value>,
}

impl WorklistSemantics for FutureWaitSemantics {
    type State = WaitState;
    type Finding = WaitObservation;

    fn initial_states(&self, method: &Method) -> Vec<Self::State> {
        vec![WaitState {
            block_start: method
                .cfg
                .blocks
                .first()
                .map(|block| block.start_offset)
                .unwrap_or(0),
            instruction_index: 0,
            monitor_depth: u8::from(method.access.is_synchronized),
            held_locks: BTreeSet::new(),
            locals: self.initial_locals.clone(),
            stack: Vec::new(),
        }]
    }

    fn canonicalize_state(&self, state: &mut Self::State) {
        while matches!(state.locals.last(), Some(Value::Unknown)) {
            state.locals.pop();
        }
    }

    fn transfer_instruction(
        &self,
        method: &Method,
        instruction: &Instruction,
        state: &mut Self::State,
    ) -> Result<InstructionStep<Self::Finding>> {
        match &instruction.kind {
            InstructionKind::Invoke(call) => return transfer_invoke(call, state),
            InstructionKind::FieldAccess(field) => {
                transfer_field_access(instruction.opcode, field, state);
                return Ok(InstructionStep::continue_path());
            }
            _ => {}
        }

        match instruction.opcode {
            opcodes::ACONST_NULL => state.stack.push(Value::Unknown),
            opcodes::ALOAD => {
                let index = local_index_operand(method, instruction).unwrap_or(0);
                state.stack.push(load_local(&state.locals, index));
            }
            opcodes::ALOAD_0 | opcodes::ALOAD_1 | opcodes::ALOAD_2 | opcodes::ALOAD_3 => {
                let index = (instruction.opcode - opcodes::ALOAD_0) as usize;
                state.stack.push(load_local(&state.locals, index));
            }
            opcodes::ASTORE => {
                let index = local_index_operand(method, instruction).unwrap_or(0);
                store_local(&mut state.locals, index, pop_value(&mut state.stack));
            }
            opcodes::ASTORE_0 | opcodes::ASTORE_1 | opcodes::ASTORE_2 | opcodes::ASTORE_3 => {
                let index = (instruction.opcode - opcodes::ASTORE_0) as usize;
                store_local(&mut state.locals, index, pop_value(&mut state.stack));
            }
            opcodes::DUP => {
                if let Some(value) = state.stack.last().cloned() {
                    state.stack.push(value);
                }
            }
            opcodes::POP => {
                pop_value(&mut state.stack);
            }
            opcodes::POP2 => {
                pop_value(&mut state.stack);
                pop_value(&mut state.stack);
            }
            opcodes::MONITORENTER => {
                pop_value(&mut state.stack);
                state.monitor_depth = state.monitor_depth.saturating_add(1);
            }
            opcodes::MONITOREXIT => {
                pop_value(&mut state.stack);
                state.monitor_depth = state.monitor_depth.saturating_sub(1);
            }
            opcodes::NEW => state.stack.push(Value::Other),
            _ => {}
        }

        Ok(InstructionStep::continue_path())
    }
}

fn reportable_wait_offsets(method: &Method) -> Result<Vec<u32>> {
    let semantics = FutureWaitSemantics {
        initial_locals: initial_locals(method)?,
    };
    let observations = analyze_method(method, &semantics)?;
    let mut by_offset = BTreeMap::<u32, (bool, bool)>::new();
    for observation in observations {
        let entry = by_offset.entry(observation.offset).or_insert((false, false));
        if observation.lock_held {
            entry.0 = true;
        } else {
            entry.1 = true;
        }
    }

    Ok(by_offset
        .into_iter()
        .filter_map(|(offset, (seen_locked, seen_unlocked))| {
            (seen_locked && !seen_unlocked).then_some(offset)
        })
        .collect())
}

fn transfer_invoke(
    call: &CallSite,
    state: &mut WaitState,
) -> Result<InstructionStep<WaitObservation>> {
    let receiver = receiver_value(call, &state.stack).cloned();
    let mut step = InstructionStep::continue_path();

    if is_wait_call(call) {
        step = step.with_finding(WaitObservation {
            offset: call.offset,
            lock_held: state.monitor_depth > 0 || !state.held_locks.is_empty(),
        });
    } else if is_lock_call(call) {
        if let Some(Value::Lock(lock)) = receiver {
            state.held_locks.insert(lock);
        }
    } else if is_unlock_call(call)
        && let Some(Value::Lock(lock)) = receiver
    {
        state.held_locks.remove(&lock);
    }

    pop_invoke_operands(call, &mut state.stack);
    if method_return_kind(&call.descriptor)? != ReturnKind::Void {
        state.stack.push(Value::Other);
    }
    Ok(step)
}

fn method_needs_analysis(method: &Method) -> bool {
    let has_wait = method.calls.iter().any(is_wait_call);
    if !has_wait {
        return false;
    }

    method.access.is_synchronized
        || method
            .bytecode
            .iter()
            .any(|opcode| matches!(*opcode, opcodes::MONITORENTER | opcodes::MONITOREXIT))
        || method
            .calls
            .iter()
            .any(|call| is_lock_call(call) || is_unlock_call(call))
}

fn transfer_field_access(opcode: u8, field: &FieldRef, state: &mut WaitState) {
    match opcode {
        opcodes::GETFIELD => {
            let receiver = pop_value(&mut state.stack);
            state.stack.push(value_for_instance_field(field, &receiver));
        }
        opcodes::GETSTATIC => {
            state.stack.push(value_for_static_field(field));
        }
        opcodes::PUTFIELD => {
            pop_value(&mut state.stack);
            pop_value(&mut state.stack);
        }
        opcodes::PUTSTATIC => {
            pop_value(&mut state.stack);
        }
        _ => {}
    }
}

fn value_for_instance_field(field: &FieldRef, receiver: &Value) -> Value {
    if is_lock_descriptor(&field.descriptor) {
        if matches!(receiver, Value::This) {
            return Value::Lock(LockIdentity::ThisField {
                owner: field.owner.clone(),
                name: field.name.clone(),
                descriptor: field.descriptor.clone(),
            });
        }
    }
    Value::Other
}

fn value_for_static_field(field: &FieldRef) -> Value {
    if is_lock_descriptor(&field.descriptor) {
        Value::Lock(LockIdentity::StaticField {
            owner: field.owner.clone(),
            name: field.name.clone(),
            descriptor: field.descriptor.clone(),
        })
    } else {
        Value::Other
    }
}

fn initial_locals(method: &Method) -> Result<Vec<Value>> {
    let descriptor =
        MethodDescriptor::from_str(&method.descriptor).context("parse method descriptor")?;
    let mut locals = Vec::new();
    if !method.access.is_static {
        locals.push(Value::This);
    }
    for parameter in descriptor.parameter_types() {
        let local_index = locals.len();
        let value = if is_lock_type(parameter) {
            Value::Lock(LockIdentity::Param(local_index))
        } else {
            Value::Other
        };
        locals.push(value);
        if matches!(parameter, TypeDescriptor::Long | TypeDescriptor::Double) {
            locals.push(Value::Unknown);
        }
    }
    Ok(locals)
}

fn is_lock_type(ty: &TypeDescriptor) -> bool {
    matches!(
        ty,
        TypeDescriptor::Object(name) if is_lock_owner(name)
    )
}

fn is_lock_descriptor(descriptor: &str) -> bool {
    descriptor
        .strip_prefix('L')
        .and_then(|value| value.strip_suffix(';'))
        .is_some_and(is_lock_owner)
}

fn is_wait_call(call: &CallSite) -> bool {
    match (call.name.as_str(), call.descriptor.as_str()) {
        ("get", "()Ljava/lang/Object;")
        | ("get", "(JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;") => {
            is_future_owner(&call.owner)
        }
        ("join", "()Ljava/lang/Object;") => call.owner == "java/util/concurrent/CompletableFuture",
        _ => false,
    }
}

fn is_lock_call(call: &CallSite) -> bool {
    call.name == "lock" && call.descriptor == "()V" && is_lock_owner(&call.owner)
}

fn is_unlock_call(call: &CallSite) -> bool {
    call.name == "unlock" && call.descriptor == "()V" && is_lock_owner(&call.owner)
}

fn is_future_owner(owner: &str) -> bool {
    owner == "java/util/concurrent/Future"
        || owner == "java/util/concurrent/FutureTask"
        || owner == "java/util/concurrent/ForkJoinTask"
        || (owner.starts_with("java/util/concurrent/") && owner.ends_with("Future"))
}

fn is_lock_owner(owner: &str) -> bool {
    owner
        .rsplit('/')
        .next()
        .is_some_and(|simple| simple.ends_with("Lock"))
}

fn receiver_value<'a>(call: &CallSite, stack: &'a [Value]) -> Option<&'a Value> {
    if call.kind == CallKind::Static {
        return None;
    }
    let param_count = MethodDescriptor::from_str(&call.descriptor)
        .map(|descriptor| descriptor.parameter_types().len())
        .unwrap_or(0);
    stack
        .len()
        .checked_sub(param_count + 1)
        .and_then(|index| stack.get(index))
}

fn pop_invoke_operands(call: &CallSite, stack: &mut Vec<Value>) {
    let param_count = MethodDescriptor::from_str(&call.descriptor)
        .map(|descriptor| descriptor.parameter_types().len())
        .unwrap_or(0);
    for _ in 0..param_count {
        pop_value(stack);
    }
    if call.kind != CallKind::Static {
        pop_value(stack);
    }
}

fn local_index_operand(method: &Method, instruction: &Instruction) -> Option<usize> {
    method
        .bytecode
        .get(instruction.offset as usize + 1)
        .copied()
        .map(|value| value as usize)
}

fn load_local(locals: &[Value], index: usize) -> Value {
    locals.get(index).cloned().unwrap_or(Value::Unknown)
}

fn store_local(locals: &mut Vec<Value>, index: usize, value: Value) {
    if index >= locals.len() {
        locals.resize(index + 1, Value::Unknown);
    }
    locals[index] = value;
}

fn pop_value(stack: &mut Vec<Value>) -> Value {
    stack.pop().unwrap_or(Value::Unknown)
}

#[cfg(test)]
mod tests {
    use crate::test_harness::{JvmTestHarness, Language, SourceFile};

    fn messages_for_sources(sources: Vec<SourceFile>) -> Vec<String> {
        let harness = JvmTestHarness::new().expect("JAVA_HOME must be set for harness tests");
        let output = harness
            .compile_and_analyze(Language::Java, &sources, &[])
            .expect("run harness analysis");
        output
            .results
            .iter()
            .filter(|result| {
                result.rule_id.as_deref() == Some("FUTURE_WAIT_WHILE_HOLDING_LOCK")
            })
            .filter_map(|result| result.message.text.clone())
            .collect()
    }

    #[test]
    fn reports_future_get_inside_synchronized_block() {
        let sources = vec![SourceFile {
            path: "com/example/ClassA.java".to_string(),
            contents: r#"
package com.example;

import java.util.concurrent.Future;

public class ClassA {
    private final Object varOne = new Object();

    public void methodX(Future<String> varTwo) throws Exception {
        synchronized (varOne) {
            varTwo.get();
        }
    }
}
"#
            .to_string(),
        }];

        let messages = messages_for_sources(sources);
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("Do not wait on a Future while holding a lock"));
    }

    #[test]
    fn reports_join_inside_synchronized_method() {
        let sources = vec![SourceFile {
            path: "com/example/ClassB.java".to_string(),
            contents: r#"
package com.example;

import java.util.concurrent.CompletableFuture;

public class ClassB {
    public synchronized void methodY(CompletableFuture<String> varOne) {
        varOne.join();
    }
}
"#
            .to_string(),
        }];

        let messages = messages_for_sources(sources);
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn reports_join_while_explicit_lock_is_held() {
        let sources = vec![SourceFile {
            path: "com/example/ClassC.java".to_string(),
            contents: r#"
package com.example;

import java.util.concurrent.CompletableFuture;
import java.util.concurrent.locks.Lock;

public class ClassC {
    public void methodZ(Lock varOne, CompletableFuture<String> varTwo) {
        varOne.lock();
        try {
            varTwo.join();
        } finally {
            varOne.unlock();
        }
    }
}
"#
            .to_string(),
        }];

        let messages = messages_for_sources(sources);
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn ignores_future_wait_after_unlock() {
        let sources = vec![SourceFile {
            path: "com/example/ClassD.java".to_string(),
            contents: r#"
package com.example;

import java.util.concurrent.Future;
import java.util.concurrent.locks.Lock;

public class ClassD {
    public void methodW(Lock varOne, Future<String> varTwo) throws Exception {
        varOne.lock();
        try {
            System.out.println("tmpValue");
        } finally {
            varOne.unlock();
        }
        varTwo.get();
    }
}
"#
            .to_string(),
        }];

        let messages = messages_for_sources(sources);
        assert!(messages.is_empty());
    }

    #[test]
    fn ignores_join_outside_synchronized_block() {
        let sources = vec![SourceFile {
            path: "com/example/ClassE.java".to_string(),
            contents: r#"
package com.example;

import java.util.concurrent.CompletableFuture;

public class ClassE {
    private final Object varOne = new Object();

    public void methodV(CompletableFuture<String> varTwo) {
        synchronized (varOne) {
            System.out.println("tmpValue");
        }
        varTwo.join();
    }
}
"#
            .to_string(),
        }];

        let messages = messages_for_sources(sources);
        assert!(messages.is_empty());
    }

    #[test]
    fn reports_timed_get_inside_synchronized_block() {
        let sources = vec![SourceFile {
            path: "com/example/ClassF.java".to_string(),
            contents: r#"
package com.example;

import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;

public class ClassF {
    private final Object varOne = new Object();

    public void methodU(Future<String> varTwo) throws Exception {
        synchronized (varOne) {
            varTwo.get(10L, TimeUnit.SECONDS);
        }
    }
}
"#
            .to_string(),
        }];

        let messages = messages_for_sources(sources);
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn reports_future_get_for_concrete_future_owner() {
        let sources = vec![SourceFile {
            path: "com/example/ClassG.java".to_string(),
            contents: r#"
package com.example;

import java.util.concurrent.FutureTask;

public class ClassG {
    private final Object varOne = new Object();

    public void methodT(FutureTask<String> varTwo) throws Exception {
        synchronized (varOne) {
            varTwo.get();
        }
    }
}
"#
            .to_string(),
        }];

        let messages = messages_for_sources(sources);
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn skips_ambiguous_lock_aliasing() {
        let sources = vec![SourceFile {
            path: "com/example/ClassH.java".to_string(),
            contents: r#"
package com.example;

import java.util.concurrent.Future;
import java.util.concurrent.locks.Lock;

public class ClassH {
    public void methodT(Lock varOne, Lock varTwo, Future<String> varThree) throws Exception {
        Lock tmpValue = pick(varOne, varTwo);
        tmpValue.lock();
        try {
            varThree.get();
        } finally {
            tmpValue.unlock();
        }
    }

    private Lock pick(Lock varOne, Lock varTwo) {
        return System.nanoTime() > 0 ? varOne : varTwo;
    }
}
"#
            .to_string(),
        }];

        let messages = messages_for_sources(sources);
        assert!(messages.is_empty());
    }

    #[test]
    fn ignores_non_blocking_future_api() {
        let sources = vec![SourceFile {
            path: "com/example/ClassI.java".to_string(),
            contents: r#"
package com.example;

import java.util.concurrent.CompletableFuture;
import java.util.concurrent.locks.Lock;

public class ClassI {
    public Object methodS(Lock varOne, CompletableFuture<String> varTwo) {
        varOne.lock();
        try {
            return varTwo.getNow("tmpValue");
        } finally {
            varOne.unlock();
        }
    }
}
"#
            .to_string(),
        }];

        let messages = messages_for_sources(sources);
        assert!(messages.is_empty());
    }

    #[test]
    fn reports_wait_while_holding_lock_implementor() {
        let sources = vec![SourceFile {
            path: "com/example/ClassJ.java".to_string(),
            contents: r#"
package com.example;

import java.util.concurrent.Future;
import java.util.concurrent.locks.ReentrantReadWriteLock;

public class ClassJ {
    public void methodR(ReentrantReadWriteLock.ReadLock varOne, Future<String> varTwo) throws Exception {
        varOne.lock();
        try {
            varTwo.get();
        } finally {
            varOne.unlock();
        }
    }
}
"#
            .to_string(),
        }];

        let messages = messages_for_sources(sources);
        assert_eq!(messages.len(), 1);
    }
}
