### TD-OILS-A-BRACE-WHOSE-NAME-SCAN-RUNS-OFF-A-FRAGMENT-TAKES-THE-OTHER-DIAGNOSTIC — 2026-08-14 — ✅ FIXED 2026-08-14

**Where:** `Shell::expand_unclosed` (`userspace/oils/src/interp.rs`) and the
`Unclosed::BadSubst` the lexer raises for it (`userspace/oils/src/lexer.rs`).

**Repro** (bash 5.2.37):

```sh
declare -a arr=(10 20 30)
declare -A m=([k]=V)
echo "[${arr['x${m']}]"
```

| | |
|---|---|
| bash | `` 'x${m': bad substitution `` |
| osh | ``bad substitution: no closing `}' in 'x${m'`` |

The same string is named — that much was fixed the same day — but it is the
wrong one of bash's two messages.

**Why bash has two.** A `${ … }` in a string is read in two steps, and only the
second one is `extract_dollar_brace_string`. First `parameter_brace_expand`
extracts the *name* with `string_extract (string, &t_index, "#%^,~:-=?+/@}",
SX_VARNAME)` (subst.c:9550), which stops at one of those operator characters or
at the end of the string — `SX_VARNAME` stepping over a whole `[ … ]` subscript
on the way. If it stopped at the end, `c` is `NUL` and the `switch (c)` falls to
`default: case '\0': bad_substitution:` (subst.c:10018-10024), which is
`report_error (_("%s: bad substitution"), string)` and no longjmp. Only if it
stopped at an *operator* does the body go to `extract_dollar_brace_string`, whose
own running-out is the "no closing" one that longjmps (subst.c:1972).

So the two messages divide on whether the unclosed brace got as far as an
operator, and the division is visible:

| fragment | bash |
|---|---|
| `'x${m'` | `` 'x${m': bad substitution `` |
| `'x${#m'` | `` 'x${#m': bad substitution `` |
| `'x${m[0]'` | `` 'x${m[0]': bad substitution `` |
| `'x${m['` | `` 'x${m[': bad substitution `` |
| `'x${m:-'` | ``no closing `}' in 'x${m:-'`` |

**Two things the entry got wrong, found while fixing it.**

*It is not only a fragment.* A here-document body takes the same two messages,
and osh had the same one answer for both — `cat <<E`/`a${m b`/`E` is
`a${m b⏎: bad substitution` in bash. The `${x@P}` case really does collapse
(`no_longjmp_on_fatal_error` makes `extract_dollar_brace_string` return `NULL`
quietly and its caller fall to the same label), which is why the divergence
looked narrower than it was.

*The name scan is not the whole story.* Two checks between it and
`extract_dollar_brace_string` also reach `bad_substitution:` with an operator
already found — `valid_brace_expansion_word` on the name (subst.c:9803) and the
length branch's `string[sindex-1] != RBRACE` (subst.c:9687). So `'x${m[a:b'`
(the `:` *is* reached, but `m[a` is no name) and `'x${#q:-'` are both plain bad
substitutions. A third check, `parameter_brace_expand_indir` (subst.c:9807),
runs there too and reports in the missing brace's place: `a${!nosuch:-b` is
`nosuch: invalid indirect expansion`, and a pointer holding `not a name` is
`not a name: invalid variable name`.

**The fix.** None of this needed new state on `Unclosed::BadSubst`. osh already
had the whole decision procedure — `Shell::unterminated_brace_kind`, written for
the arithmetic-string scanner, which answers `BadSub` / `NoClosing` /
`Indir(name)` from the body text alone and has `Shell::arith_indir_resolves`
beside it for the third. `Shell::expand_unclosed` now asks it, for `close ==
'}'`, before anything else it does, and a new `Shell::unclosed_bad_substitution`
reports the `BadSub` answer naming `text` (bash's `string`) with the
`ErrexitOrPosix` class the `bad_substitution:` label carries.

Asking it *first* matters, and is bash's own order: a `$( … )` written inside
the name is walked over by `string_extract` without being parsed, so
`a${m$(fi) b` names the bad substitution and never mentions the `fi` — where osh
used to run `Shell::unclosed_brace_reads` first and report the `fi`.

**Fixed by:** `Shell::expand_unclosed` + `Shell::unclosed_bad_substitution`
(`userspace/oils/src/interp.rs`). Corpus:
`a-brace-whose-name-scan-runs-off-the-text-is-a-bad-substitution-not-a-missing-brace.sh`
— sixteen shapes covering the fragment, the here-document, the command
substitution in each half, all three indirection outcomes and the prompt
collapse, byte-identical to bash 5.2.37 including stderr.
