# Rule Plan: system_exit

## Summary
Detect direct calls to `System.exit(int)` in analysis target classes.

## Problem framing
`System.exit` terminates the whole JVM process. In libraries and most services this causes abrupt shutdown, bypasses normal error handling, and can break calling applications.

## Scope
- Analyze call sites in analysis target classes only.
- Report exact invocations of:
  - `java/lang/System.exit(I)V`
- Emit one finding per matching call site with class/method context.

## Non-goals
- Do not report other process-termination APIs (`Runtime.halt`, `Runtime.exit`) in this rule.
- Do not infer whether calling `System.exit` is intentionally acceptable in CLI entry points.
- Do not add suppression semantics via `@Suppress` / `@SuppressWarnings`.
- Do not add non-JSpecify annotation semantics.

## Detection strategy
1. Iterate analysis target classes, then methods, then call sites.
2. Match owner/name/descriptor exactly against `System.exit(int)`.
3. Resolve source line from bytecode offset when line table data is available.
4. Emit deterministic findings in traversal order.

## Rule message
- Problem: `System.exit` terminates the JVM process from current code path.
- Fix: throw/propagate an exception or return an error status to the caller.

## Test strategy
- TP: direct `System.exit(1)` call is reported.
- TN: other `System` calls (for example `System.lineSeparator()`) are not reported.
- Edge: classpath-only classes using `System.exit` are ignored.

## Complexity and determinism
- Linear in number of call sites (`O(C)`).
- No CFG/dataflow required.
- Deterministic by stable class/method/call iteration.

## Annotation policy
- `@Suppress`-style suppression remains unsupported.
- Annotation-driven semantics remain JSpecify-only.
- Non-JSpecify annotations do not affect behavior.

## Risks
- [ ] Potential noise in true CLI-only entry-point code that intentionally exits the process.
- [ ] Missed detections where wrappers hide the direct `System.exit` call (accepted non-goal).
- [ ] Message wording must stay actionable and suggest practical alternatives.

## Post-Mortem
- Went well: exact owner/name/descriptor matching kept the rule implementation small, deterministic, and easy to test.
- Tricky: the SARIF callgraph snapshot required an update because adding a rule changes the tool metadata ordering/output.
- Follow-up: consider a companion rule for `Runtime.halt`/`Runtime.exit` once we define a separate noise policy for CLI applications.
