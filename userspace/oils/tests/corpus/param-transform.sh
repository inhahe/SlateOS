# `${var@OP}` parameter transformations, and the `${var,,}` / `${var^^}` case
# operators. These are the operators that reformat a value rather than select
# part of it.

v='a b  c'
q="it's \"quoted\" \\ and \$dollar"
empty=''
unset -v missing

# @Q re-quotes the value so it can be fed back to the shell verbatim.
echo "${v@Q}"
echo "${q@Q}"
echo "${empty@Q}"

# @E expands backslash escapes the way $'…' does.
esc='tab\there\nnewline'
echo "${esc@E}"

# @U / @L / @u uppercase all, lowercase all, uppercase first.
mixed='hello World'
echo "${mixed@U}"
echo "${mixed@L}"
echo "${mixed@u}"

# The older ^ / , operators do the same job with a pattern selecting which
# characters are affected.
echo "${mixed^}"
echo "${mixed^^}"
echo "${mixed,}"
echo "${mixed,,}"
echo "${mixed^^[aeiou]}"
echo "${mixed,,[A-Z]}"

# @a lists the attribute letters of the variable.
declare -i num=5
declare -r ro=frozen
declare -a arr=(x y)
declare -A assoc=([k]=v)
declare -x exported=e
echo "num=${num@a} ro=${ro@a} arr=${arr@a} assoc=${assoc@a} exported=${exported@a} plain=${v@a}"

# @A prints an assignment statement that would recreate the variable.
echo "${v@A}"
echo "${num@A}"
echo "${arr@A}"

# @k / @K on an array: @K quotes the whole key/value list as one word.
echo "${arr[@]@Q}"
echo "${arr[@]@A}"
echo "[${arr[@]@K}][${arr[@]@k}]"
set -- p 'q r'
echo "[${@@K}][${@@k}]"

# @K is meant to read back as shell input, so it quotes a *key* that would not
# — to exactly the degree `declare -p` does. @k quotes neither key nor value.
# (One key per array: bash walks its own hash order, which osh does not share.)
k() { unset -v m; declare -A m; m[$1]=v; echo "K=[${m[@]@K}] k=[${m[@]@k}]"; }
k 'a b'
k 'd"e'
k "it's"
k 'x$y'
k 'p*q'
k '~t'
k '#c'
k $'f\tg'
k 'a=b'
k plain

# Transformations apply element-wise across "${arr[@]}" but produce a single
# word for "${arr[*]}".
up=(one two)
echo "${up[@]@U}"
echo "${up[*]@U}"

# An unset variable transforms to nothing (and is not an error without -u).
echo "missing-Q=[${missing@Q}] missing-U=[${missing@U}] missing-a=[${missing@a}]"

# Transformations compose with the surrounding word, and the result is subject
# to word splitting when unquoted.
count() { echo "argc=$#"; }
count ${v@Q}
count "${v@Q}"

# @Q of a value containing a newline uses $'…' form.
nl=$'x\ny'
echo "${nl@Q}"

# An unknown operator is an error.
echo "${v@z}" 2>/dev/null; echo "bad-op-status=$?"
