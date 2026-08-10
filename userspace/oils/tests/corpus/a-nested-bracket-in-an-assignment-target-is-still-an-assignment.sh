# Whether a word is an assignment is decided by `assignment` (general.c:432-488),
# and the subscript it may carry is measured with `skipsubscript`, never with a
# search for the first `]`:
#
#       if (c == '[')
#         {
#           newi = skipsubscript (string, indx, (flags & 2) ? 1 : 0);
#           if (string[newi++] != ']')
#             return (0);
#           if (string[newi] == '+' && string[newi+1] == '=')
#             return (newi + 1);
#           return ((string[newi] == '=') ? newi : 0);
#         }                                          /* general.c:467-477 */
#
# `skipsubscript` is `skip_matched_pair` (subst.c:2085), so `[`/`]` nest and a
# quoted run or a substitution is stepped over whole. A nested bracket therefore
# keeps the word an assignment — `c[b[1]]=R` assigns — and a `]` inside quotes
# does not close the subscript: `p["b[1]"]=R` keys on `b[1]`.
#
# What the subscript then *means* is a second question, and the two array kinds
# answer it differently. An indexed subscript goes through `array_expand_index`
# (arrayfunc.c:1358-1363), `expand_arith_string (exp, Q_DOUBLE_QUOTES|Q_ARITH|
# Q_ARRAYSUB)`. With no `$`, `` ` ``, CTLESC or `~` anywhere in it that gate is
# closed and the string takes `string_quote_removal (string, quoted)`
# (subst.c:11892-11952) instead — whose `'` row is
#
#       case '\'':
#         if ((quoted & (Q_HERE_DOCUMENT|Q_DOUBLE_QUOTES)) || dquote)
#           { *r++ = c; sindex++; break; }           /* subst.c:11927-11933 */
#
# — under `Q_DOUBLE_QUOTES` a `'` is an ordinary character and stays, while a `"`
# always toggles and goes. So `c[b['1']]=R` reaches arithmetic still holding its
# quotes and is `'1': syntax error`, but `c[b["1"]]=R` reaches it as `b[1]` and
# assigns. A backslash keeps its slash before anything outside `CBSDQUOTE`, so
# `c[\]]=R` is the error token `\]`.
#
# An *associative* subscript takes the other reading, `expand_subscript_string
# (t, 0)` (arrayfunc.c:1593), quoting 0, where a `'` is a real quote — so the
# same `q[b['x']]=R` is simply the key `b[x]`.
#
# Verified against bash 5.2.37.

b=(0 1 2 3 4)
i=1

echo "=== a nested bracket keeps the word an assignment ==="
c1[b[1]]=R;            echo "1 rc=$? v=${c1[1]}"
c2[b[$i]]=R;           echo "2 rc=$? v=${c2[1]}"
c3[b[\"1\"]]=R;        echo "3 rc=$? v=${c3[1]}"
c4[b[$(echo 1)]]=R;    echo "4 rc=$? v=${c4[1]}"
c5[b[`echo 1`]]=R;     echo "5 rc=$? v=${c5[1]}"
c6[b[b[1]]]=R;         echo "6 rc=$? v=${c6[1]}"
c7[b[$i]]+=Q;          echo "7 rc=$? v=${c7[1]}"
c8[b[1]=2]=R;          echo "8 rc=$? v=${c8[2]} b1=${b[1]}"
b[1]=1   # row 8's subscript assigned through it; put it back

echo "=== a quoted ] does not close the subscript ==="
# A subscript that fails to evaluate discards the rest of its list, so each of
# these reports on the *next* line rather than after a `;`.
d1["b[1]"]=R;          echo "9 rc=$? v=${d1[1]}"
d2[b[1]"]"]=R
echo "10 rc=$? keys=${!d2[@]}"
d3[b[$i]"]"]=R
echo "11 rc=$? keys=${!d3[@]}"

echo "=== what is inside is arithmetic, read under Q_DOUBLE_QUOTES ==="
e1[b['1']]=R
echo "12 rc=$? keys=${!e1[@]}"
e2[b[$i]x]=R
echo "13 rc=$? keys=${!e2[@]}"
e3[\]]=R
echo "14 rc=$? keys=${!e3[@]}"
e4[']']=R
echo "15 rc=$? keys=${!e4[@]}"

echo "=== an associative subscript is a key, not an expression ==="
declare -A m1 m2 m3
m1[b['x']]=R;          echo "16 rc=$? keys=${!m1[@]}"
m2[a[$i]]=R;           echo "17 rc=$? keys=${!m2[@]}"
m3["b[1]"]=R;          echo "18 rc=$? keys=${!m3[@]}"

echo "=== and the word still has to end at the = ==="
f1[b[$i]]x=R;          echo "19 rc=$?"
# Past the first word there is no assignment to test for, so an unclosed
# subscript is just text. (In assignment position it is a *reader* error —
# `unexpected EOF while looking for matching `]'` — which is
# TD-OILS-AN-UNCLOSED-SUBSCRIPT-IN-AN-ASSIGNMENT-POSITION-IS-NOT-A-READER-ERROR
# and belongs to that case, not this one.)
eval 'echo f2[b[$i]=R'; echo "20 rc=$?"

echo TAIL
