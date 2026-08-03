# An operator standing where an array-literal element belongs — `a=(x; y)` — is
# a syntax error, but not the usual kind. The usual kind abandons the rest of the
# input and exits 2; this one costs only the **parse unit** that holds it, scores
# `$?` 1, and the shell reads on.
#
# The difference is which part of bash caught it. A grammar error has no defined
# place to resume from. But an array literal is collected by the *reader*, which
# has already matched the `( … )` before anything looks inside — so there is an
# obvious place to pick back up: just past the closing paren. That is also why an
# *unterminated* literal (`a=(x`) is worth 1 here where every other unterminated
# construct (`echo "…`, `if …`, `$( …`, `( …`) is worth 2.
#
# What dies is the unit, not the line: `;` chains commands into one unit, so
# `echo one; a=(x; y)` never prints `one` — the whole unit is dropped before any
# of it runs. And because recovery resumes past the `)`, a literal spanning
# several lines is blamed on the line the operator is on, and only the lines the
# literal itself covers are lost.
#
# bash names the longest operator it finds (`;;` over `;`, `<<<` over `<<`), the
# same way the token reader does everywhere else.

echo "=== the unit is dropped and reading goes on"
a=(x; y)
echo "after=$?"

echo "=== the unit, not the line"
echo one; a=(p; q); echo mid
echo "next=$?"

echo "=== recovery resumes past the closing paren"
a=(x
y; z)
echo "after=$?"

echo "=== the longest operator is named"
a=(x;; y)
a=(x;& y)
a=(x& y)
a=(x&& y)
a=(x| y)
a=(x|| y)
a=(x|& y)
a=(x< y)
a=(x> y)
a=(x>> y)
a=(x<< y)
a=(x<<< y)
a=(x<& y)
a=(x( y)
echo "still here=$?"

echo "=== every context the literal can appear in"
declare -a d=(p; q)
b+=(p; q)
a=([k]=; )
if true; then a=(x; y); fi
f() { a=(x; y); }
a=(x; y) | cat
echo A && a=(x; y)
echo "alive=$?"

echo "=== a well-formed literal is untouched"
declare -a ok=(p q [5]=r)
declare -p ok

echo "=== a bad element outranks a missing closing paren"
# Neither of these ends the input: the element is blamed ahead of the absent
# `)`, and reading resumes just *past that element* rather than past a paren
# that never comes. So the `<<EOF` below is never taken as a here-document —
# its "body" is read as ordinary commands, which is why `echo body` runs.
a=(x; y
echo "after=$?"
a=(x <<EOF
echo body
EOF
echo "still here=$?"

# Truly unterminated is the same class (status 1, not a syntax error's 2), but
# it does end the input — so it has to come last, and the exit status of the
# whole script is its.
echo "=== an unterminated literal ends the input, worth 1"
a=(x
