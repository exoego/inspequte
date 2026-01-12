#![allow(dead_code)]

/// Intermediate representation for parsed JVM classes and methods.
#[derive(Clone, Debug)]
pub(crate) struct Class {
    pub(crate) name: String,
    pub(crate) super_name: Option<String>,
    pub(crate) referenced_classes: Vec<String>,
    pub(crate) methods: Vec<Method>,
    pub(crate) artifact_index: i64,
}

/// Intermediate representation for a method and its bytecode.
#[derive(Clone, Debug)]
pub(crate) struct Method {
    pub(crate) name: String,
    pub(crate) descriptor: String,
    pub(crate) blocks: Vec<BasicBlock>,
    pub(crate) calls: Vec<CallSite>,
}

/// Basic block covering a range of bytecode offsets.
#[derive(Clone, Debug)]
pub(crate) struct BasicBlock {
    pub(crate) start_offset: u32,
    pub(crate) end_offset: u32,
    pub(crate) instructions: Vec<Instruction>,
}

/// Bytecode instruction captured for analysis.
#[derive(Clone, Debug)]
pub(crate) struct Instruction {
    pub(crate) offset: u32,
    pub(crate) kind: InstructionKind,
}

/// Instruction kinds needed for call graph construction.
#[derive(Clone, Debug)]
pub(crate) enum InstructionKind {
    Invoke(CallSite),
    Other(u8),
}

/// Call site extracted from bytecode.
#[derive(Clone, Debug)]
pub(crate) struct CallSite {
    pub(crate) owner: String,
    pub(crate) name: String,
    pub(crate) descriptor: String,
    pub(crate) kind: CallKind,
    pub(crate) offset: u32,
}

/// Call opcode classification used by CHA.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum CallKind {
    Virtual,
    Interface,
    Special,
    Static,
}
