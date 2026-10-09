## 1068. `patch` is Debian's GNU patch 2.7.6, ported whole with its fifteen patches

**Date:** 2026-10-09
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `patch` applies a diff to the files it describes. Ours was written
from scratch and grew one option at a time, each measured against GNU patch, but
it never had ed scripts, git-style diffs, `--merge`, `-D`, more than one backup
style, or the reject formats. It is now a port of GNU patch itself. The version
ported is the one Ubuntu ships: upstream 2.7.6 plus Debian's fifteen patches.
Those patches include the fixes for two security holes, one allowing arbitrary
command execution through an ed script and one following a symbolic link out of
the tree. The comparison harness runs against exactly that binary, so this is
also the only version it can measure byte for byte.

### What was chosen between

| | upstream 2.7.6 as released | **Debian 2.7.6-7, as Ubuntu builds it** | GNU patch 2.8 (2025) |
|---|---|---|---|
| A reference binary to measure against | none (would have to be built) | `/usr/bin/patch` in WSL | none |
| CVE-2018-1000156 (ed script runs commands) | **vulnerable**: the script is piped to `ed`, which reads a rejected command's next line as a new command | fixed: the script goes to a temporary file, `ed`'s standard input | fixed differently |
| CVE-2019-13636 (follows a symlink when opening) | **vulnerable** | fixed: `O_NOFOLLOW` unless `--follow-symlinks` | fixed |
| `-m` for `--merge` | no (disabled in the option string) | yes | yes |
| Crash fixes (mangled rename, `---` at a context hunk's start, `-o` after a file with no final newline, a `cleanup` that recursed, `RLIMIT_NOFILE` unlimited) | crashes | fixed | fixed |

Upstream 2.7.6 as released was never a real option. It can be measured only
against a build of our own making, and it carries two security bugs that
Debian has fixed. 2.8 would be the newest, but nothing here can run it as a
reference, and §1005 says a port is measured or it is not done. Debian's is the
one we can hold to an exact standard, and it has the security fixes.

### What stays different, on purpose

Each of these is listed in the module documentation of
`userspace/coreutils/src/bin/patch/main.rs`, with its reason:

- `--version` names this build.
- Upstream retries with "Plan B" (the file kept in a temporary file of
  fixed-size records) when memory for "Plan A" (the file in memory) runs out.
  A failed allocation stops a Rust program instead, so that path cannot be
  taken. Plan B itself is still there, and `-x 16` still selects it.
- When a signal ends `patch`, its temporary files are removed and the signal
  raised again, as upstream does. Upstream also puts the outputs a git-style
  diff had queued into place from inside the signal handler, using calls that
  are not safe there; this port does not.
- Two pieces of upstream behaviour are undefined in C, and this port does
  something sane with each:
  - A final unterminated line longer than plan B's record, where upstream
    writes past its buffer.
  - The merge search's give-up point, where upstream leaves `too_expensive`
    uninitialized. Here it is never reached. 1,000 random merges up to 150
    lines, with the search's result dumped by `-x 2`, agree with Ubuntu's
    binary on every one.

Everything else is kept, including slips whose effect a user can see:

- A context line that starts with a tab is stored one byte short.
- `Prereq: … at line N` gives a byte offset as N.
- `best_name` can choose no name because it never resets its minima.
- Writing an empty line with no newline reports "write error : Success".
- The ed script's output path gets glibc's assertion message when it begins
  with `-`.
- Every name is quoted as the C locale quotes it, since upstream never calls
  `setlocale`. That is the reason `quoting::Charset::Ascii` exists.

### Measured

`scripts/patch-diff.sh` compares stdout, stderr, exit status and the whole tree
left behind, including modes, symlinks, backups, reject files and the times
`-Z`/`-T` set. It covers:

- every patch format: unified, context and normal diffs, ed scripts, and
  git-style diffs with renames, copies, mode changes, symlinks, binary files
  and C-quoted names
- `--merge` in both styles, and `-D`
- every backup style
- both reject formats
- every debugging flag
- CRLF, indented and RFC 934 patches
- `Prereq:`
- read-only files
- names that climb out of the tree

Its last case is `scripts/patch-merge-fuzz.py`'s random merges.
