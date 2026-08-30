# The `test` / `[` builtin. Unlike `[[ … ]]` this is an ordinary command, so its
# arguments are expanded and word-split first and the parse is decided purely by
# the *number* of arguments — the POSIX rules that make `test` full of traps.

r() { if test "$@"; then echo "yes($*)"; else echo "no($*) status=$?"; fi; }

# Argument-count rules. With 0 args false; 1 arg is a string-emptiness test; 2
# args is `! expr` or a unary operator; 3 args is a binary operator.
test; echo "zero-args=$?"
test ''; echo "one-empty=$?"
test x; echo "one-nonempty=$?"
test '!'; echo "one-bang=$?"          # 1 arg: the string "!" is non-empty -> true
test -n; echo "one-dash-n=$?"         # 1 arg: the string "-n" is non-empty -> true
test ! ''; echo "two-bang-empty=$?"
test ! x; echo "two-bang-x=$?"
test -n ''; echo "two-n-empty=$?"
test -z ''; echo "two-z-empty=$?"
test = =; echo "three-eq-eq=$?"       # 3 args: "=" = "=" -> true
test '(' ')'; echo "two-parens=$?"    # 2 args: not a group, "(" is not unary
test '(' x ')'; echo "three-group=$?" # 3 args: parenthesised 1-arg test

# String comparison. `=` and `==` both work in bash's test; `<` and `>` compare
# lexically and must be quoted or they redirect.
r abc = abc
r abc == abc
r abc != abd
r abc '<' abd
r abd '>' abc
r '' = ''

# Numeric comparison operators only accept integers.
r 5 -eq 5
r 5 -ne 4
r 5 -lt 6
r 6 -gt 5
r 5 -le 5
r 5 -ge 5
r ' 5 ' -eq 5
test 5x -eq 5 2>/dev/null; echo "noninteger-status=$?"

# `-a` / `-o` combine, with `!` binding tighter than `-a`, which binds tighter
# than `-o`.
r x = x -a y = y
r x = x -a y = z
r x = x -o y = z
r ! x = x -o y = y

# File tests, against files this case creates.
: > plain.txt
mkdir -p adir
printf 'data' > nonempty.txt
: > emptyfile.txt
r -e plain.txt
r -e nosuch.txt
r -f plain.txt
r -f adir
r -d adir
r -d plain.txt
r -s nonempty.txt
r -s emptyfile.txt
r -r plain.txt
r -w plain.txt
# -nt / -ot compare mtimes; a missing file counts as older.
r plain.txt -nt nosuch.txt
r nosuch.txt -nt plain.txt

# An unquoted empty variable changes the argument count — the classic bug that
# makes `[ $x = y ]` a syntax error rather than false.
x=''
test $x = y 2>/dev/null; echo "unquoted-empty-status=$?"
test "$x" = y; echo "quoted-empty-status=$?"

# `[` requires a closing `]`; `test` must not have one.
[ x = x ]; echo "bracket-status=$?"
[ x = x 2>/dev/null; echo "missing-bracket-status=$?"
test x = x ] 2>/dev/null; echo "test-with-bracket-status=$?"

# `test` is a builtin but also a normal command name, so it can be redirected
# and its status used in a pipeline.
test -f plain.txt && echo and-ran
test -f nosuch.txt || echo or-ran

rm -rf plain.txt adir nonempty.txt emptyfile.txt
