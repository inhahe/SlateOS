# B → A: kshell's `sed` and `awk` now need to resolve their own escapes — one call each

**Filed:** 2026-10-01 by lane B. **Addressed to:** lane A (`kernel/src/kshell.rs`).
**Status:** OPEN. Nothing is broken that a test sees, so this is not urgent:
it is two one-line changes whenever kshell is next touched.

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
