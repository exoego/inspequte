# Rule Plan: url_hashcode_call

## Summary
Detect direct calls to `URL.hashCode()`.

## Problem framing
`URL.hashCode()` can trigger host resolution and produce unstable behavior for equality/hash-based collections in network-sensitive environments.

## Scope
- Analyze call sites in analysis target classes only.
- Report direct calls to `java/net/URL.hashCode()I`.
- Emit one finding per matching call site with class/method context.

## Non-goals
- Do not infer whether DNS/network access is available or stable at runtime.
- Do not model collection usage beyond direct call detection.
- Do not report URI hash operations.
- Do not add suppression semantics via `@Suppress` / `@SuppressWarnings`.
- Do not add non-JSpecify annotation semantics.

## Detection strategy
1. Iterate analysis target classes, methods, and call sites.
2. Match owner/name/descriptor exactly for `URL.hashCode()`.
3. Resolve source line from bytecode offset when available.
4. Emit deterministic findings in traversal order.

## Rule message
- Problem: `URL.hashCode()` may rely on host resolution.
- Fix: convert to normalized `URI` or hash explicit URL components.

## Test strategy
- TP: `URL.hashCode()` is reported.
- TN: `URI.hashCode()` is not reported.
- Edge: classpath-only calls are ignored.

## Complexity and determinism
- Linear in number of call sites (`O(C)`).
- Deterministic by stable class/method/call iteration.

## Annotation policy
- `@Suppress`-style suppression remains unsupported.
- Annotation-driven semantics remain JSpecify-only.
- Non-JSpecify annotations do not affect behavior.

## Risks
- [ ] Some code paths intentionally accept URL hash semantics for legacy behavior.
- [ ] Rule does not verify whether host resolution side effects are actually observed at runtime.

## Post-mortem
- What went well: `URL.hashCode()` is a precise bytecode target, so implementation stayed simple and deterministic.
- What was tricky: keeping the guidance actionable without implying guaranteed DNS side effects in every runtime.
- Follow-up: if user feedback requests it, consider documenting migration examples to normalized `URI` hashing.
