# A here-document declared inside a `$( … )` or `<( … )` takes its body from the
# *enclosing* input, not from the substitution's own text. So the body lines are
# whatever follows the operator's line in the file — and they can run straight
# past the `)`, in which case the `)` is body text and the substitution has to
# close on a later one (or never).
#
# That is why the shell cannot find the end of a substitution by counting
# parentheses alone, and why it has to recognise every place a `<<` can sit
# *without* being an operator: a comment, a `<<<` here-string, a shift inside
# `$(( … ))`, a `${ … }` body, a backtick body, a backslash escape. Each of those
# gets a case below, because getting one wrong sends the scan hunting for a
# delimiter that never arrives and swallows the rest of the script.
#
# The cases that end without ever closing the substitution need an input of their
# own that *ends*, so they are `eval`s.

echo "=== the common idiom: the close paren follows the delimiter line ==="
x=$(cat <<EOF
body
EOF
)
echo "x=[$x]"

echo "=== a close paren inside the body is body text ==="
# The substitution closes on the *second* `)`, and the first one is output.
x=$(cat <<EOF
body
); echo hi
EOF
)
echo "x=[$x]"

echo "=== <<- and a quoted delimiter behave the same ==="
x=$(cat <<-"EOF"
	)
	EOF
)
echo "x=[$x]"

echo "=== two here-documents, bodies collected in order ==="
x=$(cat <<A <<B
)
A
)
B
)
echo "x=[$x]"

echo "=== a here-document outside the substitution keeps its place in line ==="
# B belongs to the substitution and is collected during the scan of it; A and C
# belong to the enclosing line and are collected after, at its newline. So B's
# body is `inner` and not `one` — the order is observable.
echo "arg=[$(cat <<B
inner
B
)]" <<A <<C
one
A
two
C
echo "outer rc=$?"

echo "=== the places a << is not an operator ==="
x=$(cat <<<')'
)
echo "herestring x=[$x]"
x=$(echo hi # <<Z
)
echo "comment x=[$x]"
x=$(echo $((1 << 3))
)
echo "shift x=[$x]"
y=A
x=$(echo "${y:-<<Z}"
)
echo "brace x=[$x]"
x=$(echo \))
echo "escape x=[$x]"

echo "=== a backtick body is lexed on its own ==="
# So the here-document inside one is that body's business — it is unterminated
# *there*, which warns, and the closing backtick is still found lexically.
x=$(echo `cat <<Z` hi
) 2>&1
echo "backtick x=[$x]"

echo "=== process substitution reads its body the same way ==="
cat <(cat <<EOF
)
EOF
)
echo "procsub rc=$?"

echo "=== a body that reaches end of input leaves the substitution open ==="
# Two diagnostics, in this order: the reader's warning about the here-document,
# then the parser's complaint about the `)` it never found. The `)` is reported
# one line past the last, as it is for every unterminated `$( … )`.
eval 'x=$(cat <<EOF
body
); echo hi' 2>&1
echo "open rc=$?"
# Earlier lines have already run by then.
( eval 'echo one
x=$(cat <<EOF
body
); echo hi' ) 2>&1
echo "after-line rc=$?"
# A process substitution is no different.
( eval 'cat <(cat <<EOF
body
); echo hi' ) 2>&1
echo "procsub-open rc=$?"
# Terminated, for contrast: no warning, no error.
eval 'x=$(cat <<EOF
body
EOF
); echo "x=[$x]"' 2>&1
echo "closed rc=$?"

echo "=== …including when the <<'s own line is the last line there is ==="
# The reader takes a whole line at a time, so the input's final line *ends* even
# without a newline of its own — the here-document it declared is gathered, and
# warned about, right there. Both of the warning's numbers are that line, and it
# still precedes the `)` complaint on the line past it.
( eval 'echo $(cat <<E' ) 2>&1
echo "last-line rc=$?"
( eval 'echo one
echo $(cat <<E' ) 2>&1
echo "last-line-after rc=$?"
# One warning each, in declaration order.
( eval 'echo $(cat <<A; cat <<B' ) 2>&1
echo "two rc=$?"
# The enclosing line's own here-document is not the substitution's to gather:
# only `B` is warned about, `A` going down with the rest of the abandoned parse.
( eval 'cat <<A $(cat <<B' ) 2>&1
echo "outer rc=$?"
( eval 'echo <(cat <<E' ) 2>&1
echo "procsub-last-line rc=$?"

echo done
