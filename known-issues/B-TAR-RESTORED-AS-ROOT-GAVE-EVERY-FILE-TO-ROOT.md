## B-TAR-RESTORED-AS-ROOT-GAVE-EVERY-FILE-TO-ROOT (lane B, 2026-10-03)

**Status:** FIXED 2026-10-03 (lane B), boot confirmation pending.

**In short:** Restoring a backup as root with GNU tar gives every file back to
the user and group it belonged to, and keeps its exact permissions -- setuid
bits included, with no umask applied. Ours did neither: every file in a root
restore came out owned by root, with its setuid bits stripped and the umask
applied. And on the way, ours created every file readable by everyone until
the very end of the run, so a private file in a backup -- an SSH key, say --
could be opened by any other user while the extraction was still going.

### What was wrong

| | ours | GNU tar 1.35 |
|---|---|---|
| owner of an extracted file, as root | root | the archive's -- by *name* where this machine knows it, else by number |
| mode, as root | `stored & 0o777 & !umask` | `stored`, setuid and all |
| a file stored `0600`, while it is being written | `0644` (created `0666` minus the umask) | `0600`; owner-only when it is about to change hands |
| a directory stored `0700`, until the run is done with it | `0755` | owner-only |
| `--same-owner`, `--no-same-owner`, `--numeric-owner`, `--no-same-permissions` | refused as not implemented | implemented |

### What it does now

- **`Ownership::resolve`** is GNU's `extr_init`: `--same-owner` and `-p` are
  on by default for the superuser and off for everyone else, and each of the
  four options moves its setting the other way.
- **`OwnerLookup`** is GNU's `decode_header`: the archive's user and group
  names are looked up in this machine's passwd and group files, and the
  numbers are used where a name is empty, unknown, or `--numeric-owner` says to
  ignore names. Measured: a member stored as `root` with uid 4321 goes to uid 0.
- **`restore_metadata`** applies times, then the owner, then the mode -- GNU's
  `set_stat` order, since a `chown` can clear setuid bits -- and never gives
  ownership through a symlink, so a member swapped for a link mid-run cannot
  make tar hand away the link's target. Symlinks, delayed ones included, get
  their own owner (`restore_symlink_metadata`); hard links get none, as in GNU.
  A failure is GNU's `Cannot change ownership to uid U, gid G: REASON`, an
  error (status 2).
- **`Ownership::creation_mode` and `safe_dir_mode`** are GNU's creation modes:
  a file is created with its own permission bits (owner-only when it is about
  to be given away) rather than `0666`, and a directory with GNU's
  `safe_dir_mode`. The stored mode still arrives at the end, as before.
- **`--numeric-owner`** also shows numbers in `-tv`/`-xvv` lines and leaves the
  user and group names empty when creating.
- New in `coreutils::dirfd`: `Dir::chown`, through `fchownat`.

### Evidence

`scripts/tar-diff.sh` section 14, seventeen new cases: nine run as root in a
user namespace (where only the caller's uid is mapped, so giving a file to
root succeeds and to uid 1000 fails with `Invalid argument`, identically on
both sides), four as an ordinary user, and four for `--numeric-owner` when
listing and creating. 353 of 353 agree, three known divergences unchanged --
including `tar -xvf` as root, whose ownership errors come out between the
member lines exactly where GNU puts them, which depends on the directory
timing fixed in `B-TAR-GAVE-UP-ON-A-DAMAGED-ARCHIVE-AT-THE-FIRST-BAD-BLOCK`.
Five new unit tests; 167 tar tests pass under Linux, 152 on the host.

### Not done here

- GNU skips the final `chmod` when it already knows the mode is right; ours
  always makes it, which can only matter on a filesystem that refuses `chmod`.
- The final `chmod` still goes through `fchmodat`, which follows a symlink:
  Linux has no `fchmodat` that does not. The `chown` before it does not follow
  one.
