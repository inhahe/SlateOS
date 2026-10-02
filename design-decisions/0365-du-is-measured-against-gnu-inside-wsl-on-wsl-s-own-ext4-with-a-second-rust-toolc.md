## §365 — `du` is measured against GNU inside WSL, on WSL's own ext4, with a second Rust toolchain — because there is no other place where its inputs exist

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Where it bites:** `scripts/du-diff.sh` (new); `userspace/coreutils/src/bin/du.rs`;
by precedent every future harness for `ls -l`, `find`, `stat`, `df` and `ln`.

**In short:** Every other utility in this tree is checked by running our
version and GNU's version side by side on the development machine and
comparing what they print. `du` cannot be checked that way, because the
numbers `du` reports come from three pieces of information Windows does not
have: which disk a file is on, which file it *is* (so two names for one file
are counted once), and how many disk blocks it actually occupies. Our Windows
build of `du` is therefore a stub that does nothing. The choice was between
leaving `du` unmeasured and building a harness that runs the comparison
somewhere the information exists — inside WSL, the Linux environment already
installed on this machine. It runs inside WSL.

### The options

| | *What changes* |
|---|---|
| **A. Re-exec into WSL and build a Linux binary** (chosen) | `du` is compared against real GNU on 193 command lines; a second Rust toolchain (~1 GB) lives inside WSL; the harness is skipped, with the install command printed, on a machine without one. |
| **B. Record a fixture, as `fnmatch` and `human` do** | No second toolchain; but the recording would have to include a whole filesystem — inode numbers, device numbers, block counts — not just an answer per input. |
| **C. Leave `du` covered by unit tests only** | Nothing new to install; `du` stays checked only against what a person believed GNU does. |

### Why not B, which is the house style

`tests/fnmatch_glibc.rs` and `tests/human_gnu.rs` both work by recording what
the real thing answered and replaying the recording, and §338 gives the
reasons: the host is Windows, a test must not go quiet when a tool is missing,
and a fixture is diffable. Those reasons all still hold. They do not apply
here, because the *input* to `du` is not a string.

`fnmatch`'s input is a pattern and a name; `human`'s is a number and a format.
Both fit in a line of a text file, so a recording is a table. `du`'s input is a
directory tree — with a hard link in it, a sparse file, a device boundary, an
unreadable directory and a symlink loop. A recording of what GNU answered for
that tree only replays if the tree is reconstructed exactly, block counts
included, which means the fixture is not a table of answers but a filesystem
image plus a synthetic `stat`. We already have that synthetic `stat`: it is the
`FakeTree` the 41 unit tests run against. So B would not add coverage over C —
it would only move the same invented numbers into a different file and make
them look measured.

What B cannot do at all is catch the class of bug this harness actually found.
All five differences were in *diagnostics and ordering*, not arithmetic: an
empty name in a `--files0-from` list reported in a pass of its own rather than
where it stands, `read error` versus `cannot open … for reading`, `du ''`
refused without a position, and `--files0-from=` and `-X ''` treated as
standard input when only `-` is. None of those depend on a single block count.
All of them require running the two programs against the same command line.

### The costs, taken deliberately

**A second toolchain.** rustup plus stable inside WSL, per-user, ~1 GB, and a
build directory at `$HOME/du-diff-target` — deliberately outside the
repository, both because `/mnt/d` is 9p and an order of magnitude slower and
because a second `target/` inside the worktree is a tens-of-gigabytes surprise
for whoever next runs `du -sh` on it. The harness prints the one-line install
command and exits 0 when cargo is missing, so a machine without it reports
SKIPPED rather than failing — the same rule §338 states.

**A dependency on WSL.** `wsl -e true` decides it, and its absence is likewise
a skip. This is a harness, not a test: `cargo test` does not need WSL and
`cargo test --workspace` is unaffected.

**The comparison is against glibc's `du`, not against the target's.** Our `du`
built for `x86_64-unknown-linux-gnu` is the same source as the one built for
`x86_64-slateos` — both take the `cfg(unix)` arm, which is what
`toolchain/x86_64-slateos.json`'s `"target-family": ["unix"]` selects — so what
is measured is the code that ships. What is *not* measured is our filesystem's
own `st_blocks` accounting, which is a separate question and belongs to
whoever writes the ext4 tests.

### The one thing the harness had to be told

Both sides are invoked through a `PATH` lookup of the bare word `du`, not by
running a path. gnulib's `set_program_name` keeps the whole of `argv[0]`, so
GNU invoked as `/usr/bin/du` prefixes every diagnostic with `/usr/bin/du: `,
while ours always says `du: ` because each utility here spells its own name
(`Program::new("du", 1)`). The first run reported 48 differences, every one of
them that prefix. A harness that measures how it started the program rather
than what the program does is worse than no harness, because the real
differences were invisible underneath.
