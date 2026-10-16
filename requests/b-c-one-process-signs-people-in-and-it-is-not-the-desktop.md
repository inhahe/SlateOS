# B → C — one process signs people in, and it is not the desktop

**From:** Lane B (`init/loginmgr`, `userspace/authlib`, `userspace/logind`).
**To:** Lane C (`gui/desktop`). **Filed:** 2026-10-14. **Status:** OPEN -- a
proposal; lane B builds its half as soon as lane C agrees or counters.
**Why now:** design-decisions §1392 (the operator's answer to F-Q15) makes
"the login screen, then the desktop shell, start" the next step for lanes B
and C, ahead of both lanes' own lists.

## In short

Two programs each act as the thing that signs a person in, and neither does
all of it. Lane C's desktop draws a login screen, checks the password itself
through `authlib`, and on success hides the screen (`Session::admit`) -- the
desktop goes on running as whatever account started it, so the person who
signed in never *becomes* that user: their programs run as the desktop's
account, nothing tells `logind` a session began, and the desktop must be
allowed to read the password store to check a password at all. Lane B's
`init/loginmgr` was written as a display manager but starts nothing.

The proposal is the shape every Linux display manager has (gdm, lightdm,
sddm): **one small privileged program signs people in and starts their
desktop as them; the login screen and the desktop run without privilege.**

## The proposal

| Step | Who | What happens |
|---|---|---|
| 1 | init (lane D's `/etc/startup.conf`) | starts the compositor, then `loginmgr` |
| 2 | `loginmgr` (B) | starts the login screen as an unprivileged account, with no display key |
| 3 | login screen (C) | asks `loginmgr` over a service channel: *is this the password?* -- and gets *accepted*, *rejected*, *locked* or *try later*, never a hash (§341's shape A) |
| 4 | `loginmgr` (B) | on *accepted*: tells `logind` a session began, then starts the user's desktop **as the user** (`authlib::identity`: groups, gid, uid, in that order), with `HOME`, `USER`, `SHELL` and the session's variables set, and the display key (`f-bd` point 2) on that one spawn; the login screen exits |
| 5 | desktop (C) | starts already signed in, with no login screen of its own; locking keeps using `logind`'s `AuthenticateSession`, as now |
| 6 | `loginmgr` (B) | when the desktop exits (log out), ends the `logind` session and starts the login screen again |

An account marked to sign in by itself (`gui/desktop/src/autologin.rs`
decides it today) is decided by `loginmgr` at step 2, which reads the account database: the login
screen is not started, and step 4 runs for that account.

## What changes for lane C

- `gui/desktop` gains a **greeter mode** -- only `login_screen.rs`, talking to
  `org.slateos.LoginManager` (`Authenticate(user, password)` →
  outcome; `StartSession(user)` once accepted) instead of `authlib`. Whether it
  is `desktop --greeter` or a separate binary is yours.
- The desktop proper drops its own login screen and its `authlib::Authenticator`:
  it starts signed in, and the account it runs as is the person using it.
- `apps/lockscreen` is unchanged.

## What lane B builds

- `loginmgr` as the daemon: the service `org.slateos.LoginManager`, the two
  calls above with `authlib`'s rate limit and timing behind them, the session
  start (identity drop, environment, display key, `logind`'s
  `CreateSession`), and the restart at log out.
- Its line for lane D's `/etc/startup.conf`, holding the display key so it can
  hand it on (lane A's exact-id rule).

## Questions

1. Agreed, or a counter-proposal?
2. Greeter as a mode of `gui/desktop`, or its own binary?
3. Anything the login screen needs from `loginmgr` beyond the two calls -- the
   account list (it reads `loginusers` today, which reads the database too),
   the power menu (`powerctl`)?

## What is unchanged until then

Nothing changes on either side until this is settled: the desktop keeps its
own login screen, and `loginmgr` starts nothing.
