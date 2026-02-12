# Plan: Trait-Based Abstract Domains for Rule Analysis

## Objective
Define trait-based abstract domains so rules can reuse a common analysis engine while customizing value/state semantics without inheritance.

## Background
Rust favors composition over inheritance. A trait-based design allows independent evolution of engine mechanics and rule-specific abstract interpretation logic.

## Implementation Approach
- Define core traits (example names):
  - `AbstractValue`: merge/join behavior and normalization
  - `AbstractState`: state key and state-level join/canonicalization
  - `TransferDomain`: instruction transfer and terminal handling
- Keep traits small and focused:
  - Avoid monolithic trait with many optional methods
  - Provide default helper implementations in separate utility modules
- Implement one concrete domain for `exception_cause_not_preserved`.
- Document patterns for future rule authors:
  - Which trait to implement for value-only customization
  - When to define custom state key logic

## Test Cases
- Trait conformance tests for a toy domain.
- Real rule migration test for `exception_cause_not_preserved`.
- Determinism tests ensuring equivalent states normalize consistently.

## Success Criteria
- Rule logic compiles against trait contracts with no inheritance-like base class.
- Adding a new domain requires implementing only relevant traits.
- Existing migrated rule behavior remains unchanged.

## Dependencies
- Shared worklist engine design.
- Shared stack machine or equivalent state container.

## Complexity Estimate
Medium-High

