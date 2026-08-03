# Four things about `printf` that its usual description hides.
#
# A `*` width or precision is not punctuation, it is an *argument*, and it goes
# through the same integer conversion `%d` does — so a word that is not a number
# is reported as `invalid number`, costs printf its exit status, and counts as
# zero. The same is true of the seconds operand of `%(FORMAT)T`.
#
# A precision truncates `%b` and `%q` as readily as `%s`, and it counts the
# bytes of the *rendered* result: the escapes `%b` interpreted and the
# backslashes `%q` added are inside the count.
#
# `%c` writes the first character of its argument, and an argument that is empty
# — or missing entirely — still has one: C's terminator. So the field is one NUL
# byte wide rather than nothing.
#
# `%q` escapes by a *deny* list, not by a safe list. That distinction is only
# visible at the edges: `,` has to be escaped because a brace expansion would
# see it, while `#` and `~` are special only where a word or an assignment's
# value starts, so `a#b` and `a~` need nothing. (Bytes above 127 are left to
# whoever wrote them — see the header of the `%q` section below.)

echo "=== a star width is converted like %d"
printf 'A%*sB\n' abc 42; echo "  rc=$?"
printf 'A%dB\n' abc; echo "  rc=$?"
printf 'A%.*sB\n' abc zzz; echo "  rc=$?"
printf 'A%*.*sB\n' p q zzz; echo "  rc=$?"
# A missing argument is an empty one, and an empty one is a valid zero.
printf 'A%*sB\n'; echo "  rc=$?"
printf 'A%.*sB\n'; echo "  rc=$?"
# The width argument is taken before the value's.
printf '[%*s][%s]\n' 3 a b
# A negative width left-justifies with the magnitude; a negative precision is
# as if none had been written.
printf '[%*d][%*d]\n' 5 42 -5 42
printf '[%.*s][%.*s]\n' 3 abcdef -3 abcdef

echo "=== the seconds of %(...)T are converted the same way"
TZ=UTC printf 'A%(%Y)TB\n' abc; echo "  rc=$?"
TZ=UTC printf 'A%(%Y)TB\n' ''; echo "  rc=$?"
TZ=UTC printf '[%(%Y-%m-%d %H:%M:%S)T]\n' 86400
# An empty format is not an empty result: bash hands `%X` to strftime.
TZ=UTC printf '[%()T]\n' 0; echo "  rc=$?"
TZ=UTC printf '[%10(%Y)T][%-10(%Y)T]\n' 0 0

echo "=== a precision truncates the rendered %b and %q"
printf '[%.3b]\n' 'ab\tcd'
printf '[%.0b]\n' abc
printf '[%.2b]\n' abcdef
printf '[%.3q]\n' 'a b c'
printf '[%.0q]\n' abc
printf '[%5q][%-5q]\n' 'a b' 'a b'
printf '[%.3s][%.3c]\n' abcdef abc

echo "=== %c on an empty or missing argument is a NUL"
printf 'A%cB\n' '' | cat -v
printf 'A%cB\n' | cat -v
printf '[%5c][%-5c]\n' '' '' | cat -v
printf '[%c][%c]\n' '' abc | cat -v

echo "=== %q escapes by a deny list"
# Every printable ASCII byte, in the middle of a word, at the front, and after
# an `=`. Nothing here is above 127: which high bytes count as printable is the
# C library's business, and the two shells need not agree about it.
for c in ' ' '!' '"' '#' '$' '%' '&' "'" '(' ')' '*' '+' ',' '-' '.' '/' ':' \
         ';' '<' '=' '>' '?' '@' '[' '\' ']' '^' '_' '`' '{' '|' '}' '~'; do
  printf 'mid=%q front=%q assign=%q colon=%q\n' "a${c}b" "${c}b" "x=${c}b" "x:${c}b"
done
printf '%q\n' '' a 0 A_z 'a
b'
printf '%q|%q\n' 'it'"'"'s' '$x'
