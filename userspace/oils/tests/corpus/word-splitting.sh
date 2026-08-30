# Unquoted expansion is split on IFS and the empty result disappears entirely;
# quoted expansion is one field even when empty. `$*` joins on IFS[0], `$@`
# does not. The `printf '<%s>'` framing makes field boundaries visible.
set -- a 'b c' '' d
printf '<%s>' "$@"; echo
printf '<%s>' $@; echo
printf '<%s>' "$*"; echo

IFS=:
printf '<%s>' "$*"; echo
v='x:y::z'
printf '<%s>' $v; echo
IFS=$' \t\n'

empty=
printf 'count=%s\n' $#
set -- $empty
printf 'after-empty=%s\n' $#

# A lone unquoted empty expansion is removed; a quoted one is a real argument.
set -- "$empty"
printf 'quoted-empty=%s\n' $#

# Splitting applies to command substitution too, and trailing newlines go.
sub=$(printf 'p\nq\n\n\n')
printf '<%s>' $sub; echo
printf '<%s>' "$sub"; echo
