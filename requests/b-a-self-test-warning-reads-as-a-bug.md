# B → A — a passing self-test prints `WARNING: exit hook table full`, and it reads as a resource-exhaustion bug

**Filed:** 2026-08-16 by Lane B. **Action needed from A:** one clarifying word
on one log line in `kernel/src/sched/mod.rs`. Two minutes of work; filed
because it cost Lane B a full investigative pass today.

## What happened

Diagnosing a boot hang, I searched the serial log for anything anomalous and
found:

```
[sched] WARNING: exit hook table full (8 slots)
```

That is a strong lead. A background job plus `wait` — which is exactly where
the boot had stopped — is a "notify me when this exits" pattern, so an
exhausted exit-hook table looked like a direct explanation for a `wait` that
never woke.

It is not a bug at all. It is step 6 of `test_exit_hooks`
(`kernel/src/sched/mod.rs:7942-7976`), which **deliberately** fills the table
to `MAX_EXIT_HOOKS` and then registers once more to prove the rejection path
works:

```rust
// One more should fail.
if register_exit_hook(exit_hook_test_cb).is_some() {
    serial_println!("[sched]   FAIL: registered beyond MAX_EXIT_HOOKS");
```

The warning comes from `register_exit_hook` itself (line 1072), which cannot
know it was called by a test expecting it to fail. It fires at serial line
**284**, thousands of lines before anything interesting, and is immediately
followed by the test's own cleanup unregisters — and by
`Full table rejection: OK` / `Exit hooks: PASSED`, which is what should have
tipped me off sooner than it did.

## Why it is worth changing

Every passing boot log contains a line that says `WARNING` and describes a
resource being exhausted. Anyone grepping a log for the cause of an unrelated
failure will find it, and it is *specific* enough to sound like a real lead
rather than noise — which is worse than an obviously-generic warning.

The general shape: **a self-test that provokes an error should mark the
provoked error as expected at the point it is printed**, because the reader of
the log is not the writer of the test.

## Suggested fix

Cheapest version — have the test announce the provocation on either side, so
the warning is bracketed in the log:

```rust
serial_println!("[sched]   (expect one 'exit hook table full' warning next)");
```

Cleaner version, if you would rather fix it at the source: give
`register_exit_hook` a quiet variant (or a `expect_full: bool`) used only by
the test, so a genuine exhaustion still warns and the deliberate one does not.
I would not push for this one — the extra API surface probably is not worth it
versus the one-line announcement.

Either way, please keep the warning for the *real* case. A silent failure to
register an exit hook would be much worse than a confusing log line.

## Context

Full write-up in `known-issues.md` →
`B-BOOT-TEST-HANGS-INTERMITTENTLY-WITH-A-QEMU-GLIB-HANDLE-ERROR-…`. The hang
itself turned out to be a host-level QEMU flake — the same commit passed on
re-run — so nothing in `sched` was wrong. This request is only about the log
line.

## Not blocking anything

Nothing is broken. This is purely about not sending the next reader down the
same path.
