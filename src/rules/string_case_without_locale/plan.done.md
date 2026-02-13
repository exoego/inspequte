# Rule Plan: string_case_without_locale

## Summary
Detect locale-sensitive case conversion calls on `java.lang.String` where no explicit `Locale` is provided.

## Problem framing
Calling `String.toLowerCase()` or `String.toUpperCase()` without a `Locale` uses the JVM default locale. Behavior can differ across environments (for example, Turkish locale), producing subtle production-only bugs.

## Scope
- Analyze method calls in application classes.
- Report calls to:
  - `java/lang/String.toLowerCase()Ljava/lang/String;`
  - `java/lang/String.toUpperCase()Ljava/lang/String;`
- Produce one finding per matching call site with method context.

## Non-goals
- Do not report locale-aware overloads that already pass `java.util.Locale`.
- Do not infer intended locale from surrounding code or configuration.
- Do not add suppression semantics via `@Suppress` / `@SuppressWarnings`.
- Do not add non-JSpecify annotation semantics.

## Detection strategy
- Iterate class methods and their call sites.
- Match owner/name/descriptor exactly for the two locale-less overloads.
- Use call offset to resolve source line when available.
- Keep output order deterministic by preserving class/method/call traversal order.

## Rule message
- Problem: locale-sensitive String case conversion depends on default locale.
- Fix: pass explicit locale (`Locale.ROOT` in most cases).

## Test strategy
- TP: `toLowerCase()` and `toUpperCase()` without locale are reported.
- TN: `toLowerCase(Locale.ROOT)` and `toUpperCase(Locale.ROOT)` are not reported.
- Edge: mixed use in one method reports only locale-less calls.

## Complexity and determinism
- Expected linear complexity in number of call sites.
- No dataflow/CFG traversal needed.
- Deterministic output from stable iteration and no hash-based ordering.

## Annotation policy
- `@Suppress`-style suppression remains unsupported.
- Annotation-driven semantics remain JSpecify-only.
- Non-JSpecify annotations do not affect behavior.

## Risks
- [ ] Potential noise in intentionally locale-dependent code paths; message should suggest explicit locale choice rather than forcing `Locale.ROOT` universally.
- [ ] Missed detections if compilers/desugarers rewrite calls to helper methods; accept as out of scope for initial version.
- [ ] Message wording must stay actionable and avoid implying all locale-less usage is always wrong.

## Post-Mortem
- Went well: exact owner/name/descriptor matching made the implementation simple and deterministic while fully covering the spec acceptance criteria.
- Tricky: isolated verify input generation initially missed untracked new files, so verify had to be regenerated with rule files included in `diff.patch`.
- Follow-up: evaluate whether we should add configurable guidance text for teams that prefer a locale other than `Locale.ROOT` by default.
