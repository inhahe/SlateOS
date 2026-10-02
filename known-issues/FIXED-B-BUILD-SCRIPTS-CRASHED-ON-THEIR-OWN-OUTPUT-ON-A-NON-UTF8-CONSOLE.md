## FIXED-B-BUILD-SCRIPTS-CRASHED-ON-THEIR-OWN-OUTPUT-ON-A-NON-UTF8-CONSOLE (lane B, 2026-08-20)

**What.** `python scripts/ctest-fixtures.py --help` did not print help. It died
with `UnicodeEncodeError: 'charmap' codec can't encode character '\u2192'`.
Reproduced on this machine on 2026-08-20, not hypothetical.

**Why.** These scripts are run from a Windows console, whose code page here is
cp1252 (cp437 on many other machines), and Python's default encoding error
handler is `strict`. The module docstring — which argparse prints as the
`--help` description — contained one right-arrow in a `known-issues.md`
cross-reference. That is enough: one un-encodable character anywhere in the
string aborts the whole write.

**Why it is worse than a cosmetic bug.** The failure lands *inside the print
that was explaining something*. `scripts/check-libc-shape.py` was written the
same day and had the same flaw in its failure path, where the consequence would
have been: the sysroot build breaks, the check correctly diagnoses that
`-C codegen-units=4096` was dropped, and the operator sees a traceback about
character encoding instead of the diagnosis. A guard whose diagnostic can be
destroyed by its own prose is a guard that fails exactly when it is needed.
cp437 is the worse case — there `—`, `§` and `…` are all un-encodable, and this
repo's prose style uses all three constantly.

**Where.** `scripts/ctest-fixtures.py` (module docstring, line 15) and
`scripts/check-libc-shape.py` (failure-path messages).

**Fixed** two ways, because either alone rots:

1. The offending characters were replaced with ASCII (`->`, `--`, `S339`,
   `...`). `check-libc-shape.py` is now ASCII-only in full.
2. Both scripts reconfigure `sys.stdout`/`sys.stderr` to
   `errors="backslashreplace"` at import. Fixing the characters alone would
   have lasted until the next edit — the house style reaches for em dashes by
   default, and nobody will remember this rule. The guard turns that inevitable
   mistake into a cosmetic `\u2014` in the output instead of a lost message.

**Scope measured, deliberately not fixed repo-wide.** 26 of the Python scripts
in `scripts/` contain non-ASCII characters. Each was then actually run, rather
than assumed about:

- **On cp1252 (this machine) only `ctest-fixtures.py` broke.** cp1252 *can*
  encode `—`, `…` and `§`, so the other 25 files are latent, not broken. The
  right-arrow `→` is the one character that is both un-encodable in cp1252 and
  common in this repo's cross-references, and it is the one that bit.
- **On cp437 most of them would break**, since `—`, `…` and `§` are all absent
  there. That includes two scripts CLAUDE.md *requires* every agent to run:
  `run-timeout.py` and `which-lane.py`. Both were checked directly here
  (`--help`) and neither raises on cp1252.

They were left alone on purpose. `scripts/` is shared by all three lanes, the
fix is a mechanical six-line guard per file, and editing 25 files that are not
currently failing would produce a large cross-lane diff at the exact moment
three agents are merging — a certain conflict cost against a hypothetical code
page. The two that were fixed are the two on the sysroot build path, and one of
them was genuinely broken.

**Trigger to do the rest:** the first `UnicodeEncodeError` reported from any
script on a console that is not cp1252, or CI moving to a host with a different
default code page. The sweep is `grep -P '[^\x00-\x7F]' scripts/*.py` plus the
guard copied from `check-libc-shape.py`.
