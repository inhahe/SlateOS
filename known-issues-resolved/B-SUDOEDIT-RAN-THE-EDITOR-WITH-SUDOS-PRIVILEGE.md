## B-SUDOEDIT-RAN-THE-EDITOR-WITH-SUDOS-PRIVILEGE -- on copies at a predictable `/tmp` name, created through whatever symlink was there (lane B, 2026-10-01)

**Status:** FIXED 2026-10-01

**In short:** `sudoedit` exists so that a user allowed to edit one file is not
given a root shell: the editor runs as *them*, on copies, and only the copy-back
is privileged. Ours ran the editor with sudo's own privilege -- `:!sh` from vim
was a shell with it -- on copies at `/tmp/sudoedit-<pid>-<name>`, a name anyone
can predict, written with `fs::copy`, which follows a symlink planted there; and
it wrote the copy back with no check of what the copy had become. Latent, as the
entry above is: nothing can lift `sudo` today.

**The fix**, after sudo 1.9.15p5's `sudo_edit.c`, `edit_open.c` and
`copy_file.c`: originals are opened with `O_NOFOLLOW`, refused if any directory
on the path is a symlink or writable by the caller ("editing files in a
writable directory is not permitted"), and must be regular files; copies are
created `O_EXCL`, mode 0600, under an unguessable name (upstream's
`motdXXXXXXXX.conf` shape) in the first of `/var/tmp`, `/usr/tmp`, `/tmp` the
caller can write, then `fchown`ed to the caller; one editor (the setting split
into words) runs on all of them through `become_user`; each copy is reopened
`O_NOFOLLOW` and must still be a regular file, 0600, the caller's, or its
original is "left unmodified"; an unmoved size and time is "unchanged"; the rest
are written over their originals (opened `O_NOFOLLOW`, then cut to length), and
a copy that cannot be written back is kept and named. The exit status is the
editor's, or 1 on a copy-back failure, as upstream's. 10 tests, the
file-handling ones on Linux.

**Still not upstream's:** only the caller's primary group counts when judging a
directory writable -- `userdb` keeps supplementary memberships as names with no
name-to-gid resolver (`TD-B-USER-SWITCHING-PROGRAMS-CANNOT-RESET-SUPPLEMENTARY-GROUPS`),
so a directory writable through one of those is not refused; the original is
read and written with sudo's own identity rather than the target user's; no
`sudoedit_follow`/`sudoedit_checkdir` settings.

**Where:** `userspace/sudo/src/main.rs` (`edit_files`, `check_path_dirs`,
`check_dir`, `open_original`, `prepare_edit`, `copy_back`).
