# `[[ … ]]`: no word splitting or globbing on the RHS of a comparison, pattern
# matching with `==`, regex matching with `=~` (and BASH_REMATCH), and the
# short-circuiting of && / ||.
v='a b'
[[ $v == 'a b' ]] && echo unquoted-lhs-ok

# The RHS of ==/!= is a *pattern* unless quoted.
[[ abc == a* ]] && echo glob-rhs
[[ abc == 'a*' ]] || echo quoted-rhs-is-literal
[[ 'a*' == 'a*' ]] && echo literal-eq

# Character classes and extglob-style alternation inside [[ ]].
[[ abc == [ab]* ]] && echo class-ok
[[ file.txt == *.@(txt|log) ]] && echo extglob-in-dbl-bracket

# =~ takes an ERE; BASH_REMATCH holds the whole match then each group.
if [[ 2026-07-27 =~ ^([0-9]{4})-([0-9]{2})-([0-9]{2})$ ]]; then
  echo "re n=${#BASH_REMATCH[@]} all=${BASH_REMATCH[0]} y=${BASH_REMATCH[1]} d=${BASH_REMATCH[3]}"
fi
# A quoted RHS is a literal string, not a regex.
[[ 'a.c' =~ 'a.c' ]] && echo quoted-regex-literal
[[ abc =~ a.c ]] && echo unquoted-regex-meta
# A regex held in a variable keeps its metacharacters.
re='^[0-9]+$'
[[ 12345 =~ $re ]] && echo var-regex
[[ 12a45 =~ $re ]] || echo var-regex-nomatch

# String comparison is by the current collation; -z/-n test emptiness.
[[ abc < abd ]] && echo lt-ok
[[ -z '' && -n x ]] && echo zn-ok

# Arithmetic-flavoured operators exist here too.
[[ 5 -gt 3 && 2 -le 2 ]] && echo numeric-ok

# && / || short-circuit: the second operand must not run when it cannot matter.
f() { echo "ran-$1"; return "$2"; }
[[ $(f a 1) == ran-a ]] && echo lhs-evaluated
false || [[ 1 -eq 1 ]] && echo chained

# -v tests whether a variable is set (even to empty), -o a shell option.
unset novar
empty=
[[ -v empty ]] && echo v-set-empty
[[ -v novar ]] || echo v-unset

# File tests against real files.
touch present
[[ -e present && -f present && ! -d present ]] && echo file-tests
mkdir -p adir
[[ -d adir ]] && echo dir-test
[[ -e missing ]] || echo missing-test

# Negation and grouping.
[[ ! ( 1 -eq 2 || 3 -eq 4 ) ]] && echo negated-group
echo "final-status=$?"
