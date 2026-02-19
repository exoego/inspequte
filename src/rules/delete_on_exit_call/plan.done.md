# Rule Plan: delete_on_exit_call

## Summary
Detect direct calls to `java.io.File.deleteOnExit()`.

## Problem framing
`File.deleteOnExit()` registers paths in a global shutdown hook list that is never pruned during process lifetime. In long-running services this can accumulate memory and delay shutdown.

## Scope
- Analyze call sites in analysis target classes only.
- Report exact invocations of `java/io/File.deleteOnExit()V`.
- Emit one finding per matching call site with class/method context.

## Non-goals
- Do not infer process lifetime expectations.
- Do not add suppression semantics via `@Suppress` / `@SuppressWarnings`.
- Do not add non-JSpecify annotation semantics.

## Detection strategy
1. Iterate analysis target classes, methods, and call sites.
2. Match owner/name/descriptor exactly for `File.deleteOnExit()V`.
3. Resolve source line from bytecode offset when available.
4. Emit deterministic findings in traversal order.

## Rule message
- Problem: delete-on-exit registrations can accumulate in long-running processes.
- Fix: perform explicit deletion and handle failures directly.

## Test strategy
- TP: `varOne.deleteOnExit()` is reported.
- TN: `varOne.delete()` is not reported.
- Edge: classpath-only classes are ignored.

## Complexity and determinism
- Linear in number of call sites (`O(C)`).
- Deterministic by stable class/method/call iteration.

## Annotation policy
- `@Suppress`-style suppression remains unsupported.
- Annotation-driven semantics remain JSpecify-only.
- Non-JSpecify annotations do not affect behavior.

## Risks
- [ ] Some CLI tools intentionally rely on delete-on-exit semantics.
- [ ] Rule cannot infer whether the process is short-lived.

## Post-Mortem
- Went well: exact owner/name/descriptor matching kept the implementation deterministic and easy to review.
- Tricky: the message needed to balance risk explanation with the fact that short-lived tools may intentionally use delete-on-exit.
- Follow-up: consider adding optional policy profiles for CLI-focused projects if this proves noisy.
