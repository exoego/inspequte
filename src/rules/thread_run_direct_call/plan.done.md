# Rule Plan: thread_run_direct_call

## Summary
Detect direct calls to `Thread.run()` that likely intend asynchronous execution.

## Problem framing
Calling `Thread.run()` executes synchronously on the current thread and does not start a new thread. In most cases the intended API is `Thread.start()`.

## Scope
- Analyze method call sites in analysis target classes only.
- Report calls matching `java/lang/Thread.run()V`.
- Emit one finding per matching call site with class/method context.

## Non-goals
- Do not infer higher-level concurrency design intent.
- Do not add suppression semantics via `@Suppress` / `@SuppressWarnings`.
- Do not add non-JSpecify annotation semantics.

## Detection strategy
1. Iterate analysis target classes, methods, and call sites.
2. Match `owner/name/descriptor` exactly for `Thread.run()V`.
3. Reduce noise by ignoring `super.run()` inside overridden `run()V` methods.
4. Emit deterministic findings in traversal order.

## Rule message
- Problem: direct `Thread.run()` does not start a new thread.
- Fix: call `Thread.start()` when asynchronous execution is intended.

## Test strategy
- TP: direct `new Thread(...).run()` is reported.
- TN: `new Thread(...).start()` is not reported.
- TN: `super.run()` inside `run()V` override is not reported.
- Edge: classpath-only classes are ignored.

## Complexity and determinism
- Linear in number of call sites (`O(C)`).
- Deterministic by stable class/method/call iteration.

## Annotation policy
- `@Suppress`-style suppression remains unsupported.
- Annotation-driven semantics remain JSpecify-only.
- Non-JSpecify annotations do not affect behavior.

## Risks
- [ ] Some projects may intentionally call `run()` synchronously.
- [ ] A static rule cannot always infer intent without context.

## Post-Mortem
- Went well: exact owner/name/descriptor matching gave deterministic detection with minimal runtime cost.
- Tricky: avoiding false positives on `super.run()` required a precise exclusion for `run()V` plus `invokespecial`.
- Follow-up: evaluate whether future concurrency rules should model synchronous execution APIs beyond direct Thread calls.
