# bash blames a syntax error on `line_number` — the last line its *reader* has
# fetched — and that is not always the line the offending token was written on.
# Having finished a token the reader has already looked at the character after
# it, if only to discover that the token ends there; and when that character is
# a `\<newline>`, `shell_getc` deletes it, which it does by bumping
# `line_number` and fetching the next line. So the error, and the source line
# echoed beneath it, land one line below the token:
#
#   echo a ;;\      line 2: syntax error near unexpected token `;;'
#   b               line 2: `b'
#
# Only a continuation written *flush* against the token does this. Put a space
# in between and the space is the character the reader looked at, so it never
# asks for another line and the error stays where the token is. A plain newline
# does not move it either: a newline is the last character of the line it
# terminates, and the fetch that would bump the counter has not happened yet.
#
# The move is per continuation, not once: three of them in a row carry the error
# three lines down. And the two numbers a `[[ … ]]` prints move together, since
# both are the reader's.
#
# Each case is sourced from a written file so the line numbers are the snippet's
# own rather than this file's.

l() { printf '%s\n' "$1" > sourced.sh; ( . ./sourced.sh ) 2>&1; rm -f sourced.sh; }

echo "=== a continuation flush after the token carries the error down"
l 'echo a ;;\
b'
l 'echo a )\
b'
l 'foo() ;\
x'
l 'if true; then :; fi ;;\
x'
l 'case x in esac ;;\
y'
l 'for i in a; do :; done )\
z'

echo "=== but only flush, and only a continuation"
l 'echo a ;; \
b'
l 'echo a ;;
b'
l 'echo a ;;	\
b'

echo "=== every continuation counts, and the run may be long"
l 'echo a ;\
;b'
l 'echo a ;;\
\
b'
l 'echo a ;;\
\
\
b'
l 'echo a ;;\
   b'

echo "=== wherever in the file the token is written"
l 'echo one
echo a ;;\
b'
l 'echo one
echo two
echo a )\
b'
l '{ echo a ;;\
b ; }'
l 'echo $( echo a ;;\
b )'

echo "=== a redirection operator is read before the reader stops"
# `>>` needs no test to tell it from anything, so the reader never looks past it
# and the error stays on line 1 — as it does when a space, rather than the
# continuation, is what it would have looked at. A *single* `>` does look, and
# so carries the error down; which operators do which is pinned by
# an-operator-crosses-a-continuation-only-if-it-looks-past-itself.sh.
l '[[ 2>>\
Q ]]'
l '[[ 2> \
Q ]]'
l '[[ 2\
>Q ]]'
l '[[ 2>Q\
R ]]'

echo "=== a conditional moves both of its lines together"
l '[[ P;\
Q ]]'
l '[[ a b;\
c ]]'
l 'echo one
[[ P;\
Q ]]'
echo done
