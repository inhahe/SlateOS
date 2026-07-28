# printf: format reuse, %q quoting, %b escape interpretation, width/precision,
# and the numeric-argument conversions bash performs on non-numbers.
printf '%s-%s\n' a b c d
printf '[%5s][%-5s][%.2s]\n' ab ab abcdef
printf '%d %i %05d\n' 42 -7 3
printf '%x %X %o %c\n' 255 255 8 hello
printf '%e\n' 1234.5
printf '%.3f %g\n' 2.5 0.0001

# %b interprets backslash escapes in the *argument*; %s does not.
printf '%b|%s\n' 'a\tb' 'a\tb'

# %q produces a re-usable quoting of its argument.
printf '%q\n' 'a b' "it's" '$x' 'plain'

# A missing argument is treated as empty / zero, and the format is reused only
# while arguments remain.
printf '[%s][%d]\n'
printf '%s\n' ''

# Escapes in the *format* are always interpreted.
printf 'x\ty\n'
printf 'octal=\101 hex=\x42\n'

# -v stores instead of printing.
printf -v out '%s=%d' k 9
echo "out=$out"

# An argument that is not a number is complained about in its place among the
# output, not ahead of it — and when that output is going to a file, each piece
# of it is a separate write which must not start the file over.
printf '%d\n' 5 bad 7 2>&1; echo "rc=$?"
printf '%d\n' 5 bad 7 >f 2>/dev/null; cat f
