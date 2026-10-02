## TD-C-DESIGN-DECISION-818-HAS-NOWHERE-TO-BE-IMPLEMENTED -- FIXED 2026-09-13

**Date:** 2026-09-08. **Lane:** C.
**Where:** `apps/lockscreen/src/main.rs`, `gui/desktop/src/hotkeys.rs`
(`LOCK_COMMAND`), `gui/desktop/src/session.rs`.

**In short:** the operator decided on 2026-09-07 (`design-decisions.md` §818)
that an account with no password is never locked — the lock screen simply does
not appear, instead of appearing and letting anyone dismiss it. That decision
is **not implemented**, and it cannot be implemented where it naturally
belongs, because nothing in the tree actually locks the screen.

**What is true today.** Pressing the lock shortcut produces a *launch request*
for `/usr/bin/lockscreen` and nothing executes it — the shell reports launches
through `HotkeyOutcome::launches` / `ShellSession::take_launches` and has no
connection to a process server. So the screen never appears at all, for any
account. The bug §818 describes is therefore latent rather than live.

**Why it cannot simply be done inside `apps/lockscreen`.** "Never lock" is a
decision made by whatever *locks*, and that program only runs after the
decision. It has no trustworthy way to learn whose session it is locking:
there is no per-process user identity available to an application here, and
the only identity mechanism (`authlib`) verifies a username the caller
supplies. Deciding whether to lock from `$USER` would be worse than the
original bug — an environment variable an attacker can set would decide
whether the machine locks.

**Where it does belong**, once there is a lock path at all: at the point that
requests the lock, which knows the session. The shell already has the
ingredients — `user_accounts::UserAccount` carries `is_current` and
`login_options.has_password` — so the rule is one condition on a launch that
something must first be executing.

**What was done in the meantime.** `LockScreen::unlocks_for`'s doc comment
said the passwordless case was *"an open question rather than a settled one"*
and weighed two options. That has been false since §818, and a comment
claiming a decision is unmade is worse than no comment: it invites the next
reader to re-litigate a settled question, or to "fix" it by refusing the empty
password, which is the option §818 explicitly rejected for stranding the user.
The comment now records the decision, why accepting stays (it is the fallback
for a state §818 says cannot arise, reachable only if a password is removed
while the screen is already up), and why the real fix is elsewhere.

**Order.** This is behind the same prerequisite as
`TD-C-FOUR-SHELL-FEATURES-ARE-BUILT-AND-NEVER-CONSTRUCTED`: the session has no
lock/login lifecycle. Build that, and §818 is one condition. Do not implement
§818 by making the lock screen refuse empty passwords — that is option B,
which the operator considered and rejected.

**FIXED 2026-09-13, and the prerequisite was met the same day.** This
entry's whole argument was that nothing executes the launch, so there was
no lock path to put a condition on. `gui/desktop`'s binary now drains
`take_launches` and spawns, so the bug stopped being latent and became
live the moment the shell got a process — the trigger this entry was
waiting for, arriving from a direction it did not anticipate.

**Implemented exactly where this entry said it belonged**: at the point
that requests the lock, which knows the session. `ShellSession` records
`lockable` at login — `authlib::Outcome::NoPassword` is already
distinguished from `Accepted` there, so the fact was in hand and only
needed keeping — and `queue_launches` drops a launch of `LOCK_COMMAND`
when it is false.

Three details worth having:

- **Dropped where it is recorded, not where it is drained.** A lock
  sitting in the queue until something drains it is a lock this session
  has, however briefly, agreed to, and `launches` is readable in between.
- **`lockable` defaults to true.** A session that never showed a login
  screen has not learned that the account is passwordless, and refusing to
  lock on a guess is §818's own failure pointing the other way.
- **`LOCK_COMMAND` became `pub(crate)`** rather than being spelled twice.
  Two copies of that string is how the shortcut and the refusal drift.

**Not option B.** The lock screen still accepts an empty password, which
is the fallback for a state §818 says cannot arise — reachable only if
a password is removed while the screen is already up.

**Two tests, through the compositor rather than the shell.**
`press_lock_shortcut` sends Super+L as an `InputEvent` to the panel
surface and pumps, because the *session* is what decides whether a launch
is queued; a test calling `shell_mut().handle_hotkey` would exercise the
half that was never in question. The passwordless session queues nothing;
the one with a password queues `/usr/bin/lockscreen` exactly as before,
which is the control that stops a `queue_launches` dropping everything
from passing. Proved able to fail: removing the condition makes the first
report *818: the lock screen must not even be asked for*.
