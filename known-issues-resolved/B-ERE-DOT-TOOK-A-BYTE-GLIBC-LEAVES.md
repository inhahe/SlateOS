## B-ERE-DOT-TOOK-A-BYTE-GLIBC-LEAVES -- in a UTF-8 reading, `.` and brackets matched an undecodable byte; glibc's never do (lane B, 2026-10-02) — **FIXED** 2026-10-02

**In short:** our regular-expression engine, which `grep`, `sed`, `awk`,
`find`, `expr`, `nl`, `csplit`, `ptx`, `tac` and the shell's `[[ =~ ]]` all
use, let `.` match a byte that is not valid UTF-8, and let `[^x]` (any bracket
expression that could take it) match it too. Every program it stands in for
says the opposite. Measured under `LC_ALL=C.UTF-8` with `a\xffb` as the input:
GNU `grep -c 'a.b'`, `grep -c 'a[^x]b'`, `grep -E` of both, `sed -n '/a.b/p'`
and bash's `[[ $s =~ ^a.b$ ]]` all find **no** match; under `LC_ALL=C` all of
them match. Ours matched in both. Found porting `tac`, whose `-r -s '.'` on such
a file printed the records in a different order from GNU's.

**Why glibc answers that way.** In a multibyte locale its `.` is
`OP_UTF8_PERIOD`, which accepts a complete valid sequence or an ASCII byte and
nothing else; and a bracket expression keeps only the bytes that are
characters on their own (`bitset_mask (sbcset, dfa->sb_char)`, ASCII in a UTF-8
locale), negated or not -- measured, `[[ $'a\xffb' =~ ^a[$'\xff']b$ ]]` fails
under `C.UTF-8` too. A literal byte in the pattern is a `CHARACTER` node and
still matches itself. GNU grep's own matcher documents the same rule.

**The fix.** `ere::engine::takes` -- the one predicate the Pike VM, the
backtracker and the prefilter share -- refuses an undecodable byte to `.` and
to every bracket expression when the subject is read as characters; read as
bytes (`with_byte_chars`, the C locale) nothing changes. The unanchored
search's own step over a character, which had been the same `.` instruction,
became a separate `Skip` that takes anything, so a search still passes over
such a byte to the text beyond it. And the C locale now reads the *pattern* a
byte at a time as well (`Regex::new_syntax_bytes`, `emacs::compile_bytes`), so
`LC_ALL=C tac -r -s '[é]'` sees a bracket of two bytes, as glibc does.

**Who sees a difference.** Anyone matching `.` or a bracket against bytes that
are not UTF-8 in a program that reads characters -- now the GNU answer. libmagic
reads bytes and is unaffected. The engine's own tests that asserted the old
reading now assert both readings, each against its measured GNU behaviour;
every dependent's tests and the thirteen differential harnesses of the programs
built on it were rerun: four cases annotated as deliberate differences now agree
with GNU and are ordinary cases again (`sed` one, `expr` two, `find` one).
