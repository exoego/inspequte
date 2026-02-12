# Plan: Table-Driven Opcode Semantics with Rule Hooks

## Objective
Create a table-driven opcode semantics layer for common JVM instructions and allow rule-specific semantic hooks for special cases.

## Background
Per-rule opcode `match` blocks duplicate generic stack effects and leave coverage gaps. A shared opcode semantics table improves consistency and makes unsupported instructions visible.

## Implementation Approach
- Add shared opcode semantics (for example `src/analysis/opcode_semantics.rs`):
  - Map opcode -> default effect descriptor
  - Descriptor includes stack pop count, push kind, and local side effects when generic
- Provide extension points:
  - Pre-hook and post-hook around default semantics
  - Rule-specific handling for invoke targets, domain-specific APIs, and throw semantics
- Add instrumentation:
  - Optional debug mode to log unknown or overridden opcodes
  - Coverage counters for semantic fallback usage
- Replace duplicated generic opcode handling in selected rules.

## Test Cases
- Unit tests:
  - Verify opcode effects for representative categories (load/store/arith/field/array/control)
  - Verify hook precedence and fallback behavior
- Integration tests:
  - Migrated rules keep existing expected findings
  - Debug output appears only when enabled

## Success Criteria
- Generic opcode behavior is defined in one shared table.
- Rules only implement truly rule-specific semantics.
- Unsupported-opcode regressions become detectable via tests/logging.

## Dependencies
- Stack machine abstraction to apply effects.
- Opcode constants in `src/opcodes.rs`.

## Complexity Estimate
Medium

