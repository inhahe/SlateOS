## B-GREP-NEVER-DETECTED-BINARY-CONTENT (lane B, 2026-08-29) -- **FIXED 2026-08-29**

**In short:** `grep` had no idea what a binary file was. Searching a `.png` or
an executable printed screenfuls of terminal-wrecking bytes, where real grep
prints one line saying the file matched and stops. It now detects binary
content the way GNU does, and all three `--binary-files` behaviours work. Two
narrower gaps are left behind and tracked below; neither is reachable without
first building something else.

`--binary-files=TYPE` names three behaviours for one question -- *this file
holds a NUL; now what?* -- and we had only the third. What landed:

| | behaviour | stdout | stderr | status |
|---|---|---|---|---|
| `binary` (default) | the lines are withheld | nothing | `grep: F: binary file matches` | 0 |
| `without-match` / `-I` | the file is treated as not matching | nothing | nothing | 1 |
| `text` / `-a` | detection is skipped entirely | the bytes, NULs and all | nothing | 0 |

Four details that are easy to get wrong and were each measured against the
reference host's GNU grep 3.11 rather than inferred:

- **The decision is per read buffer, not per line.** A NUL anywhere in the
  first buffer withholds the *whole file's* output, including matches that
  physically precede it. `printf 'ary head\nmid\0dle\nary tail\n'` prints
  nothing at all -- not `ary head`.
- **The diagnostic goes to stderr, and is owed only when lines were what was
  asked for.** `-c`, `-l`, `-L` and `-q` print no lines, so there is nothing to
  withhold and GNU stays silent -- and `-c` still counts the whole file (2 for
  the fixture above), proving the file is searched to the end rather than
  abandoned at the NUL. Upstream spells this `out_quiet_0`.
- **`-I` is a third behaviour, not a quieter default.** It makes the file
  *non-matching*, so `-L` names it and `-l` does not. Suppressing the output
  alone would leave it matching, a difference an exit status cannot see.
- **`-z` disables detection.** NUL is the record terminator then, so it cannot
  also be the marker; upstream guards the whole test with `eol &&`. Without
  this, every `-z` search would call its input binary.

Implementation: `BINARY_PROBE` (32 KiB, upstream's `INITIAL_BUFSIZE`) is the
`BufReader` capacity, and one `fill_buf()` before the first line is searched
does the scan. `search_stream` returns an `Outcome { matched, binary_match }`
rather than a bare `bool`, so the *decision* is made where the bytes are and
the *printing* stays in `Run::search`. The message is assembled as bytes and
emitted with `stdfd::diag_bytes`, not `diag!`: the file name is unquoted
`input_filename()`, a name on this system need not be UTF-8, and formatting it
would mean `from_utf8_lossy` -- the exact corruption the getopt conversion
above existed to remove. That was nearly written the wrong way here.

Verification:

- `bash scripts/grep-diff.sh` (in WSL) -> **513 passed, 0 differed, 7 on
  purpose, 0 unexpectedly agreed**, up from 498/0/12/0. The four `!` lines this
  gap owned all turned XPASS together as predicted, as did a fifth (`grep foo
  zsep` -- the `-z` fixture read *without* `-z`, whose reason had been written
  as a separate `-z` matter). Their markers are gone and eleven further cases
  were added around them: `-l`/`-L`/`-o`/`-n` on a binary file, `-Il`/`-IL` for
  the matching/non-matching distinction, `-zI`, and a binary file that matches
  nothing.
- 114 unit tests pass (six new, plus one rewritten -- see below);
  `cargo clippy` clean.

One existing test had to be rewritten rather than kept:
`a_nul_in_the_input_is_data_like_any_other_byte` asserted that `a\0x\n` is
printed under default options, which was true only because of the bug. It is
now `a_nul_makes_the_input_binary_and_only_dash_a_carries_it_through`, and
asserts both halves -- default suppresses, `-a` passes the byte through
unaltered -- because passing either alone would be consistent with the searcher
mangling the NUL.
