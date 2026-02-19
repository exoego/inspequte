# Rule Plan: bigdecimal_equals_call

## Summary
Detect direct calls to `BigDecimal.equals(Object)` and recommend numeric comparison via `compareTo`.

## Problem framing
`BigDecimal.equals` compares both value and scale. This often causes surprising behavior when code intends numeric equality.

## Scope
- Analyze call sites in analysis target classes only.
- Report exact invocations of `java/math/BigDecimal.equals(Ljava/lang/Object;)Z`.
- Emit one finding per matching call site with class/method context.

## Non-goals
- Do not infer business-domain intent for scale-sensitive comparisons.
- Do not add suppression semantics via `@Suppress` / `@SuppressWarnings`.
- Do not add non-JSpecify annotation semantics.

## Detection strategy
1. Iterate analysis target classes, methods, and call sites.
2. Match owner/name/descriptor exactly for `BigDecimal.equals(Object)`.
3. Resolve source line from bytecode offset when available.
4. Emit deterministic findings in traversal order.

## Rule message
- Problem: `BigDecimal.equals` may fail for numerically equal values with different scale.
- Fix: use `compareTo(...) == 0` for numeric equality checks.

## Test strategy
- TP: `varOne.equals(varTwo)` where both are `BigDecimal` is reported.
- TN: `varOne.compareTo(varTwo) == 0` is not reported.
- Edge: classpath-only classes are ignored.

## Complexity and determinism
- Linear in number of call sites (`O(C)`).
- Deterministic by stable class/method/call iteration.

## Annotation policy
- `@Suppress`-style suppression remains unsupported.
- Annotation-driven semantics remain JSpecify-only.
- Non-JSpecify annotations do not affect behavior.

## Risks
- [ ] Some code intentionally requires scale-sensitive equality.
- [ ] Rule cannot infer domain-specific requirements automatically.

## Post-Mortem
- Went well: descriptor-exact matching on `BigDecimal.equals(Object)` made the rule deterministic and easy to validate.
- Tricky: some projects intentionally rely on scale-sensitive equality, so messaging had to clearly recommend `compareTo(...) == 0` only for numeric-equality intent.
- Follow-up: consider future allowlist support if teams need to opt out for known scale-sensitive domains.
