## B-A-STAT-COUNTER-INCREMENTS-DEADLOCKED-ON-THEIR-OWN-LOCK (lane A, 2026-08-22) — FIXED 2026-08-22

**In short:** Four statistics counters were incremented with a one-line
statement that takes the same lock twice. A lock that is already held by *you*
is not a lock you can take — the thread waits for itself, forever. So the first
call to `fs::encrypt::encrypt()` hung the machine dead: not a slow boot, a
permanent freeze with the watchdog reporting no forward progress.

**Found by** wiring `fs::encrypt::self_test` into the boot path for the first
time (`B-A-FORTY-ONE-SELF-TESTS-HAD-NEVER-RUN`). Boot cycle 3 printed
`[encrypt]   hmac-sha256: ok` and then nothing at all, followed by three
`[liveness] SYSTEM HANG` reports and an `(OVERDUE - the ISR scan is not
draining)` hrtimer. Serial output stopped at that line and never resumed.

**Mechanism.**

```rust
STATE.lock().files_encrypted = STATE.lock().files_encrypted.saturating_add(1);
```

Rust evaluates the right-hand side of an assignment first, so the RHS
`STATE.lock()` succeeds. But its guard is a *temporary*, and a temporary lives
until the end of the enclosing statement — it has not dropped when the
left-hand side's `STATE.lock()` runs. `STATE` is a non-reentrant spinlock, so
the second acquisition spins with interrupts still enabled and the lock's owner
being the very CPU that is spinning. Nothing can ever release it.

This reads as an ordinary read-modify-write and is easy to write by accident;
what makes it lethal rather than merely wrong is the non-reentrancy, and what
made it invisible is that all four sites were on paths nothing had ever called.

**Sites.** `fs/encrypt.rs:394` (`files_encrypted`) and `:444`
(`files_decrypted`); `fs/fcompress.rs:314` and `:330` (`stats.files_skipped`,
twice in one function — which gets it *right* thirty lines later at :352 with a
`let mut state = STATE.lock();` block, so both spellings sat side by side).

**Fix.** All four now bind the guard to a named local, which acquires exactly
once. The two `fcompress` sites were additionally collapsed into a
`note_skipped()` helper so there is one place left that could get it wrong
rather than two. Both carry a comment explaining the temporary-lifetime rule,
because the broken form is the shorter and more natural-looking one.

**Audit.** The whole of `kernel/src` was swept for the pattern, line-scoped and
across line breaks, for `.lock()`, `.read()`, `.write()`, `.borrow()` and
`.borrow_mut()`. These four were the only same-lock cases. Several multi-lock
statements remain (`fs/filetype.rs:523`, `fs/openwith.rs:339`,
`ipc/namespace.rs:236`, `kshell.rs:2983`) — those hold one guard while taking a
*different* one, which is a lock-ordering question rather than a self-deadlock,
and none of them is on a path that also takes those locks in the reverse order.
Two files (`fs/cas.rs:176`, `fs/integrity.rs:610`) already carry comments
warning about exactly this bug, so it has been hit and fixed here before.

**Severity.** Was a guaranteed total system hang on first use of file
encryption or transparent compression. Both subsystems were unreachable at boot
until this week, which is the only reason it had not been seen.
