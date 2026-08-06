# The compound-assignment machinery's `\`n[1]': not a valid identifier` is
# raised while the operand is being *expanded*, before the declaration builtin
# has become the command that is running — so the name it carries in front is
# not the builtin's. It is bash's `this_command_name`, which at that moment
# still belongs to whatever command is running *around* the expansion.
#
# bash sets that name from a simple command's first word only after the
# command's own words have been expanded, and on the way out clears it — to
# nothing, not back to what it was. So it is only ever visible inside a builtin
# that runs shell code (`eval`, `source`, `.`), and only until some sibling
# command has run and returned.
#
# Two compound commands set it and never put it back: `((` and `[[`. Their name
# therefore outlives them and prefixes the refusal of whatever follows. A `for`
# or `select` loop does the opposite, blanking it before each iteration. A
# group, a subshell, a pipeline and a command substitution neither set nor
# clear it, so they pass on what they were given. An assignment statement —
# bare or as a command's prefix — is applied with the name blanked and put back
# after, so a substitution in its value sees nothing.
#
# Inside a function none of this can be seen, because there the declaration
# binds a local named by the reference's spelling and complains about nothing.

R="n=(a b c); declare -n r='n[1]';"
W="n=(a b c); declare -n r='n[@]';"

echo '=== nothing is running around the top of a script'
( eval "$R"; declare r=(x y) )
( eval "$R"; typeset r=(x y) )
( eval "$R"; r=(x y) )
echo '=== eval is, for the first command of its string'
( eval "$R"; eval 'declare r=(x y)' )
( eval "$R"; eval 'eval "declare r=(x y)"' )
( eval "$R"; eval 'export r=(x y)' )
( eval "$R"; eval 'readonly r=(x y)' )
( eval "$R"; eval 'declare -a r=(x y)' )
( eval "$R"; eval 'declare -g r=(x y)' )
( eval "$R"; eval declare r=\(x y\) )
( eval "$R"; command eval 'declare r=(x y)' )
( eval "$R"; builtin eval 'declare r=(x y)' )
echo '=== …and only until something else has run and returned'
( eval "$R"; eval 'true; declare r=(x y)' )
( eval "$R"; eval ': ; declare r=(x y)' )
( eval "$R"; eval 'true && declare r=(x y)' )
( eval "$R"; eval 'if true; then declare r=(x y); fi' )
( eval "$R"; eval 'eval "true"; declare r=(x y)' )
( eval "$R"; eval 'declare -n q=zz; declare r=(x y)' )
echo '=== a plain assignment inside eval carries no name either'
( eval "$R"; eval 'r=(x y)' )
( eval "$R"; eval 'v=$(declare r=(x y))'; echo "s=$?" )
( eval "$R"; eval 'v=$(declare r=(x y)) true'; echo "s=$?" )
( eval "$R"; eval 'v=1 declare r=(x y)' )
echo '=== source and . answer with their own name'
printf 'declare r=(x y)\n' > inner.sh
printf 'true\ndeclare r=(x y)\n' > inner2.sh
( eval "$R"; source ./inner.sh )
( eval "$R"; . ./inner.sh )
( eval "$R"; builtin source ./inner.sh )
( eval "$R"; eval 'source ./inner.sh' )
( eval "$R"; source ./inner2.sh )
echo '=== a group, a subshell, a pipeline and a substitution pass it on'
( eval "$R"; eval '{ declare r=(x y); }' )
( eval "$R"; eval '(declare r=(x y))'; echo "s=$?" )
( eval "$R"; eval 'declare r=(x y) | cat'; echo "s=$?" )
( eval "$R"; eval 'echo "$(declare r=(x y))"'; echo "s=$?" )
( eval "$R"; echo "$(eval 'declare r=(x y)')"; echo "s=$?" )
( eval "$R"; eval '! declare r=(x y)' )
( eval "$R"; eval 'case a in a) declare r=(x y);; esac' )
echo '=== a loop body starts with none'
( eval "$R"; eval 'for i in 1; do declare r=(x y); done' )
( eval "$R"; eval 'while [ -z "" ]; do declare r=(x y); done' )
( eval "$R"; eval 'until [ -n "" ]; do declare r=(x y); done' )
echo '=== `((` and `[[` leave theirs behind'
( eval "$R"; eval 'for ((i=0;i<1;i++)); do declare r=(x y); done' )
( eval "$R"; eval '((1)); declare r=(x y)' )
( eval "$R"; eval '[[ 1 == 1 ]]; declare r=(x y)' )
( eval "$R"; eval 'if [[ 1 == 1 ]]; then declare r=(x y); fi' )
( eval "$R"; ((1)); declare r=(x y) )
( eval "$R"; [[ 1 == 1 ]]; declare r=(x y) )
( eval "$R"; ((1)); true; declare r=(x y) )
echo '=== the bad-subscript line a whole-array reference adds is never tagged'
( eval "$W"; declare r=(x y) )
( eval "$W"; eval 'declare r=(x y)' )
( eval "$W"; source ./inner.sh )
( eval "$W"; eval 'declare r'; echo "s=$?" )
echo '=== and inside a function nothing is refused at all'
( eval "$R"; f() { declare r=(x y); }; f; echo "s=$?" )
( eval "$R"; f() { eval 'declare r=(x y)'; }; f; echo "s=$?" )
( eval "$R"; f() { source ./inner.sh; }; f; echo "s=$?" )
( eval "$R"; f() { eval 'declare -g r=(x y)'; }; f; echo "s=$?"; declare -p 'n[1]'; declare -p n )
