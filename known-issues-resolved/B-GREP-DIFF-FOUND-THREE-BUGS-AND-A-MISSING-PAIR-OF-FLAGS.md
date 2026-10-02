### B-GREP-DIFF-FOUND-THREE-BUGS-AND-A-MISSING-PAIR-OF-FLAGS -- FIXED (2026-08-23)

**Where:** `userspace/coreutils/src/bin/grep.rs`, `scripts/grep-diff.sh`.

**In short:** a differential harness was added for `grep` to settle one
question about `-E` syntax. It settled that question in three lines of output
and then found four *other* things wrong, none of which any unit test had
noticed, because every unit test asserted the behaviour the program had.

| what | GNU | us, before | why nobody noticed |
|---|---|---|---|
| `grep -o 'o*'` on `foo bar` | one line, `oo` | `oo` surrounded by six blank lines | the `-o` tests all used patterns that cannot match empty |
| `grep -m 0 foo f` | nothing, exit 1 — and *no* `-c` count line | every match, exit 0 | `-m` was tested at 1, 2 and 5 |
| `grep -Z`, `grep -z` | NUL-delimited names and records | `unknown option` | a whole feature cannot be missed by a test of the features present |
| a fixture named `nul` | an ordinary file | the Windows null device | the harness's own bug — see below |

**`-m 0` is the interesting one.** It is not "no limit" and it is not "stop
after the first": GNU prints nothing, reports the file as not matching, *and
suppresses the `-c` count line entirely* rather than printing `0`. The only way
to get that is to answer before reading a line, since the count line is
otherwise printed unconditionally.

**`-Z`/`-z` are not decoration on this system.** A path here may hold any byte
but `/` and NUL — newline included — so `grep -rl … | xargs` is ambiguous *by
construction*, and `grep -rlZ … | xargs -0` is the only spelling that is not.
`-z` is the other half: `find -print0 | xargs -0 grep -z` only works if grep
agrees about what a record is. Measured details that are easy to get wrong and
are now asserted: `-Z` changes the byte after a *file name* only — the one after
a line number stays `:`, or `-nZ` would be unparseable — and `-z` does not reach
the `-c` count line, which ends with a newline even under `-z` because a count
is not a record.

**Two things the harness had to learn about itself**, both worth remembering
for the next `*-diff.sh`:

* **A case that names no file reads stdin, and stdin was the case list.** The
  first such case (`grep a`) swallowed every remaining line; the harness
  reported "12 passed" for a file with 180 cases in it and looked entirely
  healthy doing so. Fixed by feeding the list on fd 3 and giving every case
  `</dev/null` unless it says otherwise.
* **A command substitution discards NUL bytes.** Without the `tr` that
  `find-diff.sh` already had, every `-Z`/`-z` case would have captured
  identically to the same case without the flag — the flags would have "passed"
  by being invisible.

**The eleven deliberate differences**, all XPASS under the
`OURS=/usr/bin/grep` control: three `\<`/`\>`/`\b` refusals (`bre.rs` explains
why — the engine has no word-boundary matcher and there is no spelling that
would quietly do the wrong thing), two binary-file cases (we never replace a
line with `Binary file X matches`), and six recursion cases where the *Windows*
build joins child paths with `\`. That last group is a harness artefact, not a
grep bug: `Path::join` uses the host separator and the target's is `/`. `grep
foo sub` — recursion's sibling that names no child path — is left unmarked as
the control that proves the difference is the separator and nothing else.
