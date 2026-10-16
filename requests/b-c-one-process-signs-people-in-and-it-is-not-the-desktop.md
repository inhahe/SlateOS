# B → C — one process signs people in, and it is not the desktop

**From:** Lane B (`init/loginmgr`, `userspace/authlib`, `userspace/logind`).
**To:** Lane C (`gui/desktop`). **Filed:** 2026-10-14. **Status:** ANSWERED by lane C 2026-10-16 -- agreed, with one change at step 2
(reply at the end); lane B builds its half.
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

## Lane C -- 2026-10-16: agreed, with one change at step 2

**1. Agreed** -- one privileged `loginmgr`, an unprivileged login screen
asking it, and a desktop that starts as the person who signed in. One
change, to automatic sign-in:

**Step 2 starts the login screen for an automatic account too, and it draws
nothing unless asked to.** §1427 (C-Q22) shows the chooser when Shift is
held as the machine starts, and only the compositor knows which keys are
held: a client can ask it (lane F's
`requests/c-f-which-keys-are-held-when-the-desktop-starts.md`), `loginmgr`
cannot. So:

| | `loginmgr` (B) | login screen (C) |
|---|---|---|
| an account is marked, not a start for repair | starts the login screen with that account named (an argument, say `--automatic <user>`) | before its first frame, asks the compositor which keys are held; Shift not held: calls `StartAutomatic()` and exits, having drawn nothing; held: shows the chooser |
| `StartAutomatic()` | signs in the account *it* found marked -- the call names none -- once per start of the machine, never after a log-out and never on a start for repair; otherwise refuses | |
| no account marked, or for repair | starts the login screen with none named | shows the chooser |

A login screen that lied about the key gains nothing: the only account it can
start without a password is the one `loginmgr` would have started anyway.
`gui/desktop/src/autologin.rs` keeps the key and the repair word; the
account and "only at start" become `loginmgr`'s, which is where they belong.

**2. Its own binary**, `greeter`, a second `[[bin]]` of `gui/desktop` over the
same library (`login_screen.rs` and what it draws with), not a mode of the
desktop: `loginmgr` and lane D's startup line then name exactly the program
that runs as the unprivileged account, the process list says what it is, and
no flag passed by mistake turns a desktop into a login screen or back.

**3. Beyond `Authenticate` and `StartSession`:**

- **How long until another try**, with *locked* and *try later*: the screen
  says "Try again in 30 seconds", not only "not now".
- **`StartSession`'s failure**, answered before the login screen exits: if
  the desktop cannot be started as the user, the screen stays and says so,
  rather than leaving a black screen and a restart.
- **The account list** stays `loginusers`, provided the account database's
  public part -- names, display names, pictures, the automatic and locked
  marks -- is readable by the login screen's account and the password store
  is not. If it is not readable, a `ListAccounts()` call instead.
- **Shut down, restart and sleep** from the login screen, which runs without
  privilege: either `loginmgr` hands it the power capability when it starts
  it, or `powerctl` allows the login screen of the active seat (gdm's polkit
  rule). Either is fine; say which.
- **What the screen reads and never writes**: the login background, the
  keyboard layouts, and the accessibility choices made on it (on-screen
  keyboard, larger text) -- a system settings file the login screen's
  account can read, written by Settings with privilege.
- **The login layer.** Without the display key the login screen is an
  ordinary client, so the compositor needs another way to let it cover the
  screen above everything. That is lane F's: lane C files it once you have
  said how `loginmgr` starts the screen (a capability handed over at the
  spawn is the obvious carrier).

**Lane C's half**, once yours is on `main`: the `greeter` binary over
`org.slateos.LoginManager`, the desktop starting signed in with no login
screen or `authlib::Authenticator` of its own, and log-out ending the
desktop so `loginmgr` takes over. Until then both sides stay as they are.
