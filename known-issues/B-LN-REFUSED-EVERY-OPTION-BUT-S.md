## B-LN-REFUSED-EVERY-OPTION-BUT-S (lane B, 2026-10-03)

**Status:** FIXED 2026-10-03 (lane B), boot confirmation pending.

**In short:** `ln` made links, but only plain hard links and `-s` symbolic
ones. Every other option GNU's `ln` has was refused by name: `-f` (replace),
`-i` (ask first), `-n`, `-r` (relative), `-t`, `-T`, `-v`, the backup options
`-b`/`-S`/`--backup`, `-L`/`-P` and `-d`. So `ln -sf new link` -- the way a
link is pointed somewhere else, and the commonest `ln` command in scripts --
failed. Now every option is GNU coreutils 9.4's, ported from `ln.c` and
gnulib's `force-link.c`.

### What was wrong

`userspace/coreutils/src/bin/ln.rs` parsed `-s` and refused everything else
with `invalid option`. Scripts that relink (`ln -sfn`), install trees with
relative links (`ln -sr`) or log what they do (`ln -v`) stopped at the first
`ln`.

### What it does now

The port follows upstream function by function (see the module docs):

* GNU `main`'s operand forms, including its rule that a two-operand `ln`
  tries the link first and only then treats the second operand as a directory
  -- on `EEXIST`, `ENOTDIR` or `EINVAL`, and not for a symlink under `-n`.
* A replaced destination is never removed first. The new link is made under a
  `CuXXXXXX` name in the destination's directory and renamed over it
  (`force_linkat`/`force_symlinkat`), so `ln -f nosuch b` keeps `b`.
* The refusals: a directory is never overwritten, a file is never replaced by
  a link to itself, and a hard link made earlier in the same run is not
  replaced by a later one of the same name (`dest_set`).
* `-r` shares `canon::relpath` and `canon::path_common_prefix` with
  `realpath --relative-to` (moved from `realpath.rs` into `canon.rs`).
* Backups go through `coreutils::backup`, with `$VERSION_CONTROL` and
  `$SIMPLE_BACKUP_SUFFIX`.
* `-i` reads its answers through `coreutils::yesno`, and the program ends in
  upstream's `close_stdin` (see
  `B-PROMPTS-READ-STANDARD-INPUT-UNLIKE-STDIO`).

### How it is checked

`scripts/ln-diff.sh` (new) runs 154 cases against a built GNU coreutils 9.4,
each in a fresh fixture per side, comparing stdout, stderr, status and a
listing of the fixture afterwards (each symlink's text, each file's link count
and bytes). 151 agree; the other three are `--help` and `--version`, which
differ on purpose. Measured separately: closed and full stdout and stderr,
and closed stdin, for nine `ln` command lines -- every row agrees except
`--help`'s text.
