## BUG-AWK-NAMES-AN-OPTION-NOBODY-TYPED — `awk -é` reports `unknown option -Ã` (lane B)

**Status:** FIXED 2026-08-21, in the same sweep that closed
`BUG-QUOTE-ESCAPES-VALID-MULTI-BYTE-CHARACTERS`.

**What it was.** `src/bin/awk/main.rs`'s option loop ended with

```rust
other => return Err(format!("unknown option -{}", other as char)),
```

`other` is a `u8` and `other as char` is a *numeric* conversion: it reads the
byte as a code point. `-é` is the two bytes `0xC3 0xA9`, so the loop rejected
`0xC3`, converted it to `U+00C3`, and printed `unknown option -Ã` — a letter
that is not on the command line, is not on the keyboard the user typed `é`
with, and re-encodes as two different bytes on the way out. Verified against
the built binary before the fix.

`sort`'s option loop carries a comment warning about this exact conversion
(`src/bin/sort/main.rs`, the `other =>` arm) and calls
`Program::invalid_option` instead. awk's loop is hand-rolled — it uses the
POSIX `unknown option -x` wording rather than glibc's `invalid option -- 'x'`,
so it never adopted the helper — and so kept the bug.

**The fix.** Render the byte with `coreutils::quote::escape_unprintable`,
which gives one three-digit octal escape for anything that is not a printable
character: `unknown option -\303`. That is the same answer `seq`'s
`directive_char`, `tr`'s `printable_char` and `csplit`'s conversion-specifier
diagnostic already give, and it now comes from the shared helper rather than a
fourth private copy of the rule.

**Why octal and not the whole character**, when `lex.rs` was changed in the
opposite direction in the same commit: the option loop walks *bytes*, and from
inside it a continuation byte and the next flag in a bundle are
indistinguishable — `-\xc3\xa9` could be one character or two bad flags, and
the loop has no way to tell. The lexer has the whole program text and can tell,
so it names the character. The two are not inconsistent; they are two different
amounts of information. Both are written down at their call sites.

**Test:** `an_unknown_option_byte_is_escaped_rather_than_cast_to_a_character`
in `src/bin/awk/main.rs`, which pins `-é`, `-\xff`, `-\x01` and the unchanged
ASCII answers.

**Not covered by a harness, and it cannot be.** Measured: `gawk -é 'BEGIN{}'`
under `LC_ALL=C.UTF-8` exits 1 and prints its own multi-line usage text,
**never naming the offending option at all** (`grep -c 'invalid option'` = 0).
Ours exits 1 too, so the statuses agree, but there is no GNU sentence to
compare our sentence against — a diff case here would be asserting that two
unrelated usage texts differ, which is true and says nothing. `awk-diff.sh`
tests no option errors for this reason; its ten on-purpose divergences are all
about language semantics (`length` counting characters, `\1` as a
backreference, and so on). The unit test is the whole guard here, which is why
it pins the unchanged ASCII answers as well as the fixed ones.

**Where else this class was looked for.** The same sweep checked every `as
char` on a `u8` and every hand-rolled printability test in the tree:
`awk/main.rs:212` (`flag` is provably `F|v|f`), `sed.rs:1707` (`c` is provably
`e|f`), `csplit.rs:573` (the delimiter is `/` or `%`, enforced at both call
sites) and `ed.rs:323` (never reaches a message — ed answers `?`) are all
safe. `od.rs`, `tr.rs`, `strings.rs` and `csplit.rs:874` are byte-wise on
purpose and match glibc's single-byte `isprint` under `C.UTF-8`; the csplit one
had only its printable branch measured (`%s`), so `csplit-diff.sh` gained three
rows — `%é`, `%\001` and `% ` — that exercise the other branch and the
`isprint`/`isgraph` boundary.

A fourth row, `%<0xFF>`, was written and then deliberately removed: arguments
reach both sides through a Windows command line, which is UTF-16, so MSYS and
WSL each widen `0xFF` to U+00FF and deliver `ÿ` (`\303\277`). Measured — GNU
reports `\303` for it, identical to `%é`, because that is what it received. The
row would have passed while testing something other than its name. The
harness now carries that finding as a comment so the case is not re-added; the
undecodable-byte question is asked in-process instead, by `quote.rs`'s tests,
where no command line is involved.
