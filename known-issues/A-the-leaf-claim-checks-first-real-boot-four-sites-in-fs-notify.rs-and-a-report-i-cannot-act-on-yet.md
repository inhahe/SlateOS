### [A] The leaf-claim check's first real boot: four sites in `fs/notify.rs`, and a report I cannot act on yet -- 2026-09-17

**Status:** OPEN

It fired 8 times (its cap) across **four distinct sites**, all in one file:
`fs/notify.rs` lines 330, 394, 440, 481.

The inner lock is identifiable from the site alone: `notify.rs:46` is `use
crate::sync::Mutex`, so those are acquisitions of `NOTIFY_WAITERS` /
`WATCHES`, both tracked `Mutex`es. Something holding a `PreemptSpinMutex` is
therefore calling into `notify` and taking a tracked lock inside that
critical section. Four sites in one file suggests `notify` is routinely
reached from inside other subsystems' critical sections, which is exactly the
class dd-70's premise forbids and which no existing detector could see:
lockdep cannot see the outer type at all, and `fail_if_recursive` compares a
lock against itself.

**Not yet actionable, because I rebuilt this morning's defect.** Every report
reads:

```
"?" acquired while "?" is held, at kernel/src/fs/notify.rs:330:10
```

I carried the outer lock's *name* specifically so the report would not be
one-sided -- and the name is the half that cannot identify anything, because
`PreemptSpinMutex::new` defaults it to `b"?"` and 563 locks in this kernel
answer to that. The entry two above records exactly this about lockdep's
reports, hours earlier, and I built it straight into the replacement. What
identifies a lock is where it was acquired.

`leaf_enter` now also stores `Location::caller()` -- one pointer per CPU, on
the 0 -> 1 transition only, which is what the `#[track_caller]` added to the
`PreemptSpinMutex` paths was for in the first place. The name is kept
alongside it, because a *named* lock is the more readable half; it just
cannot be the only half. Whether these four are real is unknown until the
next boot names the outer lock -- and on this session's record (five findings,
five false) they should not be assumed real.
