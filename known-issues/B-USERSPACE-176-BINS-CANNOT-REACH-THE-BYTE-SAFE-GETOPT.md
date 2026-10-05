## B-USERSPACE-176-BINS-CANNOT-REACH-THE-BYTE-SAFE-GETOPT (lane B, 2026-09-14) — OPEN

`scripts/argv-utf8-baseline.txt` holds **178 findings, 176 of them the same
one**: `let argv: Vec<String> = env::args().collect()`. Every one is a program
that **dies on a legal filename** — `env::args()`'s iterator is a literal
`unwrap`, and on this OS a filename may hold every byte but `/` and NUL.

**Why they are all spelled the same, and why that matters.** The coreutils
bins are clean because they share `userspace/coreutils/src/getopt.rs`, which is
byte-based. Not one `userspace/*/Cargo.toml` depends on `coreutils`, so all 176
standalone bins hand-roll their argv walk in `String`.

> **CORRECTED 2026-09-14, same day, before anything was built on it.** This
> paragraph first said "**No crate outside `userspace/coreutils` can use it**",
> and that is **false**. I had checked that none *does* depend on `coreutils`
> and wrote it up as a wall. Tested instead of asserted: adding
> `coreutils = { path = "../coreutils" }` to `blockdev` and calling
> `coreutils::getopt::Program` **compiles and works today**. There is no wall.
>
> What there is, is **weight**, and that is measured rather than guessed.
> `blockdev`, debug, `x86_64-pc-windows-gnu`:
>
> | | bytes |
> |---|---|
> | as it is | 2,858,660 |
> | with the `coreutils` dependency, using `getopt` | 6,502,583 |
> | | **2.27×** |
>
> because `coreutils`' lib pulls `bignum`, `charwidth`, `ere`, `bstr`,
> `memchr` and `localtime` behind it. A 3.6 MB payload on a small utility to
> reach a 1,838-line parser.
>
> **This changes the priority, not just the wording.** The 176 bins are *not
> blocked* on an extraction — each could be fixed today, either by taking the
> dependency or by converting its argv to `OsString` in place. Extracting
> `getopt` into a small crate is an optimisation with a measured
> justification (the 2.27×), not a prerequisite. Recorded because the original
> phrasing would have had the next reader treat a multi-crate refactor as
> something they had to finish before touching a single bin.

**The precedent is already in the tree.** `userspace/quoting` exists for
exactly this reason: a helper the whole userland needs, extracted into its own
crate rather than copied. `getopt` is the same shape and has not had the same
treatment.

**WHICH of them ship, measured 2026-09-14 — and it reorders the work.**

Intersecting the baseline with `scripts/rootfs-bin-manifest.txt`, which is what
`create-ext4-rootfs.sh` actually installs into `/bin`:

| | count |
|---|---|
| binaries on the argv baseline | 169 |
| binaries on the image | 75 |
| **on both** | **3** — `ar`, `kill`, `logger` |

The coreutils binaries dominate the image and are already clean, because they
share the byte-based `getopt`. So of 169 programs that die on a legal filename,
**three can be run by a user of the current image.** The other 166 are real
defects in binaries that are built and not installed.

That is a correction to how this sweep was being run, not a footnote. The first
seven conversions were chosen by FILE SIZE — smallest first, fastest to
convert — and of those only `diff` is on the image. `blockdev`, `look`,
`nologin`, `timeout`, `lsns`, `eject` and `ldconfig` are all shipped nowhere.
The defects were real and the fixes stand; the ORDER was wrong, and picking by
"cheapest to convert" is what produced it.

**Take the three that ship first.** After them the baseline is a backlog of
things nobody can currently run, and its priority should be read that way —
against, say, the lossy-decode column, which `scripts/lossy-decode.py` reports
as VALUE 0 for the image's binaries and 182 for everything else.

**The route, revised after the measurement above:**

1. **Fix bins now; the extraction is not a gate.** Each bin needs a real pass
   anyway — `patch` and `diff` each took one, and `diff`'s turned up a silent
   `to_str()` skip in its directory walk that was worse than the panic it was
   filed for. That work does not wait on any crate reshuffle.
2. **Extract `getopt.rs` into a small crate when the weight is the reason**,
   not before. The name `userspace/getopt` is taken — it is the `getopt(1)`
   *binary*, and it is itself on the baseline — so a new name is needed.

**The one thing the extraction genuinely blocks on**, found while scoping it
and worth writing down so it is not rediscovered: `Program::report` cannot move
to an I/O-free crate. It goes through `stdfd::diag_line`, which **flushes
stdout first** — glibc's `error()` behaviour, and without it a block-buffered
`cat big missing >out 2>&1` puts the complaint ahead of output written before
it. So an extracted parser either drags `stdfd` (1,357 lines) and `errmsg`
(299) with it, or gives up `report` and changes its 44 call sites. `quoting`
set the standard that a move should cost callers nothing, and this one cannot
meet it for free.

**Taking the `coreutils` dependency is a third option** and is the cheapest
per bin — it compiles and works — but it is the 2.27× above, so it suits a bin
that already wants several of those helpers rather than one that wants only the
parser.

**Scale, measured rather than guessed:** two bins have been converted by hand
so far — `patch` and `diff` — and each took a single focused pass, with `diff`
also turning up a silent-skip bug worse than the panic it was filed for. The
work is real but it is not research.

**Not yet started.** Recorded now because the finding is the structural one,
and because a reader looking at a 176-line baseline needs to know it is one
wall and not 176 separate jobs.

**2026-10-02: 129 left (`python scripts/argv-utf8.py --check`), and the only one
that shipped is done.** Intersected again with `scripts/rootfs-bin-manifest.txt`:
of the 130, only `powerctl` was on the image -- added since the count above,
for the desktop's power buttons (lane C's request) -- and none duplicates a
coreutils binary, so §1005 deletes none of them. `powerctl` is converted, and
true to the pattern, the panic was the lesser defect: **a word a command did
not take was ignored, so `powerctl reboot --help` rebooted the machine** (as
did `shutdown --dry-run`). It now shows the usage for `--help`/`-h` anywhere,
refuses any other extra word before acting, quotes what it refuses, does not
panic on a closed standard output (`println!` did, before the action), and
reports a cancel it could not perform as that rather than "nothing scheduled".
Every caller in the tree passes one word, so none is affected.
