# `shopt -s xpg_echo` moves `echo`'s *default*, and nothing else about it.
# Backslash escapes are interpreted without `-e`, and `-E` is how a caller turns
# them back off; `-n` still works, `\c` still stops the output and takes the
# newline with it, and the escape dialect is exactly the one `-e` reads —
# `\0101` and `\x41` and `\u0041` are all `A`, a lone `\101` is not, and an
# unknown escape stands as written.
#
# Posix mode alone changes nothing at all. The *two together* are the exception,
# and a large one: an `xpg_echo` shell in posix mode reads no options from
# `echo` whatsoever, so `echo -n x` writes `-n x` and a newline. That is a
# property of the option scan, not of the escapes, which are still interpreted.
#
# `printf` is untouched by any of this.

echo "=== xpg_echo is off by default"
shopt xpg_echo
echo 'a\tb'
echo -e 'a\tb'

echo "=== on: escapes are the default"
shopt -s xpg_echo
shopt xpg_echo
echo 'a\tb'
echo -E 'a\tb'
echo -e 'a\tb'
echo -n 'a\tb'; echo "|"
echo -nE 'a\tb'; echo "|"
echo -En 'a\tb'; echo "|"
echo -Ee 'a\tb'; echo "|"
echo -eE 'a\tb'; echo "|"

echo "=== the dialect is -e's"
echo 'x\0101y'
echo 'x\101y'
echo 'x\x41y'
echo 'x\x4y'
echo 'x\u0041y'
echo 'x\u41y'
echo 'x\U00000041y'
echo 'x\qy'
echo 'x\\y'
echo 'a\ab\bc\ed\fe\nf\rg\th\vi' | cat -v

echo "=== \\c still stops"
echo 'a\cb'; echo "|rc=$?"
echo 'a\c'; echo "|"
echo -E 'a\cb'; echo "|"
echo -n 'a\cb'; echo "|"

echo "=== the option scan is unchanged"
echo -x 'a\tb'; echo "|"
echo -- 'a\tb'; echo "|"
echo - 'a\tb'; echo "|"
echo -ne 'a\tb'; echo "|"
echo 'a\tb' -n; echo "|"

echo "=== many operands still join on a space"
echo 'a\tb' 'c\td'
shopt -u xpg_echo

echo "=== posix alone changes nothing"
set -o posix
echo -n 'a\tb'; echo "|"
echo -e 'a\tb'; echo "|"
echo 'a\tb'
echo -- 'a\tb'
set +o posix

echo "=== posix + xpg_echo: no options at all"
shopt -s xpg_echo
set -o posix
echo -n 'a\tb'
echo -e 'a\tb'
echo -E 'a\tb'
echo -- 'a\tb'
echo -n
echo 'a\cb'; echo "|"
set +o posix
echo "=== …and back again"
echo -n 'a\tb'; echo "|"
set -o posix
echo -n 'x'; echo "|"
set +o posix
shopt -u xpg_echo

echo "=== POSIXLY_CORRECT reaches it the same way"
shopt -s xpg_echo
POSIXLY_CORRECT=1
echo -n 'x'; echo "|"
unset POSIXLY_CORRECT
echo -n 'x'; echo "|"
inner() { echo -n 'y'; echo "|"; }
scoped() { local POSIXLY_CORRECT=1; inner; }
scoped
inner
shopt -u xpg_echo

echo "=== printf is not affected"
shopt -s xpg_echo
printf '%s\n' 'a\tb'
printf 'a\tb\n'
set -o posix
printf '%s|' -n; printf '\n'
set +o posix
shopt -u xpg_echo

echo "=== shopt reports it"
shopt -q xpg_echo; echo "  q rc=$?"
shopt -s xpg_echo; shopt -q xpg_echo; echo "  q rc=$?"
shopt -u xpg_echo; shopt -q xpg_echo; echo "  q rc=$?"
