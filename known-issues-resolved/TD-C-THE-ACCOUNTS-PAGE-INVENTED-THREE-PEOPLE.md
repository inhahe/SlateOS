## TD-C-THE-ACCOUNTS-PAGE-INVENTED-THREE-PEOPLE -- FIXED 2026-09-15

**In short:** the Settings app's Accounts page listed three users who do not
exist on the machine -- Alice, Bob and Charlie -- with email addresses, login
counts and last-login times, and told you one of them was a child account with
screen-time limits in force. All of it was written into the source as
constants. The page now reads the machine's real account database.

**Date:** 2026-09-15. **Lane:** C.

**What it claimed.** `SettingsState::new` carried three `UserAccount` values:
Alice (Administrator, `alice@example.com`, 142 logins, last seen
2026-05-17 09:34), Bob (Standard, 56 logins) and Charlie (Child, 23 logins).
Selecting Charlie drew a "Family Safety" section reading *"Screen time limits
and content filters are active"* over a "Manage Family Settings" button. There
are no child accounts in this system, no screen-time limits, and no content
filters.

**The fix, which is a wiring rather than a deletion.** `gui/loginusers` already
reads the real account database and is what the login screen and
`apps/lockscreen` use. `SettingsState::new` now starts with an empty list and
`main` calls `load_user_accounts`, following the same split as
`load_appearance`, `load_input` and `load_notifications`: a constructor that
reads a file makes every test depend on the machine it runs on, and this app's
tests are pure `(w, h) -> RenderTree` functions by design. The mapping half is
`set_user_accounts`, which takes a slice, so a test can supply accounts without
a filesystem.

**Three things had no source and are gone rather than mapped:** the email
address and the login count -- the database records neither -- and the child
account, which is not a concept it has. Last-login is real and now comes from
the database as a timestamp, rendered "Never" when it is zero rather than as
1 January 1970, and labelled UTC because a settings window has no way to ask
for the machine's local zone.

**Nothing says which account is signed in**, so none is marked. That was
already the documented fallback in `current_account_picture` -- *"a machine
with nobody signed in does not claim a choice was made"* -- rather than a new
compromise invented for this change.

**The tests are the part worth reading.** Six went red at once, because every
one of them silently used the three invented accounts as its fixture. A fixture
that lives in production code is a fixture nobody can see they are using: the
tests looked self-contained and were not, and the only reason this surfaced is
that the list became empty rather than merely different. They now build their
own three accounts in `account_fixture`. Two of them also asserted hardcoded
emoji that happened to be the invented accounts' pictures, and now read the
expected icons back out of `ACCOUNT_PICTURES`.
