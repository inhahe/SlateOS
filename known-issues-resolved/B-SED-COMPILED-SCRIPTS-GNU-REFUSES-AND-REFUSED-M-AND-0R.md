## B-SED-COMPILED-SCRIPTS-GNU-REFUSES-AND-REFUSED-M-AND-0R (lane B, 2026-10-03)

**Status:** FIXED 2026-10-03 (lane B); boot-tested on main (394c97655, published as 109a26eec).

**In short:** sed reads its script before it reads any input, and GNU's
reader refuses some scripts that cannot mean anything -- `1,2q` (quit on a
range of lines), `1:a` (a label with a line number). Ours ran them. It also
said the wrong thing about a few broken scripts, refused two real GNU
features -- the `M` (multi-line) regex modifier and `0r FILE` (put a file
before the first line) -- and accepted two kinds of regular expression GNU
refuses: an unmatched `)` under `-E`, and `[:alpha:]` written without its
outer brackets, which is almost always a mistake for `[[:alpha:]]`.

### What was wrong, measured against GNU sed 4.9

| script | ours | GNU |
|---|---|---|
| `1,2q`, `1,2Q`, `1,2!q`, `/a/,/b/q` | ran | `command only uses one address`, 1 |
| `1:a` | ran | `: doesn't want any addresses` |
| `{p;1}` | `unexpected `}'` | ``}' doesn't want any addresses` |
| `1#x` | `unknown command: `#'` | `comments don't accept any addresses` |
| `1,p`, `1, p`, `-e '1,' -e p` | `expected an address after `,'`, one character early | `unexpected `,'` |
| `+3p`, `~3p` | `unknown command: `+'` | `invalid usage of +N or ~N as first address` |
| `+0p`, `~0p`, `+p` | `unknown command` | every line: GNU's `ADDR_IS_NULL` |
| `1,2~3p` on five lines | every line -- the range never closed | lines 1 and 2 |
| `2,0~2p` | lines 2 to the end | line 2: the end is tested on the starting line too |
| `/a/Mp`, `s/x/y/M` | `the `M' ... is not supported` | multi-line matching |
| `/a/ I p` | `unknown command: `I'` | the `I` modifier: blanks may separate them |
| `0r FILE` | `invalid usage of line address 0` | the file, then the input |
| `sed -E 's/w)/X/'` | a literal `)` | `Unmatched ) or \)`: GNU clears `RE_UNMATCHED_RIGHT_PAREN_ORD` |
| `s/[:alpha:]/X/` | the set of `:alph` | `character class syntax is [[:space:]], not [:space:]`, 4 |

### The fix

`sed.rs`, the parser: GNU's address rules -- `q`/`Q` one address, `:` `#`
`}` none, `+N`/`~N` read as addresses and refused as first ones unless N is
zero (`Addr::Null`), `BAD_COMMA` one past the character that is not an
address and not past the end of an `-e` fragment, blanks around `~` and
after `+`/`~` and between regex modifiers; `M` through
`ere::Syntax::reg_newline` and `Regex::with_newline_anchor` (not under `-z`,
as GNU's `newline_anchor`); `0rFILE` compiled as GNU compiles it, `1rFILE`
written there and then (`Action::ReadFileNow`); a `first~step` range end
tested on every line including the first. The regex is compiled in the
syntax GNU's `compile_regex_1` sets (`ere` gained the bits), and dfa.c's
complaint about `[:alpha:]` is GNU's `panic`: no location, status 4, unless
`POSIXLY_CORRECT` is set.

One measured difference is kept: GNU compiles `L` and dies when it runs it
(`INTERNAL ERROR: Bad cmd L`, status 4); ours refuses it while reading the
script (`xfail` in the harness).

### Verified

`scripts/sed-diff.sh`: 708 passed, 0 differed, 7 differ on purpose (was 598
and 6). The same day's `--posix` work is
`TD-B-SED-HAS-NO-POSIX-MODE-AND-NO-FOLLOW-SYMLINKS`; grep's half of the
`[:alpha:]` check is `B-GREP-SEARCHED-FOR-A-CLASS-WRITTEN-WITHOUT-ITS-BRACKETS`.
