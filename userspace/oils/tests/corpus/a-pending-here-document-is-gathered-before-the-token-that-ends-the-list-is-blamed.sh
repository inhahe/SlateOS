# A here-document's body is not read where the `<<` is written; it is read at
# the next newline. That is the gather everyone knows about, and it is not the
# only one. `gather_here_documents()` is called from three places in bash's
# parser, and the second of them is why a syntax error can be blamed on a line
# below the one it was written on:
#
#   * `read_token` (parse.y:3450), on the newline token — the ordinary case.
#   * the yacc action of `simple_list` (parse.y:1217, with the `&` and `;`
#     variants at 1235 and 1250):
#
#         simple_list:    simple_list1
#                         { ...  if (need_here_doc) gather_here_documents ();  ... }
#
#   * `compound_list` (1148), guarded by `last_read_token == '\n'`.
#
# The middle one fires on a *lookahead*. An LALR parser cannot reduce
# `simple_list1` to `simple_list` until it has seen the token after it, so a
# token that cannot continue the list — a `(`, a `;;`, an `fi` — is read, the
# reduction is taken, the pending here-document is gathered, and only *then* is
# the token found to be a syntax error. The body has already gone past the
# reader by the time the error is reported.
#
# How far past is exactly the physical lines the collection consumed.
# `make_here_document` (make_cmd.c:621) bumps `line_number` once per line
# `read_a_line` hands it — the delimiter's own line included — and `read_a_line`
# (parse.y:2080) bumps it again for each `\<newline>` it joins away inside an
# expanding body. The two together come to every line the body occupied.
#
# The echoed text does not move with the number. `read_a_line` reads bodies into
# a `line_buffer` of its own and never replaces `shell_input_line`, so the line
# bash quotes under the diagnostic is still the one the error was written on,
# while the number above it names a later line. Every case in the first half
# below prints that mismatch.
#
# And it is the *top-level* list only. The same action inside `compound_list` is
# guarded on the previous token having been a newline, so a `(` sitting on the
# same line as the `<<` inside a `{ }`, an `if`, a `( )`, a function body or a
# `do … done` reduces nothing, gathers nothing, and is blamed on its own line.
# That is the second half.
#
# Each probe runs under `eval` in a subshell — a syntax error otherwise abandons
# the rest of the input — which also puts the line numbers on a scale of their
# own, starting at the first line of the fragment.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== the body and its delimiter are read before the ( is looked at"
e 'cat <<E(
body
E
echo tail'

echo "=== …so a longer body pushes the blame further down"
e 'cat <<E(
b1
b2
b3
E
echo tail'

echo "=== and it is an offset, not an absolute line"
e 'echo one
cat <<E(
body
E'

echo "=== any lookahead the list cannot continue with does it"
e 'cat <<E;;
body
E'
e 'cat <<E; fi
body
E'
e 'cat <<E & fi
body
E'

echo "=== a here-document already gathered at a newline costs nothing"
# Only `F` is pending when the `(` arrives: `E` was taken by the newline that
# ended its own line, back at the first gather site.
e 'cat <<E
body
E
cat <<F(
b
F'
e 'cat <<E
body
E
cat <<F;;
b
F'

echo "=== two pending at once are gathered together"
e 'cat <<A <<B(
a1
A
b1
B'

echo "=== …but only the ones declared before the offending token are pending"
# `B` is never declared — the `(` is read first — so the advance is `A`'s body
# alone, and the same five lines of input are blamed two lines higher.
e 'cat <<A( <<B
a1
A
b1
B'

echo "=== a joined-away continuation still costs its line"
e 'cat <<E(
bo\
dy
E'

echo "=== an empty body costs only its delimiter"
e 'cat <<E(
E'

echo "=== blank lines in a body are lines like any other"
e 'cat <<E(

body

E'

echo "=== quoting the delimiter changes nothing; the count is of lines"
e 'cat <<"E"(
$x body
E'

echo "=== nor does <<-"
e 'cat <<-E(
	body
	E'

echo "=== a body cut off by end of input moves the reader by what it did read"
# The delimiter that never came costs nothing, and the warning about it is
# printed by the collection — so it appears before the syntax error, and under
# the script's own name rather than `eval`'s.
e 'cat <<E(
body'

echo "=== inside a compound nothing reduces, so nothing moves"
e '{ cat <<E(
body
E
}'
e 'if cat <<E(
body
E
fi'
e '( cat <<E(
body
E
)'
e 'f() { cat <<E(
body
E
}'
e 'for x in a; do cat <<E(
body
E
done'
e 'while cat <<E(
body
E
done'
e 'until cat <<E(
body
E
done'
# The `case` arm, restored now that a bare `(` after a command word is rejected
# where it stands there too (it used to be blamed on the `;;`, which moved this
# probe's expected line — see
# TD-OILS-A-STRAY-PAREN-IN-A-CASE-ARM-IS-NOT-DIAGNOSED-WHERE-IT-STANDS).
e 'case a in a) cat <<E(
body
E
;;
esac'
