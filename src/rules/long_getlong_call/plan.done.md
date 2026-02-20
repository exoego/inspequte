# Rule Plan: long_getlong_call

## Summary
Detect direct calls to `Long.getLong(...)`.

## Problem framing
`Long.getLong(...)` reads JVM system properties, not numeric string values. It is commonly confused with `Long.parseLong(...)`/`Long.valueOf(...)`.

## Scope
- Analyze call sites in analysis target classes only.
- Report direct calls to `java/lang/Long.getLong` overloads:
  - `(Ljava/lang/String;)Ljava/lang/Long;`
  - `(Ljava/lang/String;J)Ljava/lang/Long;`
  - `(Ljava/lang/String;Ljava/lang/Long;)Ljava/lang/Long;`
- Emit one finding per matching call site with class/method context.

## Non-goals
- Do not infer developer intent from naming/context.
- Do not model whether system properties are intentionally used.
- Do not report `Long.parseLong(...)` or `Long.valueOf(...)`.
- Do not add suppression semantics via `@Suppress` / `@SuppressWarnings`.
- Do not add non-JSpecify annotation semantics.

## Detection strategy
1. Iterate analysis target classes, methods, and call sites.
2. Match owner/name/descriptor exactly for `Long.getLong(...)` overloads.
3. Resolve source line from bytecode offset when available.
4. Emit deterministic findings in traversal order.

## Rule message
- Problem: `Long.getLong(...)` reads system properties and can be a parse mistake.
- Fix: use `Long.parseLong(...)`/`Long.valueOf(...)` for numeric parsing.

## Test strategy
- TP: `Long.getLong(String)` is reported.
- TP: `Long.getLong(String, long)` is reported.
- TN: `Long.parseLong(String)` is not reported.
- Edge: classpath-only calls are ignored.

## Complexity and determinism
- Linear in number of call sites (`O(C)`).
- Deterministic by stable class/method/call iteration.

## Annotation policy
- `@Suppress`-style suppression remains unsupported.
- Annotation-driven semantics remain JSpecify-only.
- Non-JSpecify annotations do not affect behavior.

## Risks
- [ ] Some code intentionally reads long-valued system properties and may be reported.
- [ ] Rule cannot infer intent when both parsing and property reads are plausible.

## Post-mortem
- What went well: overload-specific matching provided a clear and deterministic implementation.
- What was tricky: wording the fix to preserve legitimate system-property use cases.
- Follow-up: if FP volume appears, evaluate optional heuristics based on literal property key patterns.
