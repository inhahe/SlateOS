## 1076. `loginmgr` signs people in and starts their desktop as them; the login screen asks it

**Date:** 2026-10-16
**Lane:** B
**Decided by:** Claude (autonomous), on the shape agreed with lane C in
`requests/b-c-one-process-signs-people-in-and-it-is-not-the-desktop.md`
(lane B proposed it 2026-10-14; lane C agreed 2026-10-16, changing how the
automatic account is started).

**In short:** until now the desktop drew its own login screen, checked the
password itself and then simply hid the screen, so whoever signed in never
*became* that user: their programs ran as the desktop's account, and logind
never heard a session begin. Now one small privileged program, `loginmgr`,
does the signing in. It starts the login screen as an account with no
privilege; the screen asks it whether a password is right and gets a yes or
no; on a yes `loginmgr` tells logind and starts the person's desktop *as
them*. Log out, and it starts the screen again. It is how gdm, lightdm and
sddm work.

### What it is

`init/loginmgr` was a 3,255-line model of a login screen -- accounts, a
password check, a lock screen, a power menu, drawing -- that started nothing.
Its drawing is lane C's now (their `greeter`, over `login_screen.rs`), and
what is left is a daemon with no window:

| Part | |
|---|---|
| `protocol` | the service `org.slateos.LoginManager`: `ListAccounts`, `Authenticate(user, password)`, `StartSession(user)`, `StartAutomatic()`; their encoding; the client the screen uses |
| `manager` | every rule below, with no system call in it, tested on the host |
| `system` | the account database, logind over the bus, the launcher |

### The rules, and why each

- **Only the screen `loginmgr` started may call, and root.** The kernel names
  the caller's pid (`SYS_CHANNEL_PEER_CRED`); a caller it cannot name is
  refused, as logind refuses one (`userspace/logind/src/bus.rs`).
  Otherwise any program could call `StartAutomatic`, or guess passwords here
  besides at the console.
- **A session needs a proof from the same screen process, for the same
  account, under a minute old.** `StartSession` follows an acceptance at
  once; a proof left lying is not one the person at the screen gave now.
  It is spent by the start and dropped by any other `Authenticate`.
- **An account with no password signs in without one.** `authlib` answers
  `NoPassword` and leaves the policy to the caller (§341): a lock screen
  must refuse, a console `login` admits. A login screen offering an account
  that has no password is how that account was meant to be used, so it
  admits, as `login` does.
- **The automatic account starts once.** `loginmgr` decides it before the
  first screen (exactly one account marked and not locked, as `loginusers`
  says; not on a start for repair, `recovery` or `single` on the kernel's
  command line; not if anyone has signed in since the machine started,
  `/var/run/loginmgr/signed-in`). The screen is told its name and, unless
  Shift is held -- which only the compositor can see, so only a compositor
  client can ask (§1427) -- calls `StartAutomatic()`, which names no account
  and starts only that one.
- **The session is registered before the desktop starts, its leader named
  after.** The desktop must be told its session id (`XDG_SESSION_ID`: the
  lock screen's `AuthenticateSession` needs it), and the id exists only once
  logind has registered the session; the desktop's pid exists only once it
  has started. So `CreateSession` takes leader 0 and logind's new root-only
  `SetSessionLeader` names the desktop afterwards. (systemd has no such call
  because there the leader is the process that registers, which then becomes
  the session; `loginmgr` is one process for every session.)

### What it waits on

Starting a program as another user *holding only what it is given*. A
`fork`, `setuid`, `exec` child keeps its parent's whole capability table,
and giving up root does not take `Service` rights away, so the screen and
the desktop would both hold `loginmgr`'s `Service/0/w` (register any service
name) and the display key. `SYS_PROCESS_SPAWN_EX2` hands on exactly the
capabilities named, but runs the child as the caller. Asked of lane A in
`requests/b-a-start-a-program-as-a-user-holding-only-what-it-is-given.md`,
with init's holding the display key to hand on. Until then the launcher
starts nothing and says why, rather than hand `loginmgr`'s capabilities to
both programs.

### Alternatives

| | Why not |
|---|---|
| The desktop signs people in itself (as it did) | It never becomes the user, and must be able to read the password store. |
| A per-session helper that registers with logind and becomes the session, as gdm's worker does | One more privileged process per session, running as the user while holding `loginmgr`'s capabilities until it starts the desktop -- the very thing the launcher must avoid. |
| `fork` and `setuid` now, the kernel call later | Hands `Service/0/w` and the display key to the login screen. Nothing is lost by waiting: nothing starts `loginmgr` yet. |
