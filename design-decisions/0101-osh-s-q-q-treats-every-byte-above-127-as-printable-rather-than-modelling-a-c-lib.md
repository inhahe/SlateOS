## §101 — osh's `%q`/`@Q` treats every byte above 127 as printable, rather than modelling a C library's `isprint` table

**Date:** 2026-08-03
**Decided by:** Claude (autonomous)

**Context.** bash's `printf %q` (and `${v@Q}`) picks between two renderings by
asking `ansic_shouldquote`, which walks the value and returns true as soon as a
character is not `isprint` in the current locale. A true answer switches the
whole value to the `$'…'` ANSI-C form; a false one uses backslash quoting.
`isprint` is a *locale* question, and the answers differ sharply:

* glibc in a UTF-8 locale: a byte sequence that does not decode is not
  printable, so a lone `\x80` gives `$'a\200b'`, while valid UTF-8 like `é☃`
  passes through raw.
* the reference bash this project diffs against (Git-for-Windows / Cygwin, and
  identically under `LC_ALL=C`): a single-byte Latin-1-ish table, so
  `0x80`–`0x9F` and `0xAD` are non-printable and every other high byte is
  printable — which makes `é☃` (whose UTF-8 holds `0x98` and `0x83`) come out
  as `$'M-CM-)M-b\230\203'`, half raw and half octal.

osh has no locale machinery at all: it is byte-oriented from the lexer up, and
paths, variables and pipe data are `Vec<u8>` by policy (CLAUDE.md's "paths and
OS-boundary data are bytes").

**Decision.** `printf_quote`/`shell_quote` treat exactly the ASCII controls
(`0x00`–`0x1F` and `0x7F`) as non-printable, and every byte from `0x80` up as
printable and passed through untouched.

> **Narrowed 2026-08-07 by §104.** The rejected alternative below —
> "ANSI-C-quote anything that is not valid UTF-8" — was rejected on the grounds
> that it "commits the shell to UTF-8 as *the* encoding". §104 has since made
> exactly that commitment, on the operator's decision, so the objection no
> longer holds and **that half is now implemented**: a byte that decodes to no
> character forces the `$'…'` form, as does a non-ASCII Unicode control
> (`U+0080`–`U+009F`, which the old byte-level test could not see). The
> byte-string policy is not contradicted — `$'a\377b'` re-reads as the same
> bytes, so nothing the user typed is mangled or lost; only its *rendering*
> changes.
>
> What this entry still decides, unchanged, is the **rest** of a libc's
> `isprint` table: format characters (`U+00AD`, `U+200B`, `U+FEFF`), private use
> and unassigned code points stay printable to osh even though the reference
> bash quotes them. The reason is stronger than it was in August: `U+2028`/
> `U+2029` go the *other* way (newlib prints them, glibc does not), so there is
> no single table to copy, and `Cn` (unassigned) would mean carrying a full
> Unicode assignment table that drifts with each release.
>
> The four surfaces that ask the question — `printf %q`, `${v@Q}`, a
> `declare -p` value, and a `declare -p` associative key — now share one
> predicate, `needs_ansi_c_quote`, so a future category model has exactly one
> place to land. The measured table is in `known-issues.md` under
> `TD-OILS-PRINTF-Q-HIGH-BYTES`.

**Rationale.** It is the only rule a shell with no locale can state honestly. It
agrees with glibc-in-UTF-8 for all *valid* UTF-8 — the case that actually occurs
— and it never mangles a byte the user typed. Encoding the reference host's
Latin-1 `isprint` table would bake a Cygwin implementation detail into an OS
that will never run Cygwin, and would still be wrong for the glibc case.

**Alternatives considered.**

* *Match the reference bash byte-for-byte* (a 256-entry table with `0x80`–`0x9F`
  and `0xAD` non-printable). Would make the differential corpus green over the
  whole byte range, but it is a host artifact, not shell semantics, and would
  read as inexplicable to anyone maintaining the code.
* *ANSI-C-quote anything that is not valid UTF-8.* Principled, and matches
  glibc-in-UTF-8 exactly. Rejected because it commits the shell to UTF-8 as
  *the* encoding, which contradicts the byte-string policy — a Latin-1 filename
  is not an error, and quoting it as octal would make `%q` output that no longer
  round-trips visually.

**Consequence for the corpus.** The differential corpus cannot assert `%q` over
bytes above 127; `tests/corpus/printf-converts-its-width-and-quotes-by-a-deny-list.sh`
says so in its header and stays inside ASCII. The deviation is logged in
`known-issues.md` under `TD-OILS-PRINTF-Q-HIGH-BYTES`.

**Where it lives.** `userspace/oils/src/interp.rs` — `printf_quote`,
`shell_quote`.

**How to reverse.** Replace the `is_ascii_control` test in both quoters with a
predicate that takes the desired `isprint` model, and extend the corpus case.
