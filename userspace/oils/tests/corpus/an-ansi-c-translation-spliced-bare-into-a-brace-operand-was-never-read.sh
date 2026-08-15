# bash translates a `$' … '` where it reads it and splices the **result** back
# over the `$'` it had already written — `ansiexpand`, then `retind -= 2`
# (parse.y:3858-3893). What it splices depends on one row of that code:
#
# ```c
#   if ((rflags & P_DQUOTE) == 0)                     /* parse.y:3882 */
#     { nestret = sh_single_quote (ttrans); … }
#   else if (dolbrace_state == DOLBRACE_QUOTE || dolbrace_state == DOLBRACE_QUOTE2)
#     { nestret = sh_single_quote (ttrans); … }       /* parse.y:3866 */
#   else
#     { nestret = ttrans; nestlen = ttranslen; }      /* parse.y:3887 — bare */
# ```
#
# The third row is the one that matters: written inside `" … "` and still in the
# name or in a `:-`-style operand, the translation goes in **bare**. So the
# stored word for `"${z:-$'$(fi)'}"` is `"${z:-$(fi)}"` — the quotes are gone —
# and that `$( … )` is text no parser ever read. bash meets it only later, when
# something scans the word: `brace_gobbler` with `set -B`, and the operand's own
# `extract_dollar_brace_string` without. Either way the failure is a
# `command substitution:` diagnostic over the **word's** remainder and a
# `jump_to_top_level (DISCARD)` — the one command is dropped, `$?` is 1, and the
# script carries on.
#
# The `#`/`//` rows and the unquoted spelling take the `sh_single_quote`
# branches instead, so their remainder still shows the `'` the requoting put
# back.
#
# Verified against bash 5.2.37.

unset z

echo '=== the bare splice: no parser read the translation, so it runs at expansion'
echo "1[${z:-$'$(echo Q)'}]"
echo "1 rc=$?"
f() { echo "[${z:-$'$( (echo 2) )'}]"; }
declare -f f

echo '=== a body that will not parse is not a script syntax error'
echo "2[${z:-$'$(fi)'}]"
echo "2 rc=$?"
# The branch is never taken, and the read still happens — it is not the
# operand's expansion doing it.
echo "3[${z:+$'$(fi)'}]"
echo "3 rc=$?"
# The spelling of the translation is what counts, not how it was written.
echo "4[${z:-$'\x24(fi)'}]"
echo "4 rc=$?"

echo '=== written past a `#` or a `//` the splice is requoted, so the `'"'"'` shows'
# `sh_single_quote` again (parse.y:3866) — the stored word keeps a `' … '` run
# around the translation, and the remainder a diagnostic quotes still has the
# `'` the requoting put back. What reads it is not the splice but the run: to
# the gobbler, inside `" … "`, a `'` is not a quote at all.
echo "5[${z#$'$(fi)'}]"
echo "5 rc=$?"
echo "6[${z//x/$'$(fi)'}]"
echo "6 rc=$?"

echo '=== no `"` around it, so `sh_single_quote` again and nothing is read'
echo 7[${z:-$'$(fi)'}]
echo "7 rc=$?"

echo '=== with braces off the operand scan reads it instead, and says the same'
set +B
echo "8[${z:-$'$(fi)'}]"
echo "8 rc=$?"
set -B

echo '=== the remainder is of the word as stored — after every splice in it'
echo "9[${z:-$'$(fi)'$'$(echo Q)'}]"
echo "9 rc=$?"
echo "10[${z:-$'$(fi)'}][${z:-$'$(fi2)'}]"
echo "10 rc=$?"
echo "11[${z:-$'$(fi)'}]" more args
echo "11 rc=$?"

echo '=== a backquote in the splice is the run`s own, not a read'
echo "12[${z:-$'\140fi\140'}]"
echo "12 rc=$?"

echo '=== a translation that reshapes the body ends it early'
echo "13[${z:-$'a}b'}]"
echo "13 rc=$?"

echo '=== the failure is the scan`s, so it drops the one command'
v=1
echo "14[${z:-$'$(fi)'}]"
echo "14 rc=$? v=$v"
echo TAIL
