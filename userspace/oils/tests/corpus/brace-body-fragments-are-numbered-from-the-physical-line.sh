# A `${ … }` body is kept as text and lexed again when it is parsed, and that
# second lex numbers its lines from 1. bash has no such second coordinate
# system: `parse_matched_pair` copies the body out of the *one* stream it is
# reading, and `line_number` has already advanced over every newline in it — so
# what bash reports, and echoes, is the physical line.
#
# The body is then carved into fragments — a subscript, a slice's offset and
# length, a replacement's pattern and half — each lexed on its own. A fragment's
# own line 1 is the physical line it *starts* on, which is the body's opening
# line plus the newlines of the body before it. Most fragments are reached by
# stepping over operator characters only (`:`, `#`, `%`, `^`, `,`, `~`, `/`,
# `-`, `=`, `+`, `?`), none of which is a newline; exactly three can be pushed
# further down, and each is exercised below.
#
# The line matters twice over: it is what `line N:` prints, and it is what the
# echoed source line is looked up by — so a fragment numbered in the wrong
# system prints a wrong number *and* echoes the wrong line, or none at all.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== the body's own line, however far down the expansion sits ==="
e 'echo ${x:-$(fi)}'
e '
echo ${x:-$(fi)}'
e '


echo ${x:-$(fi)}'

echo "=== a body spanning lines is blamed on the line inside it ==="
e 'echo ${x:-$(
fi
)}'
e '

echo ${x:-$(
fi
)}'
e 'echo ${x:-$(echo a
fi
)}'

echo "=== past a multi-line subscript ==="
e '
echo ${a[1+
2]#$(fi)}'

echo "=== past a slice offset ==="
e '
echo ${x:$(echo 0
):$(fi)}'

echo "=== past a replacement pattern ==="
e '
echo ${x/aaa
bbb/$(fi)}'
e '
echo ${x//aaa
bbb
ccc/$(fi)}'
e '
echo ${a[@]/aaa
bbb/$(fi)}'
e '
echo ${a[@]#$(
fi
)}'
# Indirection re-parses the modifier against the referent, and the fragment's
# line has to survive that round trip.
e '
echo ${!r/aaa
bbb/$(fi)}'

echo "=== a nested \${ } counts from where it opens, not from its host ==="
e '
echo ${x:-${y:-$(
fi
)}}'

echo "=== the line is the body's, so a runtime failure names it too ==="
# Not a parse error: the backtick parses, and only fails when the expansion
# runs. Its line comes from what the *lexer* recorded, not from the error.
v=zaaaz
e '
echo ${v/aaa/`fi`}'
e '
echo ${v/aaa
bbb/`fi`}'

echo "=== a well-formed body is unaffected ==="
x=hello
a=(one two three)
echo "op=[${x:-$(echo d)}]"
echo "sub=[${x:$(echo 1):$(echo 3)}]"
echo "rep=[${x/ell/$(echo ELL)}]"
echo "multi=[${x/e
ll/Q}]"
echo "bulk=[${a[@]/o/$(echo 0)}]"
echo "trim=[${x#$(echo he)}]"

echo done
