# Rule Plan: bigdecimal_from_double

## Summary
Detect `new BigDecimal(double)` constructor calls because binary floating-point input can produce unexpected decimal values.

## Problem Framing
Constructing `BigDecimal` directly from `double` encodes floating-point approximation artifacts (for example `0.1`). The resulting value often differs from developer intent and can break money/precision-sensitive logic.

## Scope
- Report constructor calls to `java/math/BigDecimal.<init>(D)V`.
- Report constructor calls to `java/math/BigDecimal.<init>(DLjava/math/MathContext;)V`.
- Produce findings on analysis-target classes only.

## Non-Goals
- Do not report `BigDecimal.valueOf(double)`.
- Do not report string-based constructors like `new BigDecimal("0.1")`.
- Do not support suppression via `@Suppress` or `@SuppressWarnings`.
- Do not apply non-JSpecify annotation semantics.

## Detection Strategy
- Scan extracted method call sites in each method.
- Match owner `java/math/BigDecimal`, name `<init>`, and one of the targeted descriptors.
- Emit one finding per matched call site with method location and source line (if available).
- Preserve deterministic output by iterating classes/methods/calls in stable order.

## Rule Message
- Problem: "BigDecimal constructed from double can lose precision."
- Fix: "Use BigDecimal.valueOf(double) or a decimal string constructor."

## Test Strategy
- TP: `new BigDecimal(0.1d)` is reported.
- TN: `BigDecimal.valueOf(0.1d)` is not reported.
- TN: `new BigDecimal("0.1")` is not reported.
- Edge: `new BigDecimal(varOne, MathContext.DECIMAL64)` is reported.

## Complexity and Determinism
- Complexity is O(total call sites) across analyzed methods.
- No hash-order dependent iteration.
- Findings are emitted in encountered bytecode order.

## Annotation Policy
- `@Suppress`-style suppression is unsupported.
- Annotation-driven semantics support JSpecify only.
- Non-JSpecify annotations do not affect behavior.

## Risks
- [ ] False positives when precision loss is intentional in legacy code.
- [ ] Missed variants if future JDK adds additional double-accepting constructors.
- [ ] Message clarity must remain actionable for non-financial domains.

## Post-Mortem
- Went well: constructor-descriptor matching made the rule precise without additional control-flow complexity.
- Tricky: verify-input generation did not include untracked new files by default, so the verification bundle had to be completed explicitly.
- Follow-up: consider enhancing `scripts/prepare-verify-input.sh` to include untracked files automatically.
