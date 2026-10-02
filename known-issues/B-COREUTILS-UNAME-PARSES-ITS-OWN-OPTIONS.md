## B-COREUTILS-UNAME-PARSES-ITS-OWN-OPTIONS (lane B, 2026-09-11)

`userspace/coreutils/src/bin/uname.rs` parses `argv` by hand rather than through
`coreutils::getopt`, and the ten cases it fails in `scripts/uname-diff.sh` are
all downstream of that one decision.

**No long-option abbreviation (5 cases).** GNU accepts any unambiguous prefix,
so `uname --mach`, `--proc`, `--hard`, `--k` and `--kernel-n` all work. `uname.rs`
matches the long names exactly — `match long { b"machine" => ... }` — so each is
`unrecognized option`. `getopt.rs` implements the prefix rule and documents it
(`sort --fo`).

**Curly quotes in the diagnostic (5 cases).** Ours says
`unrecognized option 'x'` with U+2018/U+2019; GNU says it with ASCII
apostrophes. Checked in four configurations before calling it a defect — the
built GNU 9.4, and Ubuntu's installed binary under `C`, `C.UTF-8` and
`en_US.UTF-8` — and all four are ASCII, because that message comes from glibc's
getopt rather than from coreutils' locale-aware `quote()`. `uname.rs` reaches
for `quote()`; `getopt.rs` uses `named()`, which is ASCII and correct.

**The fix is to route it through `coreutils::getopt`**, which closes both
families at once rather than patching two symptoms. That is also the direction
`scripts/argv-utf8.py` argues for from a different angle: of the 35 bins already
clean of the argv-as-String defect, 24 go through `getopt`; of the 49 dirty
ones, none do. A bin that parses options through the shared module never had a
reason to reach for `String` in the first place.
