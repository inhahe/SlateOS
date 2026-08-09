# `${x@P}` is the one word expansion whose **errors are returns**, and the one
# caller that keeps them. `expand_prompt_string` is a two-line departure from
# every other entry into `expand_word_internal`:
#
#     no_longjmp_on_fatal_error = 1;
#     value = expand_word_internal (&td, quoted, 0, …);
#     no_longjmp_on_fatal_error = 0;
#     if (value == &expand_word_error || value == &expand_word_fatal)
#       { value = make_word_list (make_bare_word (string), …); return value; }
#                                                     /* subst.c:4459-4467 */
#
# Everything else goes through `call_expand_word_internal`, which turns those
# same two into `exp_jump_to_top_level (DISCARD)` / `(FORCE_EOF)` and does *not*
# consult the flag (subst.c:4326-4334). So the interception belongs to this
# boundary, not to the flag — which is why an error inside a `$( … )` that a
# prompt *runs* still ends that substitution, and only the prompt's own walk
# gets its text back.
#
# What comes back is the whole string, undecoded past that point: the failure is
# not localised to the substitution that raised it, so literals and healthy
# expansions on either side are lost with it. Every class that ends
# `expand_word_internal` early behaves this way — a bad substitution of either
# class (the DISCARD `${x!}` sort and the fatal `@`-transform sort alike), a
# nounset or `${x?word}` abort, a `bad array subscript` from a length, an
# arithmetic error in a word or in a substring bound.
#
# Three things are *not* that:
#
# * A **subscript's** arithmetic error. `array_expand_index` is the one
#   arithmetic caller that reads the flag itself — `if (no_longjmp_on_fatal_error)
#   return 0;` (arrayfunc.c:1372) — so the subscript is 0, the word is expanded
#   in full, and there is no error return to intercept at all.
# * `set -e`. `report_error` exits *at the diagnostic*
#   (`if (exit_immediately_on_error) … exit_shell (…)`, error.c:201), before
#   there is a return value to keep. An arithmetic diagnostic is `internal_error`
#   (`evalerror`, expr.c:1130-1141), which has no such clause, so those still
#   come back as text.
# * `set -o posix`. That promotion is spelled as the *return value*
#   (`(posixly_correct && interactive_shell == 0) ? &expand_wdesc_fatal :
#   &expand_wdesc_error`, subst.c:10051) — so the prompt intercepts it like any
#   other and the shell keeps running.
#
# Verified against bash 5.2.37.

a=(0 1 2)
declare -A m=([k]=K)
s=abcdef
q=QQ

echo "=== the text comes back whole, healthy neighbours and all"
v='A${q@}B'; echo "[${v@P}]"; echo "rc=$?"
w='X${q}Y${q!}Z${q}W'; echo "[${w@P}]"; echo "rc=$?"
x='A${m[]}B'; echo "[${x@P}]"; echo "rc=$?"
y='A${!"z"}B'; echo "[${y@P}]"; echo "rc=$?"
z='A${#a[-9]}B'; echo "[${z@P}]"; echo "rc=$?"

echo "=== a nounset abort is a return like any other"
( set -u; u='A${nope}B'; echo "[${u@P}]"; echo "rc=$?" ) 2>&1; echo "sub=$?"
e='A${nope?boom}B'; echo "[${e@P}]"; echo "rc=$?"

echo "=== an arithmetic error in a word or a bound comes back as text"
f='A$((1+))B'; echo "[${f@P}]"; echo "rc=$?"
g='A${s:1+:2}B'; echo "[${g@P}]"; echo "rc=$?"

echo "=== but a subscript's is answered with 0 and the word carries on"
h='A${a[0p]}B'; echo "[${h@P}]"; echo "rc=$?"
i='A${a[0p]}B${a[2]}C'; echo "[${i@P}]"; echo "rc=$?"

echo "=== a bad array subscript that is not arithmetic is only empty"
j='A${a[-9]}B'; echo "[${j@P}]"; echo "rc=$?"

echo "=== set -e exits at the diagnostic, so there is nothing to keep"
( set -e; v='A${q@}B'; echo "[${v@P}]"; echo "after=$?" ) 2>&1; echo "sub=$?"
( set -eu; u='A${nope}B'; echo "[${u@P}]"; echo "after=$?" ) 2>&1; echo "sub=$?"
( set -e; k='A${q!}B'; echo "[${k@P}]"; echo "after=$?" ) 2>&1; echo "sub=$?"

echo "=== ... except an arithmetic one, which set -e never promoted"
( set -e; f='A$((1+))B'; echo "[${f@P}]"; echo "after=$?" ) 2>&1; echo "sub=$?"
( set -e; h='A${a[0p]}B'; echo "[${h@P}]"; echo "after=$?" ) 2>&1; echo "sub=$?"

echo "=== set -o posix promotes by return value, so the prompt intercepts it"
( set -o posix; v='A${q@}B'; echo "[${v@P}]"; echo "after=$?" ) 2>&1; echo "sub=$?"
( set -uo posix; u='A${nope}B'; echo "[${u@P}]"; echo "after=$?" ) 2>&1; echo "sub=$?"

echo "=== a substitution the prompt runs is not the prompt's own walk"
n='A$(echo "x${q@}y"; echo tail)B'; echo "[${n@P}]"; echo "rc=$?"
o='A$(echo "x${nope?boom}y"; echo tail)B'; echo "[${o@P}]"; echo "rc=$?"
p='A$(echo "x$((1+))y"; echo tail)B'; echo "[${p@P}]"; echo "rc=$?"

echo done
