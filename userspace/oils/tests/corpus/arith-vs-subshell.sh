# `$((` opens two different constructs. Usually it is arithmetic, but it is also
# how a command substitution whose body *starts with a subshell* is spelled:
# `$( ( … ) | … )` has no space to force, so it is written `$(( … ) | … )`. No
# amount of lookahead separates them — `$(( 1 + 2 ))` and `$(( echo a ) | cat)`
# agree for as long as you care to look.
#
# So the reading commits to arithmetic and rewinds to a substitution only when
# the arithmetic scan fails to reach a `))`. That makes the *whole text* the
# decider, which has two consequences worth pinning: something that looks like
# commands but does reach `))` stays arithmetic (and fails as arithmetic), and
# the rewind must leave the lexer able to read the rest of the line.

echo "=== arithmetic, because the scan reaches its ')' ')' ==="
echo $((1 + 2))
echo $(((1 + 2) * 3))
echo $(( (1 + 2) * (3 + 4) ))
x=5
echo $(( x > 1 ? x : -x ))

echo "=== a body that opens with a subshell ==="
x=$(( echo a; echo b ) | tr a-z A-Z)
echo "pipe x=[$x]"
x=$(( echo a ) && echo b)
echo "andand x=[$x]"
x=$(( echo a ); echo b)
echo "semi x=[$x]"
x=$(( echo a ); ( echo b ))
echo "two-groups x=[$x]"

echo "=== the rest of the line still lexes after the rewind ==="
echo "$(( echo a ) | cat) $((1 + 1))"
x=$(( echo "$(echo inner)" ) | cat)
echo "inner x=[$x]"
x=$(( echo ')' ) | cat)
echo "quoted-paren x=[$x]"

echo "=== reaching the ')' ')' is what settles it, not looking shell-like ==="
# `echo a` is not an expression, but the scan does reach the end, so this stays
# arithmetic and fails at evaluation — it never becomes a substitution.
( eval 'echo $(( echo a ))' ) 2>&1
echo "stays-arith rc=$?"
# One more paren and the scan runs off the end, so it rewinds and runs.
x=$(( echo a ) )
echo "one-more-paren x=[$x]"

echo "=== an unterminated one is blamed on its opening line ==="
# Unlike a plain `$( … )`, which is reported one line past the last: the
# arithmetic reading has already failed by then, and that failure is stamped
# where the `$((` is.
( eval 'echo one
x=$(( echo a ) | cat' ) 2>&1
echo "open rc=$?"

echo done
