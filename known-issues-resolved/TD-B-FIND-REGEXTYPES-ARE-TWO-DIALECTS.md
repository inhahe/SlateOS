## TD-B-FIND-REGEXTYPES-ARE-TWO-DIALECTS -- `find -regextype` maps thirteen glibc syntaxes onto two (lane B, 2026-10-01) — **FIXED** 2026-10-01

**Status:** FIXED 2026-10-01

**Resolution.** Every type is now the dialect it is: the two Emacs types
through `ere::emacs` (`findutils-default` with `.` matching a newline,
`emacs::compile_dot_newline`), the basic types through `ere::bre::BreSyntax`,
the extended through `ere::Syntax` -- which grew `GNU_AWK` and `AWK` and the
three bits they need (`leading_repeat_literal`, `no_intervals`,
`no_backrefs`) -- and every type with glibc's `newline_anchor`, which
`re_compile_pattern` sets (`-regextype posix-extended -regex 't/a$.b'` finds
`a<newline>b`). `find-diff.sh` runs all thirteen types against seventeen
patterns, each chosen so that one syntax bit decides it: 221 rows, all agreeing.

Measuring the basic types turned up the same split in the tools themselves,
fixed in the same change -- `ere::bre::BreSyntax`, one per GNU syntax:

| | sed, ed, `more` | grep, `diff -I` | `expr`, `csplit`, `nl` |
|---|---|---|---|
| `a**`, `a\{2\}*` | refused | accepted | accepted |
| `\{2\}a` | refused | the text `{2}a` | the text `{2}a` |
| `[z-a]` | refused | refused | matches nothing |

Ours had accepted the first everywhere, refused the second everywhere and
refused the third everywhere; and a `\}` that closes no interval, which every
one of them reads as `}`, was refused here as "unmatched \}". The original
entry follows.

**In short:** `find -regex` matches a file's whole path against a regular
expression, and `-regextype` picks which of GNU's thirteen regex dialects the
pattern is written in. Ours reduces every one of them to "basic" or
"extended", so a pattern that means one thing in the dialect a user named can
mean another here. The default dialect, `findutils-default`, is Emacs syntax
in GNU find, and ours reads it as POSIX basic: GNU's `find -regex '.*\(a\|b\)'`
and ours agree, but `\w` inside a default pattern, `[z-a]`, and a `.` against
a newline in a name do not. `find.rs` (`regex_is_extended`) says this is
"documented in known-issues.md"; it was not, until this entry.

**What each name is in findutils 4.9** (`lib/regextype.c`), and what it needs
here:

| `-regextype` | glibc syntax | here today | the faithful reading |
|---|---|---|---|
| `findutils-default` | `RE_SYNTAX_EMACS \| RE_DOT_NEWLINE` | POSIX basic | `ere::emacs`, with `.` matching a newline |
| `emacs` | `RE_SYNTAX_EMACS` | POSIX basic | `ere::emacs` (as `ptx` uses it) |
| `posix-awk` | `RE_SYNTAX_POSIX_AWK` | POSIX extended | `Syntax::POSIX_AWK` (exists since 2026-10-01) |
| `gnu-awk` | `RE_SYNTAX_GNU_AWK` | POSIX extended | escapes in lists, malformed interval literal, a leading `*` literal, GNU operators on |
| `awk` | `RE_SYNTAX_AWK` | POSIX extended | escapes in lists, **no intervals** (`{` literal), no backreferences, no GNU operators |
| `egrep`, `posix-egrep` | `RE_SYNTAX_EGREP` / `POSIX_EGREP` | POSIX extended | `Syntax::EGREP` |
| `grep` | `RE_SYNTAX_GREP` | POSIX basic | basic with newline-as-alternation |
| `posix-minimal-basic` | `RE_SYNTAX_POSIX_MINIMAL_BASIC` | POSIX basic | basic without `\+ \? \|` (`RE_LIMITED_OPS`) |
| `posix-basic`, `ed`, `sed` | `RE_SYNTAX_POSIX_BASIC` (and `_ED`, `_SED`, equal to it) | POSIX basic | already right |
| `posix-extended` | `RE_SYNTAX_POSIX_EXTENDED` | POSIX extended | already right |

**Where:** `userspace/coreutils/src/bin/find.rs`, `regex_is_extended` and
`compile_regex`. `find-diff.sh` has one case per type (`-regextype emacs -regex
't/su.'` and so on), each written so that the approximation happens to agree;
the fix needs a case per row that the approximation gets wrong, measured.

**The proper fix:** map each name to the dialect it is -- `ere::emacs`, the
`Syntax` constants, and the two or three syntax bits the table shows are not
there yet (no intervals, a context-dependent leading `*`, limited operators) --
rather than to a boolean, and measure every row in `find-diff.sh`.
