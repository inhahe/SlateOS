## TD-B-REGEX-TOOLS-IGNORE-LC-ALL-C -- `grep`, `sed`, `awk`, `expr`, `find`, `csplit`, `nl` and `ptx` read UTF-8 whatever the locale says (lane B, 2026-10-02) — open

**In short:** GNU's regex tools read the input -- and the pattern -- a byte
at a time when the locale is `C` (`LC_ALL=C grep ...`, the idiom scripts use
for speed and for files that are not text), and as UTF-8 characters in a UTF-8
locale. Ours always read UTF-8. Until 2026-10-02 that mostly did not show,
because our `.` also matched a byte that is not UTF-8; since the engine took
glibc's rule (`B-ERE-DOT-TOOK-A-BYTE-GLIBC-LEAVES`), `.*` stops at such a byte
as GNU's does in a UTF-8 locale -- and `LC_ALL=C`, GNU's way past it, does
nothing here. `expr "$path" : '.*/\(.*\)'` on a path that is not UTF-8 is the
case a script meets.

**What the fix is.** Each tool asks `coreutils::locale::ctype_is_utf8()` once
and, when it is false, compiles with the engine's byte reading:
`Regex::new_syntax_bytes` (pattern and subject a byte at a time), and for the
translated dialects the byte variants their translators need (`emacs` has
`compile_bytes`; `bre` and `awk` would need the same). `tac` already does
this (`coreutils/src/bin/tac.rs`, `compile`), and its harness checks
`LC_ALL=C` against GNU. Then each harness gains `LC_ALL=C` cases over the
fixtures that are not UTF-8.

**Waiting on D-Q7** (lane D's open question in `open-questions.md`: is the "C"
locale one byte a character, as on Linux and as POSIX requires -- option B,
Claude's recommendation -- or is SlateOS UTF-8 everywhere, so that `LC_ALL=C`
changes nothing about text -- option A?). The answer decides this entry:

| D-Q7 | This entry |
|---|---|
| B (as Linux) | do the fix above, tool by tool, with `LC_ALL=C` harness cases |
| A (UTF-8 everywhere) | close it: `LC_ALL=C` means nothing anywhere, and `tac`'s switch (`compile` in `tac.rs`) comes out too |

Until then the tools read UTF-8 as `osh` does by the operator's §104, and `tac`
alone reads bytes under `LC_ALL=C` -- the reading B would give everything.
