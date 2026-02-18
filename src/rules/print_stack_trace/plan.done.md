# Rule Plan: print_stack_trace

## Summary
Detect direct calls to `Throwable.printStackTrace(...)`.

## Problem framing
`printStackTrace` writes diagnostics directly to standard error or arbitrary streams/writers, which bypasses structured logging and makes production incident analysis harder.

## Scope
- Analyze call sites in analysis target classes only.
- Report exact invocations of:
  - `java/lang/Throwable.printStackTrace()V`
  - `java/lang/Throwable.printStackTrace(Ljava/io/PrintStream;)V`
  - `java/lang/Throwable.printStackTrace(Ljava/io/PrintWriter;)V`
- Emit one finding per matching call site with class/method context.

## Non-goals
- Do not infer the best logging framework for the project.
- Do not inspect whether downstream wrappers eventually call `printStackTrace`.
- Do not add suppression semantics via `@Suppress` / `@SuppressWarnings`.
- Do not add non-JSpecify annotation semantics.

## Detection strategy
1. Iterate analysis target classes, then methods, then call sites.
2. Match owner/name/descriptor exactly for `Throwable.printStackTrace` overloads.
3. Resolve source line from bytecode offset when available.
4. Emit deterministic findings in traversal order.

## Rule message
- Problem: unstructured exception reporting via `printStackTrace`.
- Fix: log the exception through structured logging with context.

## Test strategy
- TP: `varOne.printStackTrace()` is reported.
- TP: `varOne.printStackTrace(PrintWriter)` is reported.
- TN: unrelated stack-trace APIs (for example `Thread.getStackTrace()`) are not reported.
- Edge: classpath-only classes using `printStackTrace` are ignored.

## Complexity and determinism
- Linear in number of call sites (`O(C)`).
- No CFG/dataflow required.
- Deterministic by stable class/method/call iteration.

## Annotation policy
- `@Suppress`-style suppression remains unsupported.
- Annotation-driven semantics remain JSpecify-only.
- Non-JSpecify annotations do not affect behavior.

## Risks
- [ ] Potential noise in small CLI tools where stderr output is intentional.
- [ ] Missed detections when helper methods wrap `printStackTrace`.
- [ ] Message should remain actionable without mandating a specific logger implementation.

## Post-Mortem
- Went well: callsite matching for `printStackTrace` overloads produced deterministic behavior with low implementation complexity.
- Tricky: owner matching needed to account for compile-time receiver types like `java/lang/Exception`, not only `java/lang/Throwable`.
- Follow-up: evaluate whether to narrow owner matching with class-hierarchy checks if false positives appear for custom `printStackTrace` methods.
