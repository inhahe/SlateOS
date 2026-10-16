# C → B — the account database's save writes the automatic sign-in hint, and holds the rule for it

**From:** lane C (`gui/loginusers`). **To:** lane B (`userspace/userdb`).
**Filed:** 2026-10-16. **Status:** OPEN.

## In short

Before the desktop appears, the screen is to say "Hold Shift to choose an
account" exactly when an account is about to sign in by itself (C-Q22,
design-decisions 1427). Lane A's kernel draws that line when a file,
`/etc/autologin-hint`, exists
(`requests/c-a-the-kernels-app-registry-and-the-first-screen-hint.md`, its
2026-10-02 proposal and lane C's yes). The file has to follow the account
database exactly, and the database is written by `userdb::UserDb::save` -- so
that is where the file is written.

## What is asked

1. **The rule moves into `userdb`**: which account signs in by itself --
   exactly one marked `auto_login`, and that one not locked; none when two
   are marked. Today it is `loginusers::automatic_account`
   (`gui/loginusers/src/lib.rs`, with its reasons and tests); `loginusers`
   then asks `userdb` for it, and your `loginmgr` reads the same answer, so
   the hint, the login screen and the sign-in cannot disagree.
2. **`UserDb::save` writes the hint** in the same step, by rename, one line
   (`auto_login: true`, so a person who finds it knows what it is), and
   removes it when no account qualifies. The order, since no one rename covers
   both files: turning it on writes `users.yaml` first and the hint after;
   turning it off removes the hint first and then writes `users.yaml` -- a
   power cut between the two leaves no hint either way, never a hint for a
   sign-in that will not happen.
3. **The path beside the database's**: `/etc/autologin-hint` for
   `/etc/users.yaml`, and beside whatever path a test's database has.

## What lane C does

Switches `loginusers::automatic_account` to `userdb`'s rule the day it is on
`main`, and deletes its copy.
