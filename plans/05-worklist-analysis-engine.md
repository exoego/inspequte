# Plan: Shared Worklist Analysis Engine

## Objective
Create a reusable worklist-based analysis engine for bytecode rules so rules can focus on rule logic rather than CFG traversal, queue handling, and convergence control.

## Background
Multiple rules perform similar control-flow iteration and state propagation. Duplicating this logic increases bug risk, makes convergence behavior inconsistent, and slows new rule development.

## Implementation Approach
- Add a shared module (for example `src/analysis/engine.rs`) containing:
  - Worklist loop
  - Per-block/per-instruction state scheduling
  - Visited-state tracking and deterministic ordering
  - Optional debug hooks
- Define a rule-facing interface:
  - Input: method CFG + initial state(s)
  - Callbacks: instruction transfer, terminal handling, branch successor filtering
  - Output: collected findings + final states (optional)
- Keep deterministic behavior:
  - Sorted successor traversal
  - Stable queue ordering
  - Stable key generation for visited states
- Migrate one existing rule (`exception_cause_not_preserved`) first, then one additional rule to validate generality.

## Test Cases
- Unit tests for engine behavior:
  - Single path
  - Branch merge
  - Loop convergence
  - Exception-edge traversal
- Rule regression tests:
  - Existing tests for migrated rules must remain green
  - No finding order instability in SARIF snapshots
- Performance checks:
  - Smoke test on SpotBugs jars
  - Compare timing before/after migration

## Success Criteria
- At least two rules run on the shared engine with no functional regressions.
- Rule modules delete bespoke worklist logic.
- Deterministic output is preserved across repeated runs.

## Dependencies
- Existing IR/CFG types in `src/ir.rs`.
- Rule metadata and SARIF result helpers in `src/rules/mod.rs`.

## Complexity Estimate
High

