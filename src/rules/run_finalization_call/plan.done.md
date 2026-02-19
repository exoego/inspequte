# Rule Plan: run_finalization_call

## Summary
Detect direct calls to finalization trigger APIs (`System.runFinalization`, `Runtime.runFinalization`).

## Problem framing
Explicitly triggering finalization is unpredictable and can create misleading cleanup assumptions. Modern JVM code should rely on deterministic resource management instead.

## Scope
- Analyze call sites in analysis target classes only.
- Report exact invocations of:
  - `java/lang/System.runFinalization()V`
  - `java/lang/Runtime.runFinalization()V`
- Emit one finding per matching call site with class/method context.

## Non-goals
- Do not infer application lifecycle policies.
- Do not add suppression semantics via `@Suppress` / `@SuppressWarnings`.
- Do not add non-JSpecify annotation semantics.

## Detection strategy
1. Iterate analysis target classes, methods, and call sites.
2. Match owner/name/descriptor exactly for target APIs.
3. Resolve source line from bytecode offset when available.
4. Emit deterministic findings in traversal order.

## Rule message
- Problem: explicit finalization trigger is unreliable.
- Fix: use explicit resource cleanup patterns (e.g. try-with-resources, close methods).

## Test strategy
- TP: `System.runFinalization()` is reported.
- TP: `Runtime.getRuntime().runFinalization()` is reported.
- TN: unrelated `Runtime` calls are not reported.
- Edge: classpath-only classes are ignored.

## Complexity and determinism
- Linear in number of call sites (`O(C)`).
- Deterministic by stable class/method/call iteration.

## Annotation policy
- `@Suppress`-style suppression remains unsupported.
- Annotation-driven semantics remain JSpecify-only.
- Non-JSpecify annotations do not affect behavior.

## Risks
- [ ] Some legacy systems may intentionally call these APIs.
- [ ] Rule cannot infer whether migration away from finalization is currently feasible.

## Post-Mortem
- Went well: exact signature matching for System/Runtime `runFinalization` provided deterministic and simple detection.
- Tricky: recommendation wording needed to avoid over-prescribing a specific resource-management framework.
- Follow-up: consider whether legacy-only compatibility contexts should be separately profiled for noise risk.
