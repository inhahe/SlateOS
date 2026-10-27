## TD-B-NOTHING-MAKES-A-SESSION-S-RUNTIME-DIRECTORY (lane B, 2026-10-16)

**Status:** OPEN

**In short:** a desktop that `loginmgr` starts is told its private
directory for sockets and other per-session files is `/run/user/<uid>`
(`XDG_RUNTIME_DIR`), but nothing creates that directory. The first program
in the session that puts a socket there fails with "no such file or
directory" -- or, if it makes the directory itself, makes it with the wrong
owner or permissions, readable by others.

**Found by:** the Opus review of 0dadd1110..9454a6359 (finding 9).

**Where:** `init/loginmgr/src/manager.rs`, `desktop_environment`, names the
directory; `userspace/logind` creates nothing on disk. `/run` itself may not
exist on SlateOS (init makes `/var/run`).

**Why it waits:** nothing reaches it yet. `loginmgr` cannot start a desktop
until the kernel can start a program as another user
(`requests/b-a-start-a-program-as-a-user-holding-only-what-it-is-given.md`),
so no session has a runtime directory to miss.

**The proper fix:** logind, the session manager, owns it, as
systemd-logind does: when a user's first session is created it makes
`/run/user/<uid>` (and `/run/user` with it), owned by the user and the
user's group, mode 0700, empty; when the user's last session ends and the
user does not linger, it removes it and everything in it. Test both with a
scratch root, and that a directory left by a crash is emptied rather than
reused. `loginmgr` then only names it.
