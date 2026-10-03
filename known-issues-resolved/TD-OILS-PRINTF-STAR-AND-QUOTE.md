### TD-OILS-PRINTF-STAR-AND-QUOTE. `printf` skipped the conversion on its `*` and `%(…)T` arguments, ignored a precision on `%b`/`%q`, dropped `%c`'s NUL, and quoted `%q` by a safe list instead of bash's deny list — 2026-08-03 — ✅ **RESOLVED 2026-08-03**

**Where:** `userspace/oils/src/interp.rs` — `format_conversion` (the `*` width
and precision, the `%(FORMAT)T` branch, the `b`/`q`/`c` arms) and
`printf_quote`.

**Five divergences, all measured against bash 5.2.37.**

1. **A `*` width or precision is an argument.** It goes through the same integer
   conversion `%d`'s operand does, so a word that is not a number is reported as
   `ARG: invalid number`, costs printf its exit status, and counts as zero.
   `printf 'A%*sB\n' abc 42` is `A42B` with a diagnostic and status 1; osh
   printed `A42B` silently with status 0. `%*.*s` reads and complains about
   *both* stars. A missing argument stays silent — an empty string is a valid 0.
2. **The seconds of `%(FORMAT)T` are converted the same way.**
   `printf '%(%Y)T' abc` complains and prints `1970`; osh was silent.
3. **An empty strftime format is not an empty result.** bash hands `%X` to
   `strftime`, so `printf '%()T' 0` under `TZ=UTC` is `00:00:00`. osh printed
   nothing.
4. **A precision truncates `%b` and `%q`**, counting the bytes of the *rendered*
   result — the escapes `%b` interpreted and the backslashes `%q` added.
   `%.3b` on `ab\tcd` is `ab<TAB>`; `%.3q` on `a b c` is `a\ `. osh applied a
   precision only to `%s`.
5. **`%c` on an empty *or missing* argument writes a NUL byte**, because C's
   terminator is a first character like any other. `printf 'A%cB'` is `A\0B` and
   the field is one wide. osh wrote nothing.
6. **`%q` quotes by a deny list.** bash's `sh_backslash_quote` names the bytes a
   re-read would treat specially and lets everything else through; osh had an
   allow list of "safe" bytes, which is not the same set. Three visible
   consequences: `,` must be escaped (brace expansion) and was not; `#` is
   special only at the front of a word, so `a#b` needs nothing and osh escaped
   it; `~` is special only at the front or just after an assignment's `=` or
   `:`, so `a~` needs nothing and osh escaped it. Every byte above 127 was also
   backslashed, which is the subject of the separate entry below.

**Coverage added:** `tests/corpus/printf-converts-its-width-and-quotes-by-a-deny-list.sh`
(every printable ASCII byte through `%q` in four positions, plus each rule
above) and four lib tests.
