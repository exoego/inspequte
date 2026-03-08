# future_wait_while_holding_lock plan

## Goal

Detect blocking wait calls made while the current method still holds either:

- an intrinsic monitor (`synchronized` method or block), or
- a `java.util.concurrent.locks.Lock`-style lock that was acquired and not yet released.

The initial target waits are:

- `java.util.concurrent.Future.get(...)`
- `java.util.concurrent.CompletableFuture.join()`

The rule should produce actionable findings that explain that waiting while holding a lock can deadlock or serialize unrelated work, and should suggest releasing the lock before waiting or restructuring the critical section.

## Scope

In scope:

- same-method detection only
- waits reached while an intrinsic lock is definitely held
- waits reached while a `Lock`-based lock is definitely held in the same method
- method-level `synchronized` and block-level `synchronized`
- common `Lock` acquire/release pairs such as `lock()` / `unlock()`

Out of scope for the first implementation:

- interprocedural reasoning across helper methods
- alias-heavy lock tracking where the held lock instance cannot be identified reliably
- non-blocking future APIs such as `thenApply`, `whenComplete`, or `getNow`
- thread-safety or liveness analysis beyond "wait occurs while lock is held"
- suppression behavior via `@Suppress` or `@SuppressWarnings`
- annotation-driven semantics beyond JSpecify

Annotation policy constraints:

- no `@Suppress` / `@SuppressWarnings` suppression semantics
- only JSpecify is eligible for annotation-driven semantics
- non-JSpecify annotations must not change rule behavior unless a future spec explicitly says otherwise

## Detection Strategy

Model the rule around call sites and definite lock state within a single method body.

1. Identify candidate wait calls:
   - `Future.get()`
   - timed `Future.get(long, TimeUnit)`
   - `CompletableFuture.join()`
2. Track intrinsic lock ownership:
   - mark the full body of `synchronized` methods as monitor-held
   - mark instructions dominated by a `synchronized` block entry and not yet past its exit as monitor-held
3. Track `Lock` ownership:
   - recognize `lock()` calls on `Lock`-like receivers
   - clear held state on matching `unlock()` calls for the same receiver when identity is stable
4. Emit a finding only when the analysis can show the wait executes while at least one lock is still held at that program point.

The rule should prefer precision over recall. If the analysis cannot reliably identify the held lock instance or cannot determine whether the lock has already been released on all paths, it should avoid reporting.

## Precision Notes

- Treat reentrant acquisition as "lock held" without trying to compute recursion depth in the first version.
- Prefer local-variable and direct-field receiver tracking for `Lock` instances.
- Avoid findings for code patterns where `unlock()` is guaranteed before the wait on every path.
- Treat synchronized methods as always holding the receiver or class monitor for the full method body.
- If multiple locks are held, one finding at the wait site is sufficient unless later spec work requires per-lock reporting.

## Determinism And Complexity

- Findings must be emitted in stable method/instruction order.
- Receiver matching must not depend on hash-map iteration order.
- Expected complexity should remain linear in method size plus local control-flow traversal, with no whole-program joins.
- Reuse existing method-level facts where available; do not introduce cross-rule state.

## Test Strategy

Cover at least these cases in the eventual implementation:

- true positive: `synchronized` block wraps `Future.get()`
- true positive: `synchronized` method calls `CompletableFuture.join()`
- true positive: `lock.lock()` followed by `Future.get()` before `unlock()`
- true negative: wait occurs after `unlock()`
- true negative: wait occurs outside the `synchronized` block
- true negative: non-blocking future APIs do not trigger
- edge case: timed `Future.get(long, TimeUnit)` is treated as blocking
- edge case: `try/finally` unlock patterns do not trigger when wait is after the `finally` release point
- edge case: ambiguous lock aliasing is skipped to preserve precision

Use generic test harness names such as `ClassA`, `MethodX`, `varOne`, and `tmpValue`.

## Open Questions

- Whether the first version should recognize `lockInterruptibly()` / `tryLock()` as lock acquisition points.
- Whether read/write locks should be handled only through their returned `Lock` views or also via higher-level APIs.
- Whether the rule should report on waits in `catch` / `finally` blocks inside a synchronized region in the first cut or defer until control-flow coverage is proven robust.

## Risks

- [ ] False positives from imprecise `Lock` receiver alias tracking
- [ ] Missed findings when monitor regions are reconstructed incorrectly from method structure
- [ ] Path-sensitivity gaps around `try/finally` and exceptional edges
- [ ] Over-expanding scope into general deadlock detection instead of same-method lock-held waits
- [ ] Performance regressions if lock-state tracking requires repeated whole-method rescans

## Post-mortem

- Went well: adding `is_synchronized` to method access made synchronized-method coverage straightforward and kept the rule contract-aligned.
- Tricky: full `cargo test` surfaced long-running existing `array_equals` tests, so validation had to rely on targeted rule tests plus a separate snapshot refresh.
- Follow-up: revisit lock-state modeling if support is needed for reentrant acquisition depth or broader `Lock` implementations.
