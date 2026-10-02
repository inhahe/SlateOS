## TD-B-BASE64-IS-STILL-THE-OLD-CRATE-UNTIL-UUENCODE-MOVES (lane B, 2026-09-25) — FIXED 2026-09-27

**Fixed** as the four steps below say, with one step more than they foresaw.
`uuencode` and `uudecode` are ports of GNU sharutils 4.15.2's, crates of their
own, and `scripts/uu-diff.sh` compares them against Ubuntu 24.04's build
(unpacked by `scripts/sharutils-ref.sh`) in 433 cases: 433 agree. The step
more: sharutils does not parse its own options -- AutoGen generates a table and
GNU's libopts 41.1 does the rest, `~/.sharrc`, `--save-opts`, `--load-opts`,
`--more-help` through `$PAGER`, `--version[=MODE]` and `AUTOOPTS_USAGE`'s
computed layouts included -- so libopts is ported too, as `userspace/autoopts`,
with its surprises kept and listed in that crate's docs. `userspace/base64` is
deleted, `coreutils` has its `base64` bin, and `base64` is back in
`scripts/basenc-diff.sh`, run as itself as well as `basenc --base64`. gnulib's
base64, which coreutils and sharutils both bundle (2023's and 2015's copies,
the same decoder), is one crate, `userspace/gnubase64`, rather than two
transcriptions. The ledger's `base64:uuencode` and `base64:uudecode` lines are
gone, and `userspace/base64`'s `argv-as-string` and workspace-lints lines with
them.

Where upstream's behaviour is undefined, the port had to choose, and says so:
`--save-opts`' warnings pass one argument to a two-`%s` format (upstream prints
a register's leftovers, the port nothing -- the harness normalises exactly those
three messages), and `uudecode` reads bytes no line wrote as zero where
upstream reads its stack (`userspace/uudecode/src/main.rs`, module docs).

**In short:** `coreutils` now has a port of GNU 9.4's `base64` -- it is
`src/basenc.rs`, reachable today as `basenc --base64` and checked against GNU
by `scripts/basenc-diff.sh` -- but no `base64` bin to run it under that name,
because `userspace/base64` still builds one, and two programs of one name are
what `scripts/check-bin-collisions.py` refuses. So the `base64` a user would
get is still the old crate's: its own decoder rather than gnulib's, argv read
as `String`, the whole input read into memory, and `--url-safe` and `--no-pad`
options GNU does not have.

**Why the old crate cannot simply go.** It also carries `uuencode` and
`uudecode`, as argv[0] personalities nothing installs (the ledger's
`base64:uuencode` and `base64:uudecode`). Renaming the crate does not rescue
them: `scripts/multicall-aliases.py` keys an alias by the directory, so
`uuencode:uudecode` would be a *new* unreachable name, which it refuses. The
move the ledger's own header prescribes is to give each tool a producer of its
own, and doing that properly means writing them as what they are -- POSIX
utilities, best ported from GNU sharutils and checked against it -- not
lifting the personalities as they stand, because as they stand they are not
POSIX's:

* `uuencode -m` writes bare base64, with no `begin-base64 MODE NAME` line and
  no `====` terminator, and `uudecode` cannot read it back;
* the mode on the `begin` line is always `644`, not the input file's;
* a zero six-bit group is written as a space (0x20), which mail transports
  strip from line ends -- the reason historical encoders write a backquote;
* `uudecode` skips a final group of fewer than four characters, so a line
  whose trailing spaces were stripped loses data silently.

sharutils is not installed in WSL, so there is not yet anything to check a
port against; that is the first step.

**This change's part:** the crate's `base32` personality is deleted -- it
shadowed `coreutils`' new `base32` (design-decisions §1005) -- which takes
`base64:base32` off the ledger.

**Fix:**
1. Build GNU sharutils in WSL the way `diff-wsl.sh` builds coreutils (or
   install Ubuntu's `sharutils` and note that it is Debian-patched), and write
   `scripts/uu-diff.sh`.
2. `userspace/uuencode` and `userspace/uudecode` as ports of sharutils', each a
   crate of its own, which makes both producers and takes two more ledger
   lines.
3. Delete `userspace/base64`.
4. Add `userspace/coreutils/src/bin/base64.rs` --
   `coreutils::basenc::main(coreutils::basenc::Program::Base64)`, three lines
   -- and put `base64` back in `scripts/basenc-diff.sh`'s `DIFF_BINS`, where it
   was drafted and taken out for this reason.
