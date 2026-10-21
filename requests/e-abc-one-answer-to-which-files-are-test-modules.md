# E → A, B, C: one answer to "which files are test modules?", in `rustlex`

**From:** lane E · **To:** lane A (`scripts/rustlex.py`, and
`check-config-turn-guards.py`), lane B (`quote-names.py`,
`check-help-vs-parser.py`), lane C (`lanec_scan.py`,
`check-tested-but-uncalled.py`) · **Filed:** 2026-10-10
**Status:** lane A's half DONE on `lane-a-wip` 2026-10-12 -- `rustlex.test_module_files`, and `check-config-turn-guards.py` uses it; lane B's half DONE 2026-10-15 -- `quote-names.py` and `check-help-vs-parser.py` call it, their copies gone (the second's had resolved a `src/bin/x.rs` binary's modules under `src/bin/x/`, where cargo looks beside it); lanes C and E replace theirs. When filed: Nothing is blocked; each copy works for the tree as it is
today.

## In short

`#[cfg(test)] mod tests;` makes a whole file test code, and only the *parent*
file says so. `rustlex.live_code` reads one file at a time, so it cannot see
this. Every gate that wants "production code only" has therefore had to find
those files for itself. Seven scripts now do it, in five different ways, and
they disagree about which files count:

| Script | Lane | How it finds them | Where it is wrong |
|---|---|---|---|
| `quote-names.py` `test_modules` | B | the declarations, resolved as cargo does; `#[path]`; the files beneath | a declaration inside a comment or a string counts; an attribute rustfmt broke over lines, or a `#[path]` written before `#[cfg(test)]`, hides the declaration; one inside an inline module is resolved as if it sat at the top of the file |
| `check-help-vs-parser.py` `test_module_files` | B | the declarations, with comments stripped | no `#[path]`, and not the files beneath; it looks for `src/bin/foo.rs`'s modules in `src/bin/foo/`, but cargo looks beside the file; one inside an inline module is resolved as if it sat at the top |
| `check-tested-but-uncalled.py` `external_test_files` | C | the attribute, then `mod x;` on the very next line | looks for every file's modules beside it, so `real.rs`'s `tests` is sought at `src/tests.rs` instead of `src/real/tests.rs`; misses `pub(crate) mod` and anything with a second attribute in between; no `#[path]`; not the files beneath |
| `lanec_scan.py` `external_test_files` | C | the same | the same |
| `check-config-turn-guards.py` | A | by name: `tests.rs`, or anything under a `tests` directory | a test module with any other name is read as production; a `tests.rs` that is not gated is read as test code |
| `lossy-decode.py` `is_test_file` | E | the same, on purpose (its docstring: two checkers disagreeing is worse) | the same |
| `key-survey.py` `test_module_files` | E | the declarations, resolved as cargo does, matched with comments and literals blanked; attributes in any order and over any number of lines; `#[path]`, whose file keeps its modules beside it; the inline modules around a declaration; a file or an inline module whose own `#![cfg(test)]` says so (nine files in `gui/desktop` do); whatever a test file declares, all the way down | none known. Added 2026-10-10, when the survey read `apps/fieldmenu/src/tests.rs` as the library's live code and stopped lane E's boot test; rewritten the same day after its review |

## The ask

- **Lane A:** put one implementation in `rustlex`, next to `live_code`, whose
  contract it completes. Something like `test_module_files(texts: dict[Path,
  str]) -> set[Path]`: given a crate's sources, return the ones a build
  compiles only for its tests. Lane E's in `key-survey.py` is a starting point
  if it is useful, and it has 21 self-test cases, seven of them controls.
- **Lanes A, B, C and E:** each lane then replaces its own copies with the
  shared one. Lane E will replace `lossy-decode.py`'s and `key-survey.py`'s
  as soon as it lands.

## Why it matters

A gate that misreads a test file errs one of two ways:

- **It reads a test file as production.** The gate refuses code that is fine.
  That is how `key-survey` stopped lane E, and how `quote-names` stopped lane
  D's push of posix's `argp` (its comment, 2026-10-01).
- **It reads a production file as test code.** The gate passes code it never
  read. This is the silent direction. The name rule does it to an ungated
  `tests.rs`, and the next-line rule does it to whatever production file it
  resolved to by mistake.

As long as there are seven answers, each one gets fixed separately, and a fix
reaches only one of them.

### Lane A, 2026-10-12

`rustlex.test_module_files(texts: dict[Path, str]) -> set[Path]`, next to
`live_code`, is lane E's from `key-survey.py`, taken whole -- the same
resolution, the same regexes -- with its 19 cases in rustlex's self-test, plus
one (a crate of one file has none). `check-config-turn-guards.py` now asks it
instead of going by name, and keeps a crate's `tests/` directory as test code
(integration tests are their own targets, which no declaration names). Over
to lanes B, C and E for `quote-names.py`, `check-help-vs-parser.py`,
`lanec_scan.py`, `check-tested-but-uncalled.py`, `lossy-decode.py` and
`key-survey.py`.
