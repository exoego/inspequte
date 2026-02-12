# Plan: Shared Analysis Budgets and Diagnostics

## Objective
Standardize analysis safety controls (state budgets, stack budgets, diagnostics) as reusable infrastructure rather than ad-hoc per-rule guards.

## Background
Complex bytecode can trigger state explosion. Consistent safety limits and observability are required for predictable runtime and easier production debugging.

## Implementation Approach
- Introduce shared budget config:
  - Max stack depth
  - Max tracked symbolic identities
  - Optional max states or per-method step budget (configurable)
- Add structured diagnostics:
  - Debug env flag for budget-hit dumps
  - Include method signature, handler/offset, and compact state summary
- Ensure non-debug mode has low overhead:
  - Fast-path checks
  - Avoid expensive stats collection unless enabled
- Expose budget policy through shared analysis APIs so all compatible rules can adopt it.

## Test Cases
- Unit tests for each budget boundary behavior.
- Regression tests to ensure findings still produced for common paths.
- Smoke test to verify no infinite loops and acceptable runtime on SpotBugs jars.
- Determinism tests ensuring budgets do not produce random output ordering.

## Success Criteria
- Budget logic is not duplicated across rule files.
- Debug diagnostics can be enabled/disabled uniformly.
- Smoke tests remain stable with budgets enabled.

## Dependencies
- Shared analysis engine and state abstractions.
- Telemetry/logging conventions already used in the project.

## Complexity Estimate
Medium

