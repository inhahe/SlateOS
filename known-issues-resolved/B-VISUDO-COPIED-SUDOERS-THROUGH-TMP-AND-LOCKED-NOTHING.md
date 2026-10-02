## B-VISUDO-COPIED-SUDOERS-THROUGH-TMP-AND-LOCKED-NOTHING -- a predictable `/tmp` copy, a lock two could take, an in-place write with no mode (lane B, 2026-10-01)

**Status:** FIXED 2026-10-01

**In short:** `visudo` is how root edits `/etc/sudoers`. Ours copied it to
`/tmp/visudo-<pid>`, a name anyone can predict, writing through whatever symlink
was planted there -- so another user could have root overwrite a file of their
choosing with the sudoers text; its lock was a `.lck` file checked for and then
written, so two `visudo`s could both take it, and one older than five minutes
was "stale" and taken while its owner was still editing; it wrote the result
over the file in place, with no mode or owner, so a crash mid-write left half a
sudoers file; and at "What now?" the end of input meant "edit again", forever.

**The fix**, after sudo 1.9.15p5's `visudo.c`: the file itself is opened
(created 0440) and `flock`ed for the whole session -- "busy, try again later"
when held, "Edit anyway? [y/N]" when locking fails otherwise; the copy is
upstream's `<file>.tmp` beside it, opened `O_NOFOLLOW`, with a final newline
added and the file's time set; the editor runs as `EDITOR -- file.tmp`; an
emptied copy and an untouched one are refused and reported as upstream reports
them; a copy that does not parse offers (e)dit / e(x)it / (Q)uit, end of input
being `x`; a good copy is given to root:root, mode 0440, *then* renamed over the
file. `visudo -c` says `FILE: parsed OK`, with upstream's colon. 9 tests; the
Linux ones drive a real edit through `sh` as the editor.

**Still not upstream's:** the parser is ours, not sudoers' grammar; the lock is
`flock`, where upstream's `sudo_lock_file` uses `fcntl` record locks (the two do
not exclude each other, which matters only if a real sudo's `visudo` ever runs
on the same file); no `-o`/`-p` owner and mode checks, no `@include`d files, no
`+LINE` jump to the error.

**Where:** `userspace/sudo/src/main.rs` (`edit_sudoers`, `ask_what_now`,
`sudoers_temp_path`); the `.lck` lock and `SudoError::LockError` are gone.
