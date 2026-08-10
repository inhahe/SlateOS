# An array subscript and a substring bound are the two places bash reads a
# word's text a second time, as arithmetic, and the reader it uses there is
# `expand_arith_string (exp, Q_DOUBLE_QUOTES|Q_ARITH|Q_ARRAYSUB)`
# (arrayfunc.c:1354). `Q_DOUBLE_QUOTES` switches the single quote *off* — so
# inside `[ … ]` a `'` is not a quote but a character, and everything between
# one and its mate is expanded like the inside of a `" … "` and then handed to
# the arithmetic parser with both quotes still standing. `${a['1']}` is
# therefore not element 1 but the error `'1': syntax error: operand expected`,
# and `${a['$(echo 1)']}` *runs* the substitution before saying so.
#
# The very same string reaches `expand_subscript_string (sub, 0)`
# (arrayfunc.c:1145) when the array turns out to be **associative**, and there
# the flag is absent and a `'` is an ordinary quote — `${m['k']}` is the `k`
# element. Which of the two readings applies is not known until the array's
# runtime type is, so both belong to the word.
#
# What is between the quotes was never *parsed*: `parse_matched_pair` reads
# nothing from a `'` to its mate, so a `$( … )` in there reaches a parser for
# the first time here, at expansion. That is why an unparsable body is a
# runtime `command substitution: line N:` diagnostic and not a script syntax
# error — `${a[$(fi)]}`, written without the quotes, never runs at all.
#
# The interior follows double-quoting's rules, not the quote's:
#
#   | written | error token | what it says |
#   |---|---|---|
#   | `'$(echo 1)'` | `'1'` | the substitution ran |
#   | `'$n'` with `n=2` | `'2'` | a parameter expands too |
#   | `` '`echo 1`' `` | `'1'` | so does a backquote |
#   | `'"1"'` | `'1'` | a *double* quote is still a quote, and comes off |
#   | `'"'` | `''` | …even unterminated, which eats the rest |
#   | `'\1'` | `'\1'` | a backslash follows double-quoting's rules |
#   | `'\$n'` | `'$n'` | …so it disappears before a `$` |
#   | `'~'` | `'~'` | no tilde expansion in an arithmetic string |
#
# See the-brace-scanner-reads-the-command-substitutions-a-single-quote-hid.sh
# for the reader that gets to these substitutions first when the word is inside
# `" … "`.
#
# Verified against bash 5.2.37.

a=(A B C D E)
z=abcdef

echo "=== bare, not inside double quotes ==="
echo 1 [${a['$(echo 1)']}]
echo "2 rc=$?"
echo 3 [${z:'$(echo 1)':2}]
echo "4 rc=$?"

echo "=== inside double quotes ==="
echo "5 [${a['$(echo 1)']}]"
echo "6 rc=$?"

echo "=== the sub really runs (side effect) ==="
echo 7 [${a['$(echo 1 >&2; echo 1)']}]
echo "8 rc=$?"

echo "=== a plain single-quoted digit ==="
echo 9 [${a['1']}]
echo "10 rc=$?"

echo "=== two subs in one run ==="
echo 11 [${a['$(echo 1)$(echo 2)']}]
echo "12 rc=$?"

echo "=== a param in the run ==="
n=2
echo 13 [${a['$n']}]
echo "14 rc=$?"

echo "=== an empty run ==="
echo 15 [${a['']}]
echo "16 rc=$?"

echo "=== a run whose expansion is a valid whole expression ==="
echo 17 [${a[$(echo "'")1$(echo "'")]}]
echo "18 rc=$?"

echo "=== the interior follows double-quoting's rules ==="
echo 19 [${a['"1"']}]
echo "20 rc=$?"
echo 21 [${a['\1']}]
echo "22 rc=$?"
echo 23 [${a['\$n']}]
echo "24 rc=$?"
echo 25 [${a['"']}]
echo "26 rc=$?"
echo 27 [${a['`echo 1`']}]
echo "28 rc=$?"
echo 29 [${a['~']}]
echo "30 rc=$?"

echo "=== the run is only part of the subscript ==="
echo 31 [${a[1'x']}]
echo "32 rc=$?"
echo 33 [${a['0'+1]}]
echo "34 rc=$?"

echo "=== a body that will not parse reports at run time ==="
echo "35 [${a['$(fi)']}]"
echo "36 rc=$?"
echo 37 [${a['$(fi)']}]
echo "38 rc=$?"

echo "=== set +B does not change the operand's own read ==="
set +B
echo "39 [${a['$(fi)']}]"
echo "40 rc=$?"
echo 41 [${a['$(fi)']}]
echo "42 rc=$?"
set -B

echo "=== a substring bound reads the same way ==="
echo "43 [${z:'$(fi)':1}]"
echo "44 rc=$?"

echo "=== an assignment subscript ==="
a['$(fi)']=X
echo "45 rc=$?"

echo "=== an assoc key takes the *other* reading ==="
declare -A m=([k]=K)
echo "46 [${m['k']}]"
echo "47 [${m['$(echo k)']}]"
p=k
echo "48 [${m['$p']}]"
declare -A h=(['$n']=DOLLARN)
echo "49 [${h['$n']}]"
declare -A e
echo "50 [${e['$(fi)']}]"
echo "51 rc=$?"

echo "=== an indexed subscript given at run time ==="
s="a['\$(echo 1)']"
echo "52 [${!s}]"
echo "53 rc=$?"
unset "a['\$(echo 1)']"
echo "54 rc=$?"
printf -v "a['\$(echo 1)']" X
echo "55 rc=$?"
echo hi | read "a['\$(echo 1)']"
echo "56 rc=$?"

echo "=== an assoc key given at run time keeps the string reading ==="
unset "m['k']"
echo "57 rc=$? m=[${m[*]}]"
declare -A m2=([k]=K)
unset "m2['\$(echo k)']"
echo "58 rc=$? m2=[${m2[*]}]"

echo "=== declare's own operand ==="
declare -a w
w['$(echo 2)']=W
echo "59 rc=$? w=[${w[*]}] n2=[${w[2]}]"
declare -a q
q['1'+1]=Q
echo "60 rc=$?"

echo "=== under @P ==="
b='A${a['"'"'$(echo 1)'"'"']}B'
echo "61 [${b@P}]"
echo "62 rc=$?"
c='A${a['"'"'$(fi)'"'"']}B'
echo "63 [${c@P}]"
echo "64 rc=$?"

echo TAIL
