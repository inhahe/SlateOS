### TD-OILS-HELP-FORMAT. `osh help` no-arg listing and per-builtin synopsis *text* don't byte-match bash — PARTIALLY ADDRESSED 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `builtin_help` (the no-pattern
listing branch) and the `HELP_TABLE` usage strings.

**What (fixed 2026-07-19):** a matched `help NAME` / `help -s NAME` topic now
prints bash's `NAME: <usage>` line (the builtin name, a colon, then the usage
synopsis) instead of the bare synopsis. Verified against bash for `return`,
`echo`, `.` (dot), etc.

**What (still open):** two cosmetic gaps remain:

1. **No-arg `help` listing.** bash prints a 5-line header (version banner + "these
   commands are defined internally…" text + the "star means disabled" note) then a
   **two-column, column-major** table of usage synopses, each column ~40 chars wide
   with a leading space and a trailing `>` truncation marker when the synopsis
   overflows the column. osh instead prints every synopsis on its own single line,
   sorted, with no header and no columns. (The per-line synopsis *content* is close;
   only the layout/header differ.)
2. **Simplified synopsis text.** A few `HELP_TABLE` usage strings are abbreviated
   versions of bash's, e.g. `cd [-L|-P] [dir]` vs bash's
   `cd [-L|[-P [-e]] [-@]] [dir]`, and `[ expr ]` vs bash's `[ arg... ]`. The
   `NAME:` prefix now matches, but the synopsis body after it still differs for
   these builtins.
3. **No long description** (noticed 2026-07-29 while adding `logout`).
   `HELP_TABLE` is `(name, usage, short)`, so a plain `help NAME` prints the
   `NAME: <usage>` line and the one-line short description and stops. bash also
   prints a blank line and then a paragraph — for `logout`, "Exits a login shell
   with exit status N.  Returns an error if not executed in a login shell." —
   and, for most builtins, an `Exit Status:` note under that. `help -s NAME`
   and `help -d NAME` *do* byte-match, because they ask for exactly the two
   pieces the table holds; only the full form is short. Corpus cases must
   therefore use `help -s`/`help -d` rather than bare `help NAME`
   (`tests/corpus/logout.sh` does, with no comment — this entry is the reason).
   The proper fix is a fourth `HELP_TABLE` column holding bash's paragraph text
   verbatim for each of the ~58 builtins.

**Impact:** very low — cosmetic stdout text only; `help` output isn't machine-parsed
and the exit status matches. Shows up only when diffing raw `help` output against bash.

**Proper fix:** (1) reproduce bash's header + two-column column-major layout (40-col
width, leading space, `>` truncation, `*`-disabled note) in the no-pattern branch,
emitting a real osh version banner rather than faking bash's; (2) align each
`HELP_TABLE` usage string with bash's exact synopsis wording byte-for-byte.
Mechanical but tedious; parked as low-value.
