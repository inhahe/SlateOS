# The token that follows the last command of a `$( … )` body is the
# substitution's own `)`, because bash reads the body in the enclosing token
# stream. That `)` closes the body's list — a body needs no terminator of its
# own — but it is not a *list terminator*, and the productions that let `!` and
# `time` stand as whole commands need one. So `$( ! )` is a syntax error in the
# very place a bare `!` on a line of its own is a valid (false) command.
#
# Each rejected probe runs inside `( eval … )`: a `$( )` parse error is fatal to
# bash's caller, so the subshell is what absorbs it — see
# tests/corpus/cmdsub-error-fatality.sh, which is where that unwind is measured.
# Here only the diagnostic matters, so the status is not printed.

echo "=== a prefix with no pipeline has no terminator to stand on"
( eval 'echo A$( ! )B' )
( eval 'echo A$(!)B' )
( eval 'echo A$( ! ! )B' )
( eval 'echo A$(time)B' )
( eval 'echo A$( time -p )B' )
( eval 'echo A$( echo x; ! )B' )

echo "=== and the closing paren is what a body that stops mid-construct names"
# Ended by a newline these would name `newline'; ended by the `)' they name it.
( eval 'echo A$(for)B' )
( eval 'echo A$(case)B' )
( eval 'echo A$(if)B' )
( eval 'echo A$(while)B' )
( eval 'echo A$(a &&)B' )
# ... while a body whose *own* tokens are wrong still names those.
( eval 'echo A$(do)B' )
( eval 'echo A$( ! & )B' )

echo "=== a complete body needs no terminator, and tolerates one"
echo "A$(echo x)B"
echo "A$(echo x; )B"
echo "A$(echo x
)B"
echo "A$( )B"

echo "=== the prefix stands fine when a command follows it"
echo "A$( ! true )B"
echo "A$( !; echo x )B"
echo "A$( !
echo x )B"

echo "=== ... or when something other than that paren closes the list"
echo "A$( { !; } )B"
echo "A$( if !; then echo y; fi )B"
echo "A$( for i in 1; do !; done )B"
echo "A$( while !; do break; done )B"

echo "=== a process substitution body is closed by its paren the same way"
( eval 'cat <( ! )' )
( eval 'cat <( time )' )
( eval 'cat <( echo a; ! )' )
( eval 'cat <(for)' )
cat <(echo x)
cat <( echo x; )
cat <( !; echo x )
cat < <(echo z)

echo "=== a body written across lines is blamed on the line it stopped on"
# Here the token after `for' is a real newline, not the paren — so the error
# names `newline', on the body's own line counted in the enclosing file. The
# sourced file goes inside a subshell for the same reason the evals above do.
cat > inner.sh <<'INNER'
echo inner-one
cat <(
for
)
echo NOT-REACHED
INNER
( . ./inner.sh )

echo "=== a backtick body really is read on its own, so a line end ends it"
echo A`!`B
echo A`echo x`B
