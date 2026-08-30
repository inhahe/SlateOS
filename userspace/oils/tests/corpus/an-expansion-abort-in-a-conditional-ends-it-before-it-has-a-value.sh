# `[[ … ]]` expands the operands of each test as it reaches them, and a fatal
# expansion error inside one never comes back: `cond_expand_word` calls
# `expand_word_unsplit`/`expand_word_leave_quoted`, and those `jump_to_top_level`
# straight out of `execute_cond_command`. The consequence is not "the test was
# false" — the test was never *decided*:
#
#   * the `!` in front of it is never applied, so an aborted `[[ ! … ]]` is
#     still the abort's own status, not its negation;
#   * a `=~` whose right-hand side aborted never reaches `regcomp`, so it is
#     not the 2 that an unparseable pattern would give;
#   * the operands after the failing one are never expanded at all, so a second
#     bad one in the same test — or in the other arm of an `&&`/`||` — is
#     silent.
#
# The two depths unwind as they do everywhere else: a *discard* (a bad array
# subscript in a substitution) drops one parse unit and leaves `$?` at 1 with
# the shell reading on, while a `${x:?word}` or a nounset reference under
# `set -u` ends the shell with the status it carried.

echo "=== an aborted test is not a false test, so ! does not flip it"
( [[ ${nope?bad} == x ]] )
echo "plain rc=$?"
( [[ ! ${nope?bad} == x ]] )
echo "negated rc=$?"
( [[ x == ${nope?bad} ]] )
echo "rhs rc=$?"
( [[ ! x == ${nope?bad} ]] )
echo "rhs negated rc=$?"

echo "=== a unary aborts the same way"
( [[ -n ${nope?bad} ]] )
echo "rc=$?"
( [[ ! -n ${nope?bad} ]] )
echo "negated rc=$?"

echo "=== and a =~ never reaches regcomp, so it is not a bad-pattern 2"
( [[ x =~ ${nope?bad} ]] )
echo "rc=$?"
( [[ x =~ *bad ]] )
echo "bad pattern rc=$?"

echo "=== the operand after the failing one is not expanded"
( [[ ${nope?bad} == ${also?worse} ]] )
echo "rc=$?"
( [[ ${nope?bad} == x || ${also?worse} == y ]] )
echo "or rc=$?"
( [[ ${nope?bad} == x && ${also?worse} == y ]] )
echo "and rc=$?"

echo "=== set -u reaches it too"
( set -u; [[ $nope == x ]]; echo "not reached" )
echo "rc=$?"
( set -u; [[ ! $nope == x ]]; echo "not reached" )
echo "negated rc=$?"

echo "=== a discard drops the parse unit instead, and the shell reads on"
[[ ${b[0=1]} == x ]]; echo "not reached"
echo "after rc=$?"
[[ ! ${c[0=1]} == x ]]; echo "not reached"
echo "after negated rc=$?"
[[ ${d[0=1]} == x || ${e[0=1]} == y ]]; echo "not reached"
echo "after or rc=$?"

echo "=== an operand that expands cleanly still decides the test"
n=x
[[ $n == x ]]
echo "true rc=$?"
[[ ! $n == x ]]
echo "negated rc=$?"
[[ $n =~ ^${n}$ ]]
echo "regex rc=$?"
