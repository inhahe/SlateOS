### TD-OILS-AN-ESCAPED-SEPARATOR-LOSES-ITS-BACKSLASH-IN-A-REPLACEMENT-PRINTBACK. `${q/a\/b/Y}` prints back as `${q/a/b/Y}` — 2026-08-08 — ✅ FIXED 2026-08-09

**Where:** `userspace/oils/src/parser.rs` — `parse_replace_pieces`, the one place
the scan *consumes* a backslash (line ~6472: `\` before `/` in the pattern is
dropped and a bare `/` pushed).

**What.** The pattern word then holds a literal `/` with no record that the
source wrote `\/`, so the printback re-emits it bare — and the result does not
even re-parse to the same expansion, since the printed `/` now reads as the
separator:

```text
f() { echo "${q/a\/b/Y}" "${q/a\/b}"; }; declare -f f
    bash:  echo "${q/a\/b/Y}" "${q/a\/b}"
    osh :  echo "${q/a/b/Y}" "${q/a/b/}"
```

The **expansion** is already right — `q="za/bz"; echo "${q/a\/b/Y}"` is `zYz` on
both, and `r="za*bz"; echo "${r/a\*b/Y}"` is `zYz` on both — so this is purely
representational.

**Fixed by deleting the special case**, not by patching it. `skip_construct` —
the generic "a nested construct is copied whole" rule three lines below in the
same loop — already has an escape arm (`'\\' if i + 1 < chs.len() => Some(i + 2)`)
whose own comment says "a `\/` ends no pattern". The two rules were redundant
and disagreed about the backslash, so removing the `\/` branch lets the escape
fall through to the one rule that handles every other escape in both halves of
the body: it hides the `/` from the separator test *and* copies both bytes into
the pattern text. That is bash's arrangement too — `skip_to_delim`
(subst.c:9157) only locates the delimiter and never rewrites `lpatsub`, so the
`\/` reaches `getpattern`, survives `expand_string_for_pat`, and comes back out
of `quote_string_for_globbing` (subst.c:5754, 5764) as a glob escape matching a
literal `/`. The match was already right; the printback now is too, as a
consequence of the same rule rather than a second mechanism.

**Regression cover:** a printback section added to
`a-replacement-separator-is-found-at-one-level-only.sh` — `\/` in the pattern
under all four operator shapes, in a bulk form, in both halves at once, as the
whole replacement (`${s//a/\/}`), and beside the quoted spellings it competes
with (`$'a\/b'`, `'a/b'`, `"a/b"`).
