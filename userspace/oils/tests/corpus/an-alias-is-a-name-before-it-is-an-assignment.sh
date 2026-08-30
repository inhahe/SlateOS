# `alias name=value` looks like an assignment, and that is the trap: the name is
# not a variable name. It may start with a digit, hold a `-`, or carry what
# looks like a subscript — the only bytes refused are the ones that could not
# stay in one command word (`legal_alias_name` = the shell's break characters,
# its quoting characters, `$` and `/`). So `1a`, `a-b` and `a[0]` are all fine
# names and `a b` is not.
#
# The split is at the *first* `=`, and only counts when it leaves a name in
# front of it. That makes `a=b=c` a definition of `a` with the value `b=c`, but
# leaves `=v` and `==v` as *queries* — for aliases spelled `=v` and `==v`,
# which are names no definition could ever have created.
#
# `-p` is not "print instead of list": it prints the table and *then* still
# handles the operands. The one thing that cuts a call short is an empty table,
# which answers success before it looks at an operand at all.

shopt -s expand_aliases

echo "=== the name is not a variable name"
alias 1a=v 'a-b=v' 'a[0]=v' 'a=b=c' _x=v 'x!'=v; echo "  rc=$?"
alias
unalias -a

echo "=== the bytes a name may not hold"
for c in '`' "'" '"' '\' '$' '(' ')' '<' '>' ';' '&' '|' ' ' '/'; do
  alias "a${c}b=v" 2>/dev/null; echo "  refused rc=$?"
done
# and the ones it may
for c in '!' '^' '~' '#' '*' '?' '[' ']' '{' '}' ':' ',' '.' '+' '-' '@' '%'; do
  alias "a${c}b=v" || echo "  unexpectedly refused: $c"
done
alias
unalias -a

echo "=== a refused name is reported and the rest still land"
alias f1=1 'a b=2' f2=3 'c/d=4' f3=5; echo "  rc=$?"
alias
unalias -a

echo "=== a leading = is a query, not a nameless definition"
alias '=v'; echo "  rc=$?"
alias '=='; echo "  rc=$?"
alias '==v'; echo "  rc=$?"
alias '='; echo "  rc=$?"
alias

echo "=== the first = wins"
alias 'a=b=c'
alias a
unalias -a

echo "=== a name that starts with a dash prints behind a --"
alias -- -x=1 --y=2 -=3
alias
alias -- -x; echo "  rc=$?"
alias -p
unalias -a

echo "=== -p prints the table and then keeps going"
alias a=1
alias -p b=2; echo "  rc=$?"
alias
unalias -a

echo "=== an empty table ends the call before the operands"
alias -p nope; echo "  rc=$?"
alias nope; echo "  rc=$?"

echo "=== the options stop at the first operand"
alias c=3
alias c -p; echo "  rc=$?"
alias -pp; echo "  rc=$?"
alias -p -- c; echo "  rc=$?"
alias -q 2>&1 >/dev/null; echo "  rc=$?"
unalias -a

echo "=== a query reports each missing name and follows the operands"
alias m=1 n=2 o=3
alias o m n zz m; echo "  rc=$?"
unalias -a

echo "=== a name that only looks like an option"
alias -- -p=1
alias -p; echo "  rc=$?"
unalias -a

echo "=== the listing is sorted by bytes, not by definition order"
alias B=1 a=2 A=3 b=4 _=5 0=6 'z~'=7 zz=8
alias
unalias -a

echo "=== unalias refuses no name, it only fails to find one"
alias k=1
unalias 'b d'; echo "  rc=$?"
unalias x k y; echo "  rc=$?"
alias; echo "  after rc=$?"

echo "=== unalias -a clears everything, and after a name it is a name"
alias g=1 h=2
unalias -a g; echo "  rc=$?"; alias; echo "  after rc=$?"
alias g=1 h=2
unalias g -a; echo "  rc=$?"; alias; echo "  after rc=$?"
unalias -a

echo "=== the value is quoted so the line would re-enter it"
alias q1="it's here" q2='a b' q3='$x `y`' q4=''
alias
unalias -a

echo "=== BASH_ALIASES sees the same table"
alias z=1
echo "${BASH_ALIASES[z]}"
echo "${!BASH_ALIASES[@]}"
unalias -a
