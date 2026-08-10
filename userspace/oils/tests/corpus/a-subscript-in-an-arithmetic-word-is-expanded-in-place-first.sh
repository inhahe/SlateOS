# A word being expanded *as arithmetic* expands each of its top-level `[ … ]`
# runs in place, ahead of the reading the arithmetic itself gives them. That is
# `expand_word_internal`'s `[` row, which fires only under `Q_ARITH`:
#
#     case '[':        /*]*/
#       if ((quoted & Q_ARITH) == 0 || shell_compatibility_level <= 51)
#         { … goto add_character; }
#       else
#         {
#           temp = expand_array_subscript (string, &sindex, quoted, word->flags);
#           goto add_string;
#         }                                        /* subst.c:11103-11115 */
#
# `expand_array_subscript` (subst.c:10836-10894) does two things:
#
#   exp = substring (string, si+1, ni);
#   t = expand_subscript_string (exp, quoted & ~(Q_ARITH|Q_DOUBLE_QUOTES));
#   free (exp);
#   exp = t ? sh_backslash_quote (t, abstab, 0) : savestring ("");
#
# — it expands the subscript with `Q_ARITH` and `Q_DOUBLE_QUOTES` **masked off**,
# quoting 0, where a `'` *is* a quote and comes off; and it backslash-quotes the
# result against `abstab` (`[`, `]`, `$`, `` ` ``, `~`, `\`, `'`, `"`;
# subst.c:10848-10857) so the reading that follows cannot expand it again.
#
# **Which words get it.** `[[ -v ]]` always does: `cond_expand_word (…, varop ?
# 3 : 0)` (execute_cmd.c:3913-3919) reaches `call_expand_word_internal (w,
# Q_ARITH)` unconditionally, however plain the operand. Every other arithmetic
# string is gated, inside `expand_arith_string` (subst.c:4032-4074): it scans for
#
#     #define ARITH_EXP_CHAR(s) (s == '$' || s == '`' || s == CTLESC || s == '~')
#
# and only on finding one runs the word expansion; finding none it takes
# `string_quote_removal`, which has no `[` row at all. The gate is asked over the
# **whole** string, so a `~` at the far end decides how a subscript at the near
# end is read: `(( q['1'] ))` is `'1': syntax error` while `(( ~q['1'] ))` reads
# element 1. `test -v` is not `[[ -v ]]` — its operand is an ordinary word.
#
# **The second reading.** Every array reference the arithmetic *evaluator* forms
# has its subscript expanded once more, by `array_expand_index`
# (arrayfunc.c:1358-1363):
#
#   t = expand_arith_string (exp, Q_DOUBLE_QUOTES|Q_ARITH|Q_ARRAYSUB);
#   val = evalexp (t, eflag, &expok);
#
# The backslash-quoting above leaves it nothing to expand, so all it does is
# `Q_DOUBLE_QUOTES`'s backslash rule — a `\` is dropped before `$`, `` ` ``, `"`
# and `\`, and kept before anything else. That is the whole of the asymmetry in
# the error tokens: `(( a[\$] ))` reports a bare `$` and `` (( a[\`] )) `` a bare
# `` ` ``, while `(( a[\~] ))` reports `\~` and `(( a[\'] ))` reports `\'`. An
# **associative** subscript takes the other second reading, `expand_subscript_
# string (t, 0)` (arrayfunc.c:1593), quoting 0, where a backslash comes off
# before anything at all — so `m[\[]` is the key `[`.
#
# Both readings find the closing `]` with `skipsubscript` (`skip_matched_pair`,
# subst.c:2085), never with a bracket count: a backslash hides the character
# after it and a quoted run is stepped over whole. `(( ~a[\[] ))` therefore reads
# the subscript `\[`, and `k='['; [[ -v "m[$k]" ]]` — which reaches the name
# check as `m[\[]`, two opens and one close to a counter — finds its key.
#
# **What it is not.** The subscript is expanded once, not twice: with `x='$y'`,
# `(( a[$x] ))` reports `$y`, never `y`'s value. And a *nested* subscript comes
# apart, because the inner expansion runs at quoting 0 where the `[` row is off
# and the brackets are then quoted as ordinary characters:
# `(( a[b[1]] ))` is silent while `(( ~a[b[1]] ))` is `b\[1\]: syntax error`.
#
# Verified against bash 5.2.37.

a=(A B C D E)
q=(9 8 7 6 5)
s='$(echo 1)'
x='$y'
declare -A m=([k]=1 ['$']=2 ['~']=3 ['x y']=4 ['\']=5 ["'"]=6 ['[']=7)
f() { echo "f" >&2; echo a; }
g() { echo "g" >&2; echo 1; }

echo "=== [[ -v ]] is never gated ==="
[[ -v "a['1']" ]];          echo "1 rc=$?"
[[ -v "a['\$(echo 1)']" ]]; echo "2 rc=$?"
[[ -v "a['x y']" ]];        echo "3 rc=$?"
[[ -v "a[\$s]" ]];          echo "4 rc=$?"
[[ -v "a['@']" ]];          echo "5 rc=$?"
[[ -v a['1'] ]];            echo "6 rc=$?"
[[ -v "m['x y']" ]];        echo "7 rc=$?"
[[ -v "a[i=3]" ]];          echo "8 rc=$? i=$i"
test -v "a['1']";           echo "9 rc=$?"
[[ -v "a[\~]" ]];           echo "10 rc=$?"
[[ -v "a[\$]" ]];           echo "11 rc=$?"
[[ -v "a[1]" ]];            echo "12 rc=$?"
t="a['1']"; [[ -v "$t" ]];  echo "13 rc=$?"
[[ -v "a[\$(f)]" ]];        echo "14 rc=$?"
[[ -v "$(f)[$(g)]" ]];      echo "15 rc=$?"
[[ -v "a[1][2]" ]];         echo "16 rc=$?"
[[ -v "a[]" ]];             echo "17 rc=$?"
[[ -v "a['1'" ]];           echo "18 rc=$?"
[[ -v "m['\$n']" ]];        echo "19 rc=$?"
[[ -v "a['0'+1]" ]];        echo "20 rc=$?"

echo "=== the gate: one ARITH_EXP_CHAR anywhere opens it ==="
(( ~q['1'] ));              echo "21 rc=$?"
(( q['1'] ));               echo "22 rc=$?"
(( q['1'] + `echo 0` ));    echo "23 rc=$?"
(( q['1'] + $# ));          echo "24 rc=$?"
(( q["1"] ));               echo "25 rc=$?"
(( a["'"$s"'"] ));          echo "26 rc=$?"
let "a['1']";               echo "27 rc=$?"
echo "28 [$(( a['1'] ))]"
echo "29 [${a[q['1']]}]"
echo "30 [${a[~q['1']]}]"

echo "=== the backslash the two readings leave behind ==="
(( a[\q] ));                echo "31 rc=$?"
(( a[\'] ));                echo "32 rc=$?"
(( a[\"] ));                echo "33 rc=$?"
(( a[\\] ));                echo "34 rc=$?"
(( a[\$] ));                echo "35 rc=$?"
(( a[\~] ));                echo "36 rc=$?"
(( a[\`] ));                echo "37 rc=$?"
(( ~a[\q] ));               echo "38 rc=$?"
(( ~a[\'] ));               echo "39 rc=$?"
(( ~a[\"] ));               echo "40 rc=$?"
(( ~a[\\] ));               echo "41 rc=$?"
(( ~a[\[] ));               echo "42 rc=$?"
(( ~a[\]] ));               echo "43 rc=$?"
(( ~a["]"] ));              echo "44 rc=$?"
(( a[1\]] ));               echo "45 rc=$?"

echo "=== expanded once, not twice ==="
(( a[$x] ));                echo "46 rc=$?"
(( a["$x"] ));              echo "47 rc=$?"

echo "=== tilde expansion happens, at quoting 0 ==="
HOME=/zz; (( a[~] ));       echo "48 rc=$?"
HOME=/zz; (( a[~/p] ));     echo "49 rc=$?"

echo "=== a nested subscript survives the closed gate only ==="
b=(0 1 2 3 4); i=3
(( a[b[1]] ));              echo "50 rc=$?"
(( a[b[$i]] ));             echo "51 rc=$?"
(( ~a[b[1]] ));             echo "52 rc=$?"

echo "=== an associative subscript takes the other second reading ==="
(( m[k] ));                 echo "53 rc=$?"
(( m[\$] ));                echo "54 rc=$?"
(( m[\~] ));                echo "55 rc=$?"
n='k'; (( m[$n] ));         echo "56 rc=$?"
(( ~m[\$] ));               echo "57 rc=$?"
(( ~m[\~] ));               echo "58 rc=$?"
(( ~m[\\] ));               echo "59 rc=$?"
(( ~m[\'] ));               echo "60 rc=$?"
(( ~m[\[] ));               echo "61 rc=$?"
(( ~m['x y'] ));            echo "62 rc=$?"
(( ~m["x y"] ));            echo "63 rc=$?"

echo "=== the name check reads the quoting the expansion just wrote ==="
[[ -v "m[\[]" ]];           echo "64 rc=$?"
[[ -v 'm[[]' ]];            echo "65 rc=$?"
k='['; [[ -v "m[$k]" ]];    echo "66 rc=$?"
[[ -v "m['[']" ]];          echo "67 rc=$?"
[[ -v "m[\]]" ]];           echo "68 rc=$?"
[[ -v "m[\\\\]" ]];         echo "69 rc=$?"

echo "=== and through \${…}, which is one reading only ==="
echo "70 [${m['x y']}]"
echo "71 [${m[\$]}]"
echo "72 [${m[\~]}]"

echo "=== a store, which is not an arithmetic word at all ==="
a['1']=Q;                   echo "73 rc=$?"
declare -a d; d['1']=S;     echo "74 rc=$?"

echo TAIL
