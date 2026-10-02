### BUG-OILS-ESCAPE-DECODERS. Three divergent copies of the backslash-escape table — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** Two of them were parse bugs, not just wrong values:
```sh
v=$'ab\c'      # bash: the 4-char word `ab\c`; osh: "unexpected EOF looking for `''"
v=$'\c\'       # bash: unterminated; osh: happily produced 0x1c
x='a\cAb'; echo "${x@E}"   # bash: 61 01 62; osh: 61 5c 63 41 62
echo -e '\101'             # bash: literal `\101`; osh: `A`
printf '%b' '\?'           # bash: literal `\?`; osh: `?`
printf '\xg'               # bash also warns "printf: missing hex digit for \x"
echo $'a\401b'             # bash: 61 01 62 (byte-masked); osh: 61 c8 81 62
```

**Root cause.** osh had *three* near-copies of the escape table: an inline
decoder in `Lexer::read_ansi_c_quote`, `decode_escape` (ANSI-C / `%b`) in
`interp.rs`, and `echo_expand_escapes` for `echo -e`. Each had drifted
differently, and none matched bash on all four of its escape dialects, which
genuinely disagree:

| | `$'…'`, `${v@E}` | `printf` FORMAT | `printf %b` | `echo -e` |
|---|---|---|---|---|
| `\c` | control character | literal `\c` | stop output | stop output |
| `\?` `\'` `\"` | the bare character | the bare character | literal | literal |
| octal | `\nnn` | `\nnn` | `\0nnn` or `\nnn` | `\0nnn` only |
| bad `\x`/`\u` | silent, literal | diagnostic | diagnostic | silent, literal |

The lexer bug was structural: it decoded escapes *while* scanning for the
closing quote, so an escape that consumes a character could eat the terminator
(`$'ab\c'`) or fail to (`$'\c\'`). bash splits these into `parse_matched_pair`
(which knows only "a backslash quotes the next character") and `ansicstr`.

**Fix.** One table, in the new `userspace/oils/src/escape.rs`, parameterised by
an `EscapeMode` (`AnsiC` / `PrintfFormat` / `PrintfB` / `EchoE`); all three old
copies are gone. `Lexer::read_ansi_c_quote` now scans a raw body and hands it to
`escape::ansi_c_unescape`. `\c` gained its three ANSI-C wrinkles (`\c?` is DEL,
`\c\\` consumes both backslashes, a dangling `\c` stays literal), `\xHH` and
`\nnn` are masked to a byte (so `$'\400'` is a NUL that truncates the word and
`$'\401'` is `\001`), and `printf` reports a malformed `\x`/`\u`/`\U` through a
new `PrintfDiags::notes` channel — messages that print like an error but leave
the exit status at 0, as bash does. Pinned by `tests/corpus/escapes.sh` and the
unit tests in `escape.rs`.

**Known remaining gap.** A byte above 0x7F is materialised as the *code point*
of that byte, so `$'\xff'` is U+00FF (`c3 bf`) where bash writes the single byte
`0xff`. osh stores a shell word as a Rust `String`, which cannot hold invalid
UTF-8; closing this would mean moving words to `Vec<u8>` throughout. The masking
above at least makes every ASCII-range result exact.
