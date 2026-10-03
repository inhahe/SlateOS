## TD-B-COREUTILS-GETOPT-IGNORED-POSIXLY-CORRECT (lane B, 2026-09-25) — FIXED 2026-09-25

**In short:** with `POSIXLY_CORRECT` set, every GNU program built on glibc's
getopt stops reading options at the first file name, so `POSIXLY_CORRECT=1 cat
f -n` prints `f` unnumbered and then fails to open a file called `-n`. None of
ours did: the shared parser, `coreutils::getopt`, never looked at the variable,
and neither did the seventeen utilities that walk argv by hand. `sort` -- which
GNU keeps out of getopt's rule and gives its own -- had no rule at all.

**How it surfaced.** `scripts/pinky-diff.sh`'s `POSIXLY_CORRECT=1 pinky alice
-q`: GNU treats `-q` as a second user name. The gap was already known in one
place -- `pwd.rs`'s docs called it "crate-wide rather than `pwd`'s" -- but it
pointed at a `known-issues.md` line that described `uniq`, not this, so it was
never tracked as debt of its own.

**How it was closed.** glibc picks one of three orderings from the option
string's first byte, and `Program::parse` now reads that byte the same way.
The table and the reasoning are in `getopt.rs`, "Where option parsing stops":

| prefix | ordering | `POSIXLY_CORRECT` |
|---|---|---|
| none | permute | stops at the first operand |
| `+` | require order | always stops |
| `-` | return in order | never consulted |

- `pr` now passes its upstream string verbatim, leading `-` and all. `tar`
  passes `-` too, because argp's `ARGP_IN_ORDER` builds exactly that (measured:
  `POSIXLY_CORRECT=1 tar -tf t.tar a -v` still lists verbosely). `ed` pins the
  variable off with `Parser::posixly_correct(false)`, because GNU ed parses with
  `carg_parser` and never reads it.
- The seventeen hand-walked parsers (`cat comm cut wc nl ln rmdir paste expand
  fold unexpand tsort head tail csplit split bc`) take the variable as a
  parameter, as `od` and `uniq` already did. Each test module shadows
  `parse_args` with a wrapper that pins it off, so no existing test depends on
  the environment `cargo test` inherited, and each gained a test of both
  answers.
- `sort` got upstream's rule: once a file has been named every word is a file,
  except a traditional `-o FILE`, which `-c` and the 2001 edition both switch
  off. Upstream's `traditional_usage` came with it, so `_POSIX2_VERSION=200112`
  now makes `+POS` a file name unless a `-POS` follows it -- ours had read `+POS`
  as a key in every edition.
- `coreutils::posixver` is gnulib's `posix2_version`, lifted out of `uniq` so
  that `sort` could share it. Its `strtol` now counts the vertical tab as white
  space, which C does and `u8::is_ascii_whitespace` does not.

Pinned by `POSIXLY_CORRECT` blocks in twenty-one harnesses: the seventeen above
bar `ln` and `rmdir`, which have none, plus `sort`, `sed`, `cmp`, `grep`, `ed`
and `tar`. Each row is the file then the option, the option then the file, a
`--` after the file, and the variable set to the empty string, which counts.

**Still to do:** `diff`, `patch` and `hostname` compare argv against exact
spellings instead of using the shared parser at all, and are recorded apart
(below) rather than given a fourth hand-written rule each.
