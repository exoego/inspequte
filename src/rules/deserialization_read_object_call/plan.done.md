# Rule Plan: deserialization_read_object_call

## Summary
Detect direct calls to `ObjectInputStream.readObject()` and `readUnshared()`.

## Problem framing
Java native deserialization is a common security risk when reading untrusted input. Direct `readObject` APIs are high-risk choke points and should be guarded or replaced with safer formats.

## Scope
- Analyze call sites in analysis target classes only.
- Report exact invocations of:
  - `java/io/ObjectInputStream.readObject()Ljava/lang/Object;`
  - `java/io/ObjectInputStream.readUnshared()Ljava/lang/Object;`
- Emit one finding per matching call site with class/method context.

## Non-goals
- Do not infer input trust boundaries from dataflow.
- Do not model custom `ObjectInputFilter` behavior.
- Do not add suppression semantics via `@Suppress` / `@SuppressWarnings`.
- Do not add non-JSpecify annotation semantics.

## Detection strategy
1. Iterate analysis target classes, methods, and call sites.
2. Match owner/name/descriptor exactly for target deserialization APIs.
3. Resolve source line from bytecode offset when available.
4. Emit deterministic findings in traversal order.

## Rule message
- Problem: native deserialization is high risk with untrusted data.
- Fix: use safer serialization formats or strict deserialization controls.

## Test strategy
- TP: `readObject()` is reported.
- TP: `readUnshared()` is reported.
- TN: unrelated stream reads are not reported.
- Edge: classpath-only classes are ignored.

## Complexity and determinism
- Linear in number of call sites (`O(C)`).
- Deterministic by stable class/method/call iteration.

## Annotation policy
- `@Suppress`-style suppression remains unsupported.
- Annotation-driven semantics remain JSpecify-only.
- Non-JSpecify annotations do not affect behavior.

## Risks
- [ ] Some trusted internal protocols may intentionally use Java deserialization.
- [ ] Rule does not distinguish trusted and untrusted sources without additional analysis.

## Post-mortem
- What went well: exact owner/name/descriptor matching made implementation straightforward and deterministic.
- What was tricky: keeping scope limited to analysis targets required an explicit classpath-only negative test.
- Follow-up: consider a future taint/config rule to distinguish trusted internal streams from untrusted inputs.
