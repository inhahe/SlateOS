# `$(( … ))` is settled in two steps, and they disagree often enough that both
# have to be modelled.
#
# The first only finds the `)`. Seeing a second `(`, bash's `parse_comsub` hands
# the text straight to `parse_matched_pair` with `P_ARITH` (parse.y:4103) — a
# character scan, never a parse. So the body is not read as commands: a `<<` in
# it declares nothing, a `#` starts no comment, and an input that runs out
# inside it is only ever the missing `)`.
#
# The second step is `chk_arithsub` (subst.c:9487), and in bash it does not
# happen until expansion: the parens come off and what is left must balance, or
# the whole thing is run as a command substitution instead. The two steps skip
# *different* things — a backtick is opaque to the scan and not to the check —
# so a `)` inside one can give the scan its `))` and still send the result to
# the shell.
#
# The cases that end without ever closing the construct need an input of their
# own that ends, so they are `eval`s.
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== a ) the scan steps over closes nothing ==="
# Each of these reaches its `))` only because the construct around the `)` is
# opaque; the expression bash then evaluates still has the `)` in it.
e 'echo $(( 1 + "2)3" - 2 ))'
e "echo \$(( 1 + '2)3' ))"
e 'echo $(( 1 \) ))'
e 'echo $(( $(echo ")") ))'
# The same set, and the same reason, for the deprecated `$[ … ]` spelling.
e 'echo $[ 1 + "2)3" - 2 ]'

echo "=== but a \${ … } is not one of them ==="
# `parse_matched_pair` skips a `\${` under `P_ARRAYSUB|P_DOLBRACE` only
# (parse.y:3929), and the arithmetic scan carries neither — so the `)` inside
# ends the expansion and the `}` is left behind as ordinary text.
e 'echo $(( 1 + ${x:-)} ))'
e 'echo $(( ${'
# With no closing delimiter inside it there is nothing to notice.
v=5
echo "brace=[$(( ${v} + 1 ))]"
echo "len=[$(( ${#v} ))]"
# A `$( … )`, by contrast, *is* parsed, so a `)` in a `${ … }` there is safe.
echo "cmdsub=[$(echo ${x:-)})]"

echo "=== the scan skips a backtick; the check does not ==="
# So this one is scanned as arithmetic and then run as a command substitution —
# which is why `1` is looked up as a command.
e 'echo $(( 1 + `echo 2)3` ))'

echo "=== a body that does not balance is a command substitution ==="
e 'echo $(( echo a ) )'
e 'echo $(( echo hi ) | cat )'
# Reaching `))` keeps it arithmetic, and it fails as arithmetic.
e 'echo $(( echo a ))'
e 'echo $(( 1 + 1 ))'

echo "=== and one bash does not read until the word is expanded ==="
# `chk_arithsub` fails at *expansion* time, and what bash reaches for then is
# `command_substitute` — the very call a backtick body makes. So the body is
# read then and not before: a branch never taken never reads it, exactly as for
# a backtick and unlike `$( … )`, whose eager parse kills the whole unit.
e 'if false; then echo $(( fi ) ); fi; echo ok'
e 'if false; then echo `fi`; fi; echo ok'
e 'if false; then echo $(fi); fi; echo ok'
# Read at last, it reports as the substitution it is and leaves the enclosing
# command running — `AB`, not nothing.
e 'echo A$(( fi ) )B'
# `declare -f` prints back the spelling that was written, so the fallback needs
# a form of its own rather than a backtick's.
f() { echo $(( echo hi ) ) X$(( 1 + 1 ))Y; }; declare -f f
# `$[ … ]` has no fallback at all: it is arithmetic or it is an error.
e 'echo $[ fi ]'
e 'echo $[ ( echo a ) ]'

echo "=== running out of input inside one is only ever the missing ) ==="
# The body is never parsed as commands, so nothing in it can object first.
e 'echo $(( fi'
e 'echo $(( ]]'
e 'echo $(( )'
e 'echo $(('
e 'echo $(( 1'
e 'x=$(('
e 'echo a$(( b'
e 'echo $(( echo a )'
e 'echo $(( echo a ) | cat'
# Blamed on the line the `$((` is on, not the end of input — the scan starts and
# fails in one place, with no re-parse afterwards to move the counter on.
e 'echo one
echo $(( fi'
e 'echo $((
foo'

echo "=== …unless an inner construct ran out first ==="
# Then it is that construct's delimiter that is named, not the `)`.
e 'echo $(( "'
e 'echo $(( `'
e "echo \$((  '"

echo "=== a nested \$( … ) is a parse of its own even in here ==="
# `P_ARITH` reaches `parse_comsub` too (parse.y:3927), so the body is read as
# commands after all — and its own error wins over the missing `)` outside it.
e 'echo $(( $(fi'
e 'echo $[ $(fi'
e '(( $(fi ))'
e 'echo $(( $(echo a'

echo "=== here-documents: the scan declares none, the nested parse does ==="
# The `$((` body is not commands, so `E` is still pending when the input ends
# and is warned about.
e 'cat <<E $(('
e 'cat <<E $(( 1 +'
e 'cat <<E $(( echo a )'
e 'cat <<E $(( cat <<F'
# A nested `$(` *is* commands: `F` is gathered and warned about, while `E` goes
# down with the parse the substitution abandoned.
e 'cat <<E $(( $(cat <<F'
# For contrast, the plain substitution, which forgets `E` the same way.
e 'cat <<E $('
# A well-formed one collects its body from the enclosing input as usual.
echo "heredoc=[$(cat <<E
body
E
)]"

echo done
