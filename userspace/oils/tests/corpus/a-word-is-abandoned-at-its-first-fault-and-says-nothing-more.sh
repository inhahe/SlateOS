# `call_expand_word_internal` propagates the first expansion failure straight out
# of the word list, so a word that faults is abandoned exactly where it stood:
# nothing later in it is expanded, no operator is applied to it, and no second
# diagnostic comes out. Only one complaint ever comes from one command.
#
# A bad *subscript* is the case that shows it, because the reference it belongs
# to is unset by construction and would otherwise have a `set -u` complaint of
# its own to add. `${nope[0+]}` under `set -u` reports the arithmetic syntax
# error at `0+` and stops there — no `nope[0+]: unbound variable` behind it. The
# same holds when the subscript's fault is itself a nounset one (`${nope[n]}`
# with `n` unset reports only `n`), and it holds through every operator: `:?`
# does not raise its message, `:=` does not evaluate the subscript a second time
# to assign through it, and a `$( … )` later in the word never runs.
#
# `${!name[sub]}` is the exception that proves the rule: the subscript is
# evaluated *first*, before bash discovers there is no pointer to indirect
# through, so its side effects happen and its faults are reported in place of
# `invalid indirect expansion`. Arithmetically, too — with no variable in hand
# there is nothing to say the subscript is an associative key.
#
# The subscripted *length* is a separate machine again (`array_length_reference`,
# `subst.c:7213`), and has two rules of its own:
#
#   if ((var == 0 || invisible_p (var) ||
#        (assoc_p (var) == 0 && array_p (var) == 0)) && unbound_vars_is_error)
#     { set_exit_status (EXECUTION_FAILURE); err_unboundvar (s); return (-1); }
#   else if (var == 0 || invisible_p (var))
#     return 0;
#
# Under `set -u` it faults for anything with no array shape to ask about — a bare
# `declare -a q`, an `export E`, a name that does not exist, and a perfectly good
# scalar — naming the base alone, ahead of any complaint the subscript would have
# earned. And whether `set -u` is on or not, an *invisible* name answers 0
# without the subscript ever being evaluated, which is why
# `declare -a q; echo ${#q[0+]}` prints 0 where the visible `q=(); echo ${#q[0+]}`
# reports the syntax error.
#
# Every probe below runs inside its own `( … )` so that the abort it raises stops
# that subshell rather than this script; the status printed after each is that
# subshell's. Which *status* a fatal abort carries where nothing caught it is a
# separate question — see
# `the-status-of-a-fatal-expansion-abort-belongs-to-whoever-caught-it`.

# TIMEOUT: 60

p() { printf '%-40s ' "$1"; ( eval "$1"; echo "in=$?" ) 2>&1; echo "  sub=$?"; }

echo '=== the first fault is the only one'
set -u
p 'echo "${nope[0+]}"'
p 'echo "${nope[n]}"'
p 'n=; echo "${nope[$n]}"'
p 'declare -a x=(a b); echo "${x[0+]}"'
p 'declare -A m=([k]=v); echo "${m[0+]}"'
p 'declare -a x=(a b); echo "${x[9]}"'
p 'unset -v x; echo "${x[0+]}"'
p 'echo "${nope[@]}"'
set +u
p 'echo "${nope[0+]}"'

echo '=== no operator is applied to a word that already faulted'
set -u
p 'echo "${nope[0+]:?boom}"'
p 'echo "${nope[0+]:-d}"'
p 'echo "${nope[0+]:+d}"'
p 'echo "${nope[0+]:=d}"'
p 'echo "${nope[0+]^^}"'
p 'echo "${nope[0+]#x}"'
p 'echo "${nope[0+]@Q}"'

echo '=== and nothing later in the word is expanded either'
p 'echo "${nope[0+]}$(echo ran >&2)"'
p 'echo "$nope$(echo ran >&2)"'
set +u

echo '=== an indirection evaluates its subscript before it misses its pointer'
set -u
p 'echo "${!nope[0+]}"'
p 'echo "${!nope[n]}"'
set +u
p 'echo "${!nope[$(echo ran >&2; echo 0)]}"'
p 'echo "${!nope[0+]}"'
p 'echo "${!nope[0]}"'

echo '=== a subscripted length asks about the shape, and skips the subscript'
declare -a q
declare -A mm
export E
s=hi
qq=(a b c)
declare -A mv=([k]=vvv)
e=(a); unset 'e[0]'
for r in q mm E s nope qq mv e; do
  for sub in '0+' 0 '@' '*' -5; do
    p 'echo "[${#'"$r"'['"$sub"']}]"'
  done
done

echo '=== …and under set -u it faults for anything shapeless, base name only'
set -u
for r in q mm E s nope qq mv e; do
  for sub in '0+' 0 '@' -5; do
    p 'echo "[${#'"$r"'['"$sub"']}]"'
  done
done
set +u

echo '=== done'
