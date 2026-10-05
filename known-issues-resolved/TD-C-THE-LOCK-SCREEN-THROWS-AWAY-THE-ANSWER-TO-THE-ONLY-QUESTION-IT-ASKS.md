## TD-C-THE-LOCK-SCREEN-THROWS-AWAY-THE-ANSWER-TO-THE-ONLY-QUESTION-IT-ASKS — RESOLVED 2026-08-24

**Lane C, found 2026-08-24 while planning the `authlib` rework.**

**RESOLVED 2026-08-24.** Fixed exactly as the plan at the bottom of this
entry describes, and the plan is left in place because it is the record of
what was done. `submit_password` returns `AuthOutcome`; `unlock_requested`
is a real field collected by `take_unlock_request()`; `PasswordAuthority`
is the trait the verdict arrives through and `PasswordValidator` is now
one interim implementor of it rather than the only way to get an answer.
Eight tests drive `handle_event` and the mouse path instead of calling
`submit_password` directly, including one that presses Enter with the
right password and asserts an unlock was authorised — which is the
assertion whose absence let this ship.

Two things deliberately *not* done here, both recorded rather than
silently decided:

- **The `NoPassword` policy is unchanged**, so a passwordless account's
  screen still opens on Enter as it always has. Refusing is the secure
  answer and also locks the real user out forever; it is now one function
  (`LockScreen::unlocks_for`) and one question in `open-questions.md`.
  Changing who can unlock a machine is not something to slip into a
  commit about interface shape.
- **`PasswordValidator` is still here**, because `logind` answers
  `system.logind.Error.UnknownCaller` to everyone until lane A can tell a
  service who is calling it
  (`requests/b-a-a-service-cannot-find-out-who-is-calling-it.md`). Its
  `PasswordAuthority` impl carries the note; when that lands the impl is
  deleted and a bus connection is constructed in its place, and nothing
  else in the file moves.

`apps/lockscreen/src/main.rs` — `LockScreen::submit_password` returns `bool`,
and **both of its call sites discard it**:

```rust
Key::Enter => {
    let _ = self.submit_password();      // :1025
    EventResult::Consumed
}
// and on the submit button, :1064
```

So typing the correct password and pressing Enter runs the key derivation,
clears the buffer, resets the failure count — and reports nothing to anybody.
There is no path by which the screen can unlock.

**What makes this worse than a missing line is the comment above it.**
`handle_event`'s doc says:

> Special return: if the screen should unlock, this is signaled by the
> `unlock_requested` flag on the struct (checked separately).

There is no `unlock_requested` field. `grep -rn unlock_requested apps/` matches
that sentence and nothing else. The comment does not describe a mechanism that
broke; it describes one that was never written, and it is the only
documentation the first caller of `handle_event` will read. That caller will
look for the flag, not find it, and have to reverse-engineer the fact that the
verdict is unreachable — which is the `days_in_month` failure shape again: a
second answer justified by a false statement about the first.

### Why the tests are all green

Every test calls `submit_password()` **directly** and asserts on its return
(`assert!(ls.submit_password())`, `:1887`). Not one of them goes in through
`handle_event`, which is the only path a user has. The unit under test is
correct; the wiring between it and the world is absent, and nothing looks at
the wiring.

### Why it has not bitten yet

`apps/lockscreen` has `fn main() {}` — it is a library wearing a binary's
clothes, with no compositor session driving it. So this is not a live
"correct password rejected" bug today; it is a trap set for whoever writes the
first driver. That is exactly when it is cheapest to fix.

### The proper fix — and it lands with the `authlib` rework, not before

The return type is changing anyway. `requests/b-c-desktop-password-checks-go-through-a-privileged-verifier.md`
asks for the verdict to arrive from *outside* the screen as a four-way outcome
rather than a `bool` computed in-process, so both changes touch the same
signature and splitting them would mean editing every call site twice:

- `AuthOutcome` mirroring `authlib::Outcome` and logind's `OUTCOME_*` wire
  codes — `Accepted` / `Rejected` / `Locked` / `NoPassword` / `Unusable` /
  `RateLimited { retry_after_secs }`. Lane B's point that this is not a `bool`
  is the same point as this entry: `Unusable` means the *system* is broken and
  must reach an administrator rather than be counted as a typo.
- A `PasswordAuthority` trait with `authenticate(&mut self, username, password)
  -> AuthOutcome`. `PasswordValidator` implements it as the interim local path
  and is deleted when logind can identify its callers (blocked on
  `requests/b-a-a-service-cannot-find-out-who-is-calling-it.md`, which is lane
  A's — until it lands, a real bus call gets a refusal rather than a verdict).
- `submit_password() -> AuthOutcome`, and a real `unlock_requested` flag with a
  `take_unlock_request()` accessor, so the comment above becomes true by
  construction rather than aspirationally.
- **A test that drives `handle_event` rather than `submit_password`** — typing
  the password a character at a time and pressing Enter. Without it the new
  wiring is exactly as untested as the old, and the whole entry recurs.

---
