## B-chmod-FOLLOWS-SYMLINKS-WHILE-RECURSING (lane B, 2026-08-22) — FIXED 2026-08-22

**In short:** The same escape as `B-chown-FOLLOWS-SYMLINKS-WHILE-RECURSING`
immediately above, in `chmod`, found by going looking for it once `chown` had
it. `chmod -R 777 srv/` on a directory containing a *symbolic link* (a file
whose content is the name of another file) named `x` and pointing at
`/etc/shadow` made `/etc/shadow` — the system password file — writable by
everyone. Found in **both** implementations. Fixed: a recursive `chmod` now
skips every symbolic link it meets and never walks through one.

### Why the answer is "skip", not "chown's answer"

`chown` changes the link itself under `-R`; `chmod` does not touch it at all.
That is not an inconsistency, it is what the two operations mean:

* A symlink's owner is a real, meaningful thing — it decides who may delete or
  rename the link. Changing it is a sensible request.
* A symlink's **permission bits are never consulted by anything**. Every access
  is checked against the target. So the only effect a `chmod` on a link can
  have is on its target's bits, which is precisely the escape.

GNU chmod resolves this the same way, and documents it: *"chmod never changes
the permissions of symbolic links … this is not a problem since the permissions
of symbolic links are never used."*

A link named directly on the command line **is** still dereferenced, in both
implementations, also matching GNU (`fts` with `FTS_COMFOLLOW`). The caller
typed that name and can see what it is; the distinction throughout this pair of
entries is between a name the caller chose and a name the filesystem handed us.

### Four defects in the coreutils copy

`userspace/coreutils/src/bin/chmod.rs`:

1. **`if path.is_dir()` in `chmod_recursive`** — follows, so the walk descended
   through a link to a directory and out of the tree. Now `walk_action()` on
   the `lstat`-based `DirEntry::file_type()`.
2. **`fs::set_permissions` on a link** — follows, so the target's mode changed.
   Now links are skipped outright.
3. **`fs::metadata(path).map(…).unwrap_or(0)`** in `apply_chmod` — a stat
   failure silently became "no bits set", which is the base a symbolic mode is
   applied to. So `chmod u+x f` on a file whose mode could not be read did not
   fail; it **set the mode to `0o100`, clearing every other permission on the
   file**. A discarded error that destroys data rather than merely hiding one,
   and exactly the `.unwrap_or_default()` pattern `CLAUDE.md` §9 warns about.
4. **`chmod_recursive` returned `Result` and used `?`**, so the first
   unreadable subdirectory abandoned every sibling after it. `chmod -R 700 ~`
   could report an error and leave most of the home directory world-readable.
   It now reports each failure and keeps walking, matching GNU.

### And in the standalone copy

`userspace/chown/src/main.rs` is a dual-mode binary — invoked as `chmod` it
runs `run_chmod`. Its `collect_recursive` had the identical `ft.is_dir()`
recursion (safe, as it happens: that one *was* already `lstat`-based) but
returned a bare `Vec<PathBuf>`, discarding the fact that an entry was a link,
so `run_chmod` then chmod'ed links and their targets. The walk now returns
`WalkEntry { path, is_symlink }`, carrying the type it already knew at no cost
and with no second `stat` to race against.

### Testing

Same problem as `chown`: the walk is `cfg(unix)` and invisible to the host test
run. The rule is extracted as `walk_action(is_symlink, is_dir) -> Skip | Descend
| Apply` and tested on all platforms, including the one assertion that *is* the
bug — `walk_action(true, true) == Skip`, a symlink to a directory, which the old
`is_dir()` answered "descend" to. 35 tests in the coreutils copy; clippy clean
on both targets.

**Not verified end-to-end**, unlike `tar`: `chmod` is `cfg(unix)`-gated, so it
cannot be run on the Windows build host at all, and a real check needs QEMU.
This is the same gap `dd`, `tee` and `chown` have, and the third argument in
three days for a filesystem-level harness that runs the shipped binaries inside
the OS.
