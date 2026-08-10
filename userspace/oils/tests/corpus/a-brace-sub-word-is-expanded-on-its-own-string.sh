# A `${ … }` is read twice, over two different strings, and a `$( … )` inside it
# has a different remainder for each.
#
# The *scan* — `extract_dollar_brace_string` — walks the whole word, so a `$(`
# it meets is handed everything after it, the closing `}` and the rest of the
# word included. The *expansion* does not: `parameter_brace_expand` cuts the
# sub-word out as a string of its own (`patstr` for a pattern,
# `rep` for a replacement, the bound text for a substring) and hands it to a
# fresh `expand_word_internal`. So the same `$( … )` sees `fi⏎q)r}B` on the
# scan and `fi⏎q)r` on the expansion, and only the second decides what the
# expansion is worth.
#
# Two things follow when the read *fails* — which needs `no_longjmp_on_fatal_error`,
# i.e. a prompt, so `@P` throughout:
#
#  1. The leftover the failed read hands back is the sub-word's, not the word's.
#     `${z#p$(fi⏎q)r}` gives the pattern `p⏎q)r`; reusing the word-scoped
#     leftover would give `p⏎q)r}B` and then repeat the word's own `B`.
#  2. `sindex` ends at the end of *that* string, so the walk that stops is the
#     sub-word's. The `r` after the `$( … )` is not read again, and the word's
#     own `B` still is: bash prints `AzzB`, not `Azz`.
#
# Every sub-word reader is its own `expand_word_internal` for this purpose —
# `parameter_brace_remove_pattern`, `parameter_brace_patsub`'s
# `expand_string_if_necessary (rep, …)` (subst.c:9180-9187),
# `array_expand_index`'s `expand_arith_string`, and `cond_expand_word` for a
# `case` arm — so each owns a `sindex` and none of them may end the walk of the
# word around it.
#
# The `${x:-w}`/`${x:=w}` operand is the same shape *plus* a third read the
# others do not have (`parameter_brace_expand_rhs`'s
# `string_extract_double_quoted`, subst.c:7726-7731), so it is left to its own
# case — see `known-issues.md`,
# TD-OILS-A-BRACE-OPERAND-IS-SCANNED-AGAIN-BEFORE-IT-IS-EXPANDED.
#
# Verified against bash 5.2.37.

z=zz

# --- pattern sub-words: the word's tail survives the failed read ----------
b='A${z#p$(fi
q)r}B';        printf '1 [%s]\n' "${b@P}"
b='A${z%p$(fi
q)r}B';        printf '2 [%s]\n' "${b@P}"
b='A${z##p$(fi
q)r}B';        printf '3 [%s]\n' "${b@P}"
b='A${z/p$(fi
q)r/X}B';      printf '4 [%s]\n' "${b@P}"
b='A${z^^p$(fi
q)r}B';        printf '5 [%s]\n' "${b@P}"
b='A${z#p$(fi
q)r}B${z}C';   printf '6 [%s]\n' "${b@P}"

# --- and the pattern really is the operand-scoped leftover ----------------
printf -v y 'p\nq)rX'
b='A${y/$(fi
q)r/-}B';      printf '7 [%s]\n' "${b@P}"
b='A${y#$(fi
q)r}B';        printf '8 [%s]\n' "${b@P}"

# --- a replacement is its own string too ----------------------------------
b='A${y//X/$(fi
q)r}B';       printf '9 [%s]\n' "${b@P}"
b='A${y//X/$(fi
q)r}';        printf '10 [%s]\n' "${b@P}"

# --- a substring bound, whose reader is `expand_arith_string` -------------
b='A${z:$(fi
q)r}B';        printf '11 [%s]\n' "${b@P}"
b='A${z:1:$(fi
q)r}B';        printf '12 [%s]\n' "${b@P}"

# --- a subscript, already its own string, unchanged by any of this --------
declare -a a=(0 1 2)
b='A${a[$(fi
q)r]}B';       printf '13 [%s]\n' "${b@P}"

# --- a read that closes takes the ordinary path ---------------------------
b='A${z#p$(echo ok
q)r}B';       printf '14 [%s]\n' "${b@P}"
b='A${z//X/$(echo ok)}B';  printf '15 [%s]\n' "${b@P}"

# --- the same text read by a *parser* is a parse error, not any of this ---
( eval 'printf "16 [%s]\n" "A${z#p$(fi
q)r}B"' ); echo "16 rc=$?"

# --- PS4 is the other prompt string, and reaches the same code ------------
z=zz
PS4='A${z#p$(fi
q)r}B '
set -x
: mark
set +x
PS4='+ '

echo "after $?"
