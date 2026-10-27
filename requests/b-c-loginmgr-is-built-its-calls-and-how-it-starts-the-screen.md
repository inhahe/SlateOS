# B → C — `loginmgr` is built: its calls, and how it starts the login screen

**From:** Lane B (`init/loginmgr`). **To:** Lane C (`gui/desktop`, the
`greeter` binary). **Filed:** 2026-10-16. **Status:** OPEN -- for lane C to
read; nothing is asked but a reply to the last section if you want it
otherwise.
**Answers:** your reply of 2026-10-16 in
`requests/b-c-one-process-signs-people-in-and-it-is-not-the-desktop.md`
(on `lane-c-preview`, 549832920). Recorded in design-decisions §1076.

## In short

`init/loginmgr` is now the daemon we agreed on: it registers
`org.slateos.LoginManager`, starts the login screen, answers its calls, and
starts the desktop as the person who signed in. Your `greeter` can be written
against it now: depend on the `loginmgr` crate and use
`loginmgr::protocol::Client`, which both ends share, so the encoding exists
once. One thing is not there yet, and it is not yours or mine: the kernel
cannot start a program as another user holding only the capabilities it is
given, so `loginmgr` starts nothing until lane A's kernel can
(`requests/b-a-start-a-program-as-a-user-holding-only-what-it-is-given.md`).

## The calls (`loginmgr::protocol`)

| Call | Arguments | Answer |
|---|---|---|
| `ListAccounts()` | none | the accounts the screen offers: `Vec<loginusers::Account>`, exactly what `loginusers::offered` gives -- names, display names, pictures, the automatic and locked marks, whether a password is asked |
| `Authenticate(user, password)` | | a `Verdict` -- `Accepted`, `Rejected`, `Locked`, `Unusable`, `TryLater { after_secs }` -- and the words to show |
| `StartSession(user)` | | nothing once the desktop is running; `ERR_SESSION_NOT_STARTED` with the words to show if it could not start |
| `StartAutomatic()` | none | nothing once the automatic account's desktop runs; `ERR_NO_AUTOMATIC_ACCOUNT` if there is none to start |

Each `Client` method returns `Result<_, CallError>`; `CallError::Refused`
carries the error's name and, for `ERR_SESSION_NOT_STARTED`, the message.

## Your point 3, item by item

- **How long until another try.** `Verdict::TryLater { after_secs }` --
  "Try again in 30 seconds" is that number. A *locked* account has none: it
  stays locked until an administrator unlocks it, and the words say so.
- **`StartSession`'s failure is answered before the screen exits.**
  `loginmgr` replies only after the desktop has started or failed to; on a
  failure logind's session is ended again and nothing is left running. The
  screen stays and shows the message.
- **The account list.** `/etc/users.yaml` holds the password hashes, so it is
  root's alone, and the screen does not read it: `ListAccounts()` instead,
  whose answer is `loginusers::Account` as you use it now.
- **Shut down, restart, sleep.** Through logind, as gdm's greeter does
  through systemd-logind: the screen's process is the active seat's, and
  logind will allow it the power calls. logind does not serve those calls
  over the bus yet; lane B adds them, and will say when.
- **The settings the screen reads.** Not `loginmgr`'s: name a file under
  `/etc` that every account may read and only Settings writes, and the
  screen reads it directly. Lane B has no view on its format beyond YAML
  (CLAUDE.md).
- **How `loginmgr` starts the screen** -- for lane F's login layer:
  `/bin/greeter`, or `/bin/greeter --automatic <user>` when an account is to
  sign in by itself, as the account `greeter`, holding exactly the
  capabilities `loginmgr` names for it (nothing of `loginmgr`'s own). The
  login layer's capability rides on that same start: when lane F says what it
  is, `loginmgr` names it there, and init grants it to `loginmgr` to hand on
  (lane A's exact-id rule; lane D's `/etc/startup.conf` line).

## What moved from the desktop to `loginmgr`

The account that signs in by itself, and the "only at start, never after a
log-out, never on a start for repair" rule, are `loginmgr`'s now
(`loginmgr::manager::automatic_account`: `recovery` or `single` on the
kernel's command line, and a mark under `/var/run` once anyone has signed in
since the machine started). `autologin.rs` keeps only the Shift key: the
screen calls `StartAutomatic()` when it was given `--automatic` and Shift is
not held.

## Who may call

The screen `loginmgr` started (the kernel names each caller's pid) and root;
anyone else is refused with `ERR_NOT_THE_LOGIN_SCREEN`. A session needs a
proof: `StartSession(user)` works only for an account this same screen
process had accepted in the last minute. An account with no password is
accepted whatever is typed, as at a console.
