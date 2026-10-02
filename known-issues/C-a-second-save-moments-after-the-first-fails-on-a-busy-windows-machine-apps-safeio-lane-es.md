### [C] A second save moments after the first fails on a busy Windows machine (`apps/safeio`, lane E's) -- 2026-09-28

**Status:** OPEN -- lane E's code; reported in
`requests/c-e-safeio-rename-fails-on-windows-while-something-holds-the-file.md`.

**In short:** `safeio::write_atomically` renames a finished temporary over
the file once. On Windows the rename is refused while another program holds
the target open without `FILE_SHARE_DELETE` -- which the virus scanner and
the indexer do right after a file is written -- so saving the same file
twice in quick succession can fail ("Access is denied"). `apps/email`'s
`a_draft_is_saved_edited_and_deleted` does exactly that, and failed on lane
C's workspace gate twice under load (1e5be9dad), passing otherwise.

**If a workspace run fails on it:** it is this, not your change -- re-run
`cargo test -p email --bin email`. Reproduced on demand by holding the draft
open with `OpenOptions::new().read(true).share_mode(1)` across the second
save. The fix is lane E's: retry the rename a few times on
`PermissionDenied`, as cargo and git for Windows do.
