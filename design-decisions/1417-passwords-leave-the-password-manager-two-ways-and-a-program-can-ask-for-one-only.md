## 1417. Passwords leave the password manager two ways, and a program can ask for one -- only with a key for it, and your say-so

**Date:** 2026-09-27 &middot; **Decided by:** Operator (Claude recommended B, and A only if wanted; the operator chose both, and added a third way) &middot; **Lane:** C, with E and A

**In short:** The password manager could not export at all. It will offer both
ways out: a plain-text export for moving to another password manager, made
unmistakably clear that it writes every password readable by anyone who gets
the file, and an encrypted backup that restores everything and is useless to
anyone else. And the operator added a third: a program may ask the password
manager for a password directly, over a secure connection, only if it holds a
capability (a system-issued key) for exactly that -- and when it asks, the
password manager shows who is asking and lets you allow or refuse.

**The question:** `open-questions.md` C-Q25 (now resolved).

| Way | What it is | Whose |
|---|---|---|
| Plain-text export | every password, readable, in a file; a warning that says so plainly before it is written, not a checkbox to click through | lane E, `apps/credmanager` (`export_csv`, written and unused) |
| Encrypted backup | a file only the password manager can read, with the vault's own password; restores everything | lane E, `apps/credmanager` -- the existing `serialize_backup` omits the passwords and is not to be connected as it stands |
| A program asks for a password | through the credential service (`gui/credentials`), only with a capability granted for it, and with a prompt showing the asking program's identity, to allow or refuse | lane C, with lane A for the capability |

**Not changed:** the backup writer that holds no passwords stays disconnected
-- a file named like a backup that restores empty logins is the failure this
lane keeps finding.
