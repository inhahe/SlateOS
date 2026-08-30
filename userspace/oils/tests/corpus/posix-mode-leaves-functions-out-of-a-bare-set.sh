# A bare `set` lists the shell's variables and then every function definition —
# except in posix mode, where POSIX says the output is the *variables*, so bash
# drops the functions entirely. Not just their bodies: the names do not appear
# either, which is what keeps the listing re-inputtable the way the standard
# describes it.
#
# Nothing else about the listing changes with the mode, and neither does
# `declare -f`, which is bash's own spelling and keeps printing definitions in
# both modes.
#
# (Everything below counts lines rather than showing them: a bare `set` prints
# the whole environment, which the two shells cannot be expected to share.)

f() { echo hi; }
g() { :; }
v=zzz

echo "=== the default listing has both"
echo "  the variable: $(set | grep -c '^v=zzz$')"
echo "  the function: $(set | grep -c '^f () *$')"
echo "  and the other one: $(set | grep -c '^g () *$')"
echo "  bodies too: $(set | grep -c '^    echo hi$')"

echo "=== and the functions come after every variable"
echo "  variables left after the first function line: $(set | sed -n '/^f () *$/,$p' | grep -c '^v=zzz$')"

echo "=== posix mode drops them"
( set -o posix; echo "  the variable is still there: $(set | grep -c '^v=zzz$')" )
( set -o posix; echo "  the function is gone: $(set | grep -c '^f () *$')" )
( set -o posix; echo "  its name too: $(set | grep -c '^f')" )
( set -o posix; echo "  and its body: $(set | grep -c '^    echo hi$')" )

echo "=== but declare -f is unmoved"
( set -o posix; echo "  declare -f: $(declare -f | grep -c '^f () *$')" )
( set -o posix; echo "  declare -F: $(declare -F | grep -c '^declare -f f$')" )
( set -o posix; echo "  and by name: $(declare -f g | grep -c '^g () *$')" )

echo "=== the mode going away brings them back"
( set -o posix; set +o posix; echo "  after +o posix: $(set | grep -c '^f () *$')" )
echo "  and outside the subshells: $(set | grep -c '^f () *$')"
