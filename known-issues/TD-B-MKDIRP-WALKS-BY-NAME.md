## TD-B-MKDIRP-WALKS-BY-NAME (lane B, 2026-10-02) — **Status: OPEN**

**In short:** `install -d` and `install -D` make a directory's missing
ancestors one whole name at a time (`a`, then `a/b`, then `a/b/c`), where GNU
steps into each directory it makes and creates the next one inside it. Two
things follow that GNU does not have: a name longer than 4096 bytes cannot be
made (`File name too long`), and if someone else replaces a directory with a
symbolic link between two of those steps, the next directory is created
wherever the link points.

**Where.** `userspace/coreutils/src/mkdirp.rs`: `mkancesdirs` and its
`step_into`. The *last* step -- giving the directory its owner and mode -- is
already done GNU's way, through a descriptor opened `O_NOFOLLOW` when this
call made the directory, so the step that hands out permissions cannot be
redirected; it is only the walk above it that goes by name.

**Why it is not the descriptor walk already.** A descriptor walk needs to
step into a directory its user may *search* but not *read* -- `chdir` can, and
an `O_RDONLY` open cannot; `/home/someone` at `0711` is the ordinary case, and
`install -D f /home/someone/sub/f` must work. That wants `O_PATH` (or
`O_SEARCH`) and `mkdirat`/`openat` relative to such a descriptor, which
`coreutils::dirfd` does not offer: it was built for removal walks, which open
every directory to read it anyway. Whether SlateOS's `openat` honours
`O_PATH` has not been checked.

**The proper fix:** give `dirfd::Dir` a search-only descend (`O_PATH`, falling
back as gnulib's `savewd_chdir` does on `EACCES`) and a follow/no-follow
choice, then make `mkancesdirs` keep a `Dir` per step -- `O_NOFOLLOW` for a
directory it just made, following for one it found, exactly upstream's
`SAVEWD_CHDIR_NOFOLLOW` rule. `scripts/install-diff.sh` cannot see the
difference (it has no long names and no racing writer), so the test is a
unit test with a 5000-byte name and one that swaps a component mid-walk.

**Checked 2026-10-03: on SlateOS the fix would buy nothing yet, so it waits
on two other entries.** SlateOS's libc accepts `O_PATH` and gives the
descriptor Linux's `EBADF` on every operation that would touch the file
(`posix/src/file.rs`, `reject_path_fd_entry`), but the open itself still goes
to the kernel as an ordinary open (`SYS_FS_OPEN_MODE`), so a directory its
caller may search and not read -- the `0711` case above -- cannot be opened
at all. And every `*at` call, `mkdirat` and `openat` included, is carried out
by gluing the name onto the text of the path the descriptor was opened with,
then walking that path from the root again
(`B-POSIX-THE-AT-FAMILY-IS-TEXTUAL`). A descriptor walk on SlateOS would
therefore re-walk by name at every step: no longer names than `PATH_MAX`, and
no protection from a component swapped mid-walk -- exactly what it has now.
Under Linux, where the harness runs, it would differ; on the system it ships
on, not until those land. **Trigger:** `B-POSIX-THE-AT-FAMILY-IS-TEXTUAL`
closed, and an `O_PATH` open that needs only search permission.
