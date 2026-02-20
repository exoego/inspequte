# Rule Plan: boolean_getboolean_call

## Summary
Detect direct calls to `Boolean.getBoolean(...)`.

## Problem framing
`Boolean.getBoolean(...)` reads JVM system properties, not text booleans from input values. It is often mistaken for `Boolean.parseBoolean(...)`.

## Scope
- Analyze call sites in analysis target classes only.
- Report direct calls to `java/lang/Boolean.getBoolean(Ljava/lang/String;)Z`.
- Emit one finding per matching call site with class/method context.

## Non-goals
- Do not infer whether system property reads are intended.
- Do not model configuration loading semantics.
- Do not report `Boolean.parseBoolean(...)` or `Boolean.valueOf(...)`.
- Do not add suppression semantics via `@Suppress` / `@SuppressWarnings`.
- Do not add non-JSpecify annotation semantics.

## Detection strategy
1. Iterate analysis target classes, methods, and call sites.
2. Match owner/name/descriptor exactly for `Boolean.getBoolean(String)`.
3. Resolve source line from bytecode offset when available.
4. Emit deterministic findings in traversal order.

## Rule message
- Problem: `Boolean.getBoolean(...)` reads system properties and can be a parse mistake.
- Fix: use `Boolean.parseBoolean(...)` for string parsing.

## Test strategy
- TP: `Boolean.getBoolean(String)` is reported.
- TN: `Boolean.parseBoolean(String)` is not reported.
- TN: `Boolean.valueOf(String)` is not reported.
- Edge: classpath-only calls are ignored.

## Complexity and determinism
- Linear in number of call sites (`O(C)`).
- Deterministic by stable class/method/call iteration.

## Annotation policy
- `@Suppress`-style suppression remains unsupported.
- Annotation-driven semantics remain JSpecify-only.
- Non-JSpecify annotations do not affect behavior.

## Risks
- [ ] Some code intentionally reads boolean-valued system properties and may be reported.
- [ ] Rule cannot infer intent when both parsing and property reads are plausible.

## Post-mortem
- What went well: single-signature matching kept the rule straightforward and deterministic.
- What was tricky: preserving precision while distinguishing parse intent vs property lookup intent.
- Follow-up: if needed, document migration guidance for system-property-specific use cases.
