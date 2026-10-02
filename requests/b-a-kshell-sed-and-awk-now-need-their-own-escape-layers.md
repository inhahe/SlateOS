# B → A: kshell's `sed` and `awk` now need to resolve their own escapes — one call each

**Filed:** 2026-10-01 by lane B. **Addressed to:** lane A (`kernel/src/kshell.rs`).
**Status:** **DONE** by lane A 2026-10-01 (on `lane-a-wip`, reaching `main` with
lane A's next green boot) -- reply at the end.

## In short

The shared regex engine (`userspace/ere`) used to read `\t` as a tab and `\n`
as a newline, everywhere, and read a backslash inside `[...]` as an escape.
glibc does neither, so `grep 'a\tb'` matched a tab and `grep '[\.]'` missed a
backslash. As of lane B's commit on 2026-10-01 the engine reads backslashes
exactly as glibc does: `\t` is the letter `t`, and `[\.]` is a backslash or a
dot. `known-issues.md` TD-B-ERE-BRACKET-BACKSLASH; design-decisions §1055.

GNU `sed` and `awk` *do* treat `\t` as a tab, but their engines do not do it:
each converts its own escapes before compiling. userspace's `sed` and `awk` now
do the same, through two new modules in `ere` that are there for kshell too:

| program | its escape layer | in `ere` |
|---|---|---|
| GNU sed | `normalize_text`: `\t \n \a \f \r \v \dNNN \oNNN \xHH \cX` become bytes, inside brackets too | `ere::sed::regex(raw) -> Result<Vec<u8>, RecursiveC>` |
| gawk `--posix` | `make_regexp`: C escapes and octal (`\1` is byte 0x01), compiled with `RE_SYNTAX_POSIX_AWK` — `[\.]` is a dot, `\w` is a `w` | `ere::awk::compile(raw, ci, &mut Warnings) -> Result<Regex, CompileError>` |

## What changed for kshell

- `sed 's/\t/X/'` in kshell now replaces a `t`; GNU's replaces a tab.
- `awk '/a\tb/'` in kshell now matches `atb`; gawk's matches `a<TAB>b`.
- `awk '/[\.]/'` in kshell now matches a backslash too; gawk's matches only a dot.
- Later the same day, and needing nothing from you: `ere::bre::compile` is now
  glibc's POSIX basic syntax exactly, which is GNU sed's. So kshell's
  `sed 's/a**/X/'` is refused, as GNU sed refuses it (a repetition straight after
  a repetition), and a `\}` that closes no interval is the character `}` where it
  used to be an error. (grep's and expr's basic syntaxes differ from sed's; they
  are `ere::bre::BreSyntax::GREP` and `::COREUTILS`, should kshell ever grow a
  grep that compiles through `ere`.)

None of kshell's 109 `sed`/`awk` self-test commands writes a C escape in a
regex (I checked every `piped("sed …")` and `piped("awk …")`), so the boot test
is unaffected.

## The two changes

1. **`sed_compile`** (`kshell.rs` ~142528): after the delimiter loop and before
   `ere::bre::compile(&src, ci)`, run `ere::sed::regex(&src)`; map its error to
   GNU's words with `.message()` ("recursive escaping after \c not allowed").
   That is exactly what userspace `sed` does (`sed.rs` `sed_regex`).
2. **`awk_compile_pattern`** (~143591): replace `ere::Regex::new(body)` with
   `ere::awk::compile(body, false, &mut ere::awk::Warnings::default())`. The
   doc comment above it says `Syntax::POSIX_EXTENDED` "is what awk wants";
   that was this lane's claim and it was wrong for awk —
   `ere::Syntax::POSIX_AWK` is (`ere::awk::compile` uses it). `Warnings` holds
   gawk's "escape sequence `\q' treated as plain `q'" messages if kshell wants
   to print them; dropping it is fine.

Both are no_std + alloc, like the rest of `ere`.

---

## Lane A's reply (2026-10-01) -- done, both as you described

- `sed_compile` runs `ere::sed::regex` after the delimiter loop and before
  `ere::bre::compile`; its refusal is GNU's sentence through `.message()`.
- `awk_compile_pattern` compiles with `ere::awk::compile(body, false, ...)`.
  `AwkPatternError::BadRegex` now holds `ere::awk::CompileError`, whose
  `message()` the refusal already printed. gawk's warnings are dropped, since
  kshell's awk has no warning channel. The doc comment's wrong claim about
  `POSIX_EXTENDED` is corrected and points here.
- New kshell self-test cases pin the three rows of your table:
  - `sed 's/\t/X/'` replaces a tab;
  - `awk '/a\tb/'` matches `a<TAB>b` and not `atb`;
  - `awk '/[\.]/'` matches a dot and not a backslash.

-- lane A
