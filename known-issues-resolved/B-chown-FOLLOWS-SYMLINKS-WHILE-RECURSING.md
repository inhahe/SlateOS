## B-chown-FOLLOWS-SYMLINKS-WHILE-RECURSING (lane B, 2026-08-22) — FIXED 2026-08-22

**In short:** A *symbolic link* is a file whose entire content is the name of
another file; opening it gets you the other file. `chown -R alice srv/` is
supposed to give alice everything under the `srv` directory and nothing else.
Both of our `chown` implementations, on meeting a symbolic link inside `srv`,
followed it — so a link named `srv/x` pointing at `/etc` made the command walk
into `/etc` and hand alice the system's configuration, and a link pointing at
`/etc/shadow` (the password file) handed her that. Whoever put the files in
`srv` chooses those links, and under `/tmp`, a downloads directory, or a shared
home that is not the person running the command. Fixed: the walk no longer
follows links, and the links it meets are changed rather than their targets.

**UPDATE 2026-09-25: still fixed for the default, and the rest now follows
GNU exactly.** The walk moved into `coreutils::chowncore`, shared with the new
`chgrp`, as a port of `chown-core.c`. `chown -R` with no `-H`/`-L` is
unchanged -- nothing walked through, every link changed as a link -- and
`-R --dereference` without `-H`/`-L`, which asks for the opposite, is now
refused as GNU refuses it. What changed is the two corners where the caller
*asked* for symlinks to be followed, which the fix below had merged into one
rule: under `-R -H` a link met inside the tree now has its target changed
(GNU 9.4, measured), and under `-R -L -h` links are changed rather than their
targets. design-decisions.md §1029 has the reasoning; `scripts/chown-diff.sh`
gained the in-tree links and the loop that tell the rules apart, and against
the old implementation 16 of its cases fail.

### Two separate escapes, either of which is enough

`userspace/coreutils/src/bin/chown.rs`, both fixed:

| # | Old code | What it did |
|---|---|---|
| 1 | `if path.is_dir() { … recurse … }` | `is_dir()` **resolves the link first**, so a link to a directory answered "yes, a directory" and the walk descended through it into a tree the caller never named. |
| 2 | `chown(path, uid, gid)` | `chown(2)` is defined to follow the final symlink. So even without descending, `srv/x -> /etc/shadow` transferred `/etc/shadow`. |

POSIX has a name for the correct behaviour and makes it the default: of
`-H`/`-L`/`-P`, **`-P` (follow nothing) is what `chown -R` does when none is
given**, and it exists for exactly this reason. Neither implementation had any
of the three options, so there was not even a way to ask for the safe
behaviour, let alone get it by default.

The fix, in both copies:

* Traversal is driven by `symlink_metadata`/`DirEntry::file_type()`, which are
  `lstat`-based — a link to a directory reports as a link, so the recursion
  cannot descend through one.
* The syscall issued during traversal is `lchown(2)` (`SYS_FS_SET_OWNER` with
  arg4 bit 0 = NO_FOLLOW), which changes the link inode itself.
* A link named *on the command line* is still followed when `-R` is absent, per
  POSIX — the caller typed that name and can see what it is. Under `-R` it is
  not, because the root of the walk is as good an exit as any leaf.

### The standalone copy said the safe call did not exist

`userspace/chown/src/main.rs` was worse than wrong; it was documented as wrong.
Its `-h`/`--no-dereference` handler printed:

> `chown: warning: -h/--no-dereference is not supported on Slate OS; symlink targets will be affected`

and a comment explained that "the Slate OS VFS set_owner path resolves symlinks
(resolve_follow) and there is no lchown-equivalent syscall yet". That was true
when it was written and stopped being true afterwards: `sys_fs_set_owner` reads
a NO_FOLLOW bit from arg4 (`kernel/src/syscall/handlers.rs`), and
`posix/src/file.rs` → `lchown` has been passing it. The binary was only issuing
`syscall4`, so the fifth argument never reached the kernel — it had no
`syscall5` at all. Adding one is the whole of the plumbing fix.

The stale claim also lived in `todo.txt`'s 2026-05-30 entry as a "JUDGMENT
CALL / DOCUMENTED LIMITATION" marked *"Deferred: low value"*. That is annotated
in place now. It was neither low value nor, by the time anyone read it, still
undone — which is the more useful lesson: a deferral note with no expiry
outlives the condition that justified it, and the next reader takes it as
current.

### Everything else this rewrite fixed

`userspace/coreutils/src/bin/chown.rs` was rewritten rather than patched. The
other defects found in it:

1. **`-r` was accepted as `-R`.** POSIX `chown` has no `-r`, and reading it as
   recursion turns a one-file change into a whole-tree one. There was a test,
   `parse_recursive_lowercase_r_accepted`, asserting the wrong behaviour — the
   **third** instance of that anti-pattern in this audit after `chmod` and
   `dd`. The test was right about what the code did and wrong about what it
   should do, so fixing the bug turned a green test red, which reads like a
   regression to anyone who did not know the history.
2. **Unknown options became file names.** `chown -x alice f` treated `-x` as a
   path. Now rejected.
3. **Paths had to be UTF-8.** `to_str()` on the operand meant a perfectly legal
   filename with a non-UTF-8 byte in it could not be chowned at all. Paths are
   bytes here (`CLAUDE.md` §7); the rewrite uses `OsStr`/`as_bytes` throughout.
4. **The error message was `chown failed (errno)` with no errno in it.** It now
   reports `io::Error::last_os_error()` and the path.
5. **Recursion aborted on the first error** via `?`, so one unreadable
   subdirectory silently left every later sibling unchanged. It now reports and
   continues, tracking the worst status.
6. **`4294967295` as a literal uid collided with the "unchanged" sentinel.**
   POSIX's `(uid_t)-1` — `u32::MAX` — means "leave this field alone", and it is
   passed straight through, which is better than a stat-then-write (no TOCTOU
   window, and an omitted field never becomes a real write).
7. **Directory loops.** The walk now keys on `(st_dev, st_ino)`, so a
   filesystem containing a cycle terminates. Not reachable through symlinks any
   more, but bind mounts and hard-linked directories are not this program's to
   rule out.

`-c/--changes`, `--from` and `--reference` are still unimplemented in the
coreutils copy, and neither copy resolves user *names* without `/etc/users.yaml`
(§353). Both are noted rather than fixed because **B-Q7** has not yet decided
which of the two files survives.

### Testing

The `Runner` is `cfg(unix)`-only, so on the Windows build host — where
`cargo test --workspace` runs — none of it compiles and none of it would be
exercised. The two decisions that matter were therefore extracted as pure,
all-platform functions, `follow_operand(recursive, no_deref)` and
`follow_child()`, and tested directly. A rule nobody can test is a rule that
quietly stops holding. 35 tests in the coreutils copy, 45 in the standalone;
clippy clean for both `x86_64-pc-windows-gnu` and `x86_64-slateos`.

**Five for five.** Every shipped `coreutils` bin read against its bigger
standalone twin so far — `chmod`, `dd`, `tee`, `tar`, `chown` — has had a
silent-wrong-behaviour bug, and this is the second that was exploitable.
