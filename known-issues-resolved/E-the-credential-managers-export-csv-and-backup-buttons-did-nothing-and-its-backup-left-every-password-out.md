### [E] The credential manager's Export CSV and Backup buttons did nothing, and its "backup" left every password out -- 2026-09-27
**Status:** FIXED (lane E, 2026-09-27), as the operator answered C-Q25 (§1417).

**In short:** Settings drew "Export CSV" and "Backup" buttons that recorded no
target, so pressing them did nothing. Behind them sat two writers nothing
called: a CSV export that quoted a field only when it thought it had to, and a
"backup" that wrote the names of the logins and none of the passwords -- a
file that would have restored a vault of empty entries to someone who believed
it held everything. There was also no vault on disk to back up.

**Now,** with the vault kept on disk (the entry above):

| Control | What it does |
|---|---|
| **Back up...** | writes the vault sealed, under a new nonce: it opens with the master password the vault has now, restores everything, and is as closed as the vault to anyone else |
| **Restore from a backup...** | opens a backup with the master password it was made with -- a wrong one opens nothing -- then asks before replacing this vault's entries; what is restored is sealed under this vault's own master password |
| **Export as plain text...** | says first what the file will be ("readable by anyone -- and any program -- that can open it"), then writes CSV with **every** field quoted and every `"` doubled, CRLF between records -- a password holding a comma, a quote, a line break, a tab or a leading `=` comes back out exactly (B-Q12) |

All three write owner-only files; the file dialog and the dialogs are modal.
`serialize_backup` is deleted.

**Not done:** the operator's third part of C-Q25 -- a program reading a
password through a capability after a prompt -- lives in `gui/credentials`,
the system keyring (lane C). *(Amended the same day: the credential manager
could not **edit** or **delete** an entry either -- `update_entry` and
`remove_entry` had no caller -- which mattered more once entries were kept.
Both are wired: Edit (Ctrl+E) opens the form filled in and keeps what it does
not show; Delete asks first.)*
