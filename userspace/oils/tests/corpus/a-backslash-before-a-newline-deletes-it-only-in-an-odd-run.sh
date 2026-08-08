# A `\` written flush against a newline is a line continuation only when the
# *run* of backslashes it belongs to is odd.
#
# `shell_getc` deletes `\<newline>` on sight, but `read_token_word` never lets it
# see the character after a backslash: it reads that one with `shell_getc (0)` —
# continuation removal off (parse.y). So a run inside a word is consumed in
# pairs, each pair yielding one literal backslash, and only an unpaired last
# backslash is still there to join with the newline. Two backslashes before a
# newline are therefore one quoted backslash *and* an ordinary newline; three are
# one quoted backslash and a continuation.
#
# The difference is a line, because deleting a continuation is a fetch and bash
# pays `line_number++` for it (parse.y:2361) while an ordinary newline costs
# nothing until the fetch that follows. It shows on the line bash stamps on a
# simple command, which it takes at the reduction of the command's first element
# — and that reduction waits for one more token, so for a one-word command the
# lookahead is the newline itself and the parity decides its line.
#
# A DEBUG trap is the only way to read that number back without running the
# command, since a word ending in a backslash names nothing that exists. Each
# probe is sourced in a subshell so that its trap, and the stderr it closes off
# for the same reason, are its own.
#
# Verified against bash 5.2.37.

mk() { printf '%s' "$2" > "$1"; }
head='trap '"'"'echo "  [$LINENO]"'"'"' DEBUG
exec 2>/dev/null
'

echo "=== a one-word command: the newline is its lookahead"
mk a1 "$head"'nosuch\
echo done
'
( . ./a1 )
mk a2 "$head"'nosuch\\
echo done
'
( . ./a2 )
mk a3 "$head"'nosuch\\\
echo done
'
( . ./a3 )
mk a4 "$head"'nosuch\\\\
echo done
'
( . ./a4 )

echo "=== a second word takes the lookahead's place, so the parity stops mattering"
mk b1 "$head"'nosuch A\\
echo done
'
( . ./b1 )
mk b2 "$head"'nosuch A\\\
echo done
'
( . ./b2 )

echo "=== the same parity in the middle of a script"
mk c1 "$head"'echo one
nosuch\\
echo two
'
( . ./c1 )

echo "=== an assignment reduces on sight, so it never sees the newline at all"
mk d1 "$head"'v=1\\
echo done
'
( . ./d1 )
# The odd run joins the two lines, and the assignment is still numbered by its
# own extent — which now ends on the second of them.
mk d2 "$head"'v=1\
2 echo done
'
( . ./d2 )

echo "=== a here-document delimiter reads its run the same way"
cat <<E\\
one
E\
echo "$LINENO"
cat <<E\\\\
two
E\\
echo "$LINENO"
