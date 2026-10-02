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
