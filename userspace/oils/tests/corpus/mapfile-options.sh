# `mapfile` as a *reader*, which is the part that is easy to get wrong: an array
# builtin looks like it fills a name and stops, but it also moves a descriptor,
# and how far it moves it is decided by options that never mention the input.
# `-n` bounds what is wanted, `-s` still has to be read to be skipped, and `-d`
# decides what a record even is — so the same three options settle both what the
# array holds and where the next reader starts.
#
# The other half is the option scan itself, which is an ordinary clustered
# getopt: it stops at the first operand, `--` ends it, and every number it reads
# is checked as it is read, before a byte of input is touched.

printf 'l1\nl2\nl3\nl4\nl5\n' > five
printf 'p1\np2\nnoeol' > partial

echo "=== what is read, and what is left"
{ mapfile -n 2 a; read r; } < five; echo "  n2 [${a[*]}] r=$r"
# The skipped records are read to be skipped, so they count towards the move.
{ mapfile -s 1 -n 2 a; read r; } < five; echo "  s1n2 [${a[*]}] r=$r"
# Two bounded reads in a row each start where the last one stopped.
{ mapfile -t -n 2 a; mapfile -t -n 2 b; } < five; echo "  pair [${a[*]}][${b[*]}]"
# Without a bound the whole input is wanted, and nothing is left behind.
{ mapfile a; read r; } < five; echo "  all n=${#a[@]} r=[$r]"
{ mapfile -n 0 a; read r; } < five; echo "  n0 n=${#a[@]} r=[$r]"
# A bound larger than the input is simply never reached.
{ mapfile -n 9 a; read r; } < five; echo "  n9 n=${#a[@]} r=[$r]"
# …and so is a skip larger than it, which still leaves nothing.
{ mapfile -s 9 -n 2 a; read r; } < five; echo "  s9 n=${#a[@]} r=[$r]"
# The bound counts *records*, and `-d` is what a record is.
{ mapfile -d , -n 1 a; read r; } <<< 'x,y,z'; echo "  d, [${a[0]}] r=[$r]"
# A numbered descriptor moves the same way.
exec 8< five
mapfile -n 2 a <&8 || true
mapfile -n 2 -u 8 c; read -u 8 r; echo "  u8 [${c[*]}] r=$r"
exec 8<&-

echo "=== the delimiter is kept unless -t takes it"
mapfile a < five; declare -p a
mapfile -t a < five; declare -p a
# A last record with no delimiter has none to keep or to strip.
mapfile a < partial; declare -p a
mapfile -t a < partial; declare -p a
# An empty `-d` is the NUL byte, which this input never contains, so the whole
# file is one record.
mapfile -d '' a < five; echo "  nul n=${#a[@]}"

echo "=== -O writes over what is there instead of clearing it"
a=(z z z z z z z z)
mapfile -O 2 -n 1 a < five; declare -p a
# Without -O the array is emptied first, so nothing of the old one survives.
b=(z z z z)
mapfile -n 1 b < five; declare -p b
# The origin is where the first record lands, so a high one leaves a gap.
unset c
mapfile -O 5 c < partial; declare -p c; echo "  n=${#c[@]}"

echo "=== the callback runs before the element it announces"
# It is handed the index it is about to fill and the value that is about to go
# there — the -t-stripped one when -t is given, the raw one otherwise.
f() { echo "  cb[$1][$2] have=[${d[*]}]"; }
mapfile -C f -c 1 d < partial
mapfile -t -C f -c 2 d < five
# `-c` counts elements assigned, not lines read, so a skip does not shift it.
mapfile -s 2 -C f -c 1 d < five
# `-C` without `-c` uses a quantum so large this input never reaches it.
mapfile -C 'echo NOPE' d < five; echo "  quiet n=${#d[@]}"

echo "=== the options are a clustered getopt"
mapfile -n2 a < five; echo "  n2 n=${#a[@]}"
mapfile -tn 2 a < five; echo "  tn [${a[*]}]"
mapfile -td, a <<< 'x,y'; echo "  td, n=${#a[@]} [${a[0]}]"
mapfile -O2 -t a < five; echo "  O2 first=${!a[@]}"
# `--` ends them, and what follows is the name — even when it looks like one of
# them, which is then refused as the name it is.
mapfile -t -- a < five; echo "  dashdash [${a[*]}]"
mapfile -- -t < five; echo "  badname rc=$?"
# The scan stops at the first operand, so an option after the name is a word
# nothing ever looks at.
mapfile a -t < five; echo "  after [${a[0]}]"
# An invalid letter is named on its own, not with the cluster it sat in.
mapfile -ta < five; echo "  invalid rc=$?"
mapfile -n < five; echo "  missing rc=$?"

echo "=== every number is checked as it is read"
# Blanks around it are skipped and a sign is allowed, because this is the same
# reader every other builtin number goes through.
mapfile -n ' 2 ' a < five; echo "  spaced n=${#a[@]}"
mapfile -n +2 a < five; echo "  signed n=${#a[@]}"
mapfile -O ' 2' a < five; echo "  origin first=${!a[@]}"
# Anything left over means it was not a number at all.
mapfile -n x a < five; echo "  n-x rc=$?"
mapfile -n 2x a < five; echo "  n-2x rc=$?"
mapfile -n 0x2 a < five; echo "  n-hex rc=$?"
mapfile -n -1 a < five; echo "  n-neg rc=$?"
mapfile -s -1 a < five; echo "  s-neg rc=$?"
mapfile -O y a < five; echo "  O-y rc=$?"
mapfile -O -1 a < five; echo "  O-neg rc=$?"
mapfile -c x a < five; echo "  c-x rc=$?"
# A quantum of zero would announce every element forever, so it is refused
# rather than taken to mean "never".
mapfile -c 0 a < five; echo "  c-0 rc=$?"
# The first bad one decides, and it beats what a good one would have done.
unset a
mapfile -n 2 -O y a < five; echo "  first rc=$?"
declare -p a

echo "=== what the name has to be is settled before anything is read"
mapfile 1bad < five; echo "  badname rc=$?"
declare -A m
mapfile m < five; echo "  assoc rc=$?"
readonly ro
mapfile ro < five; echo "  ro rc=$?"
# A refused mapfile leaves the input where it was.
{ mapfile 1bad; read r; } < five; echo "  untouched r=$r"
# The default name is MAPFILE, and a second operand is ignored in silence.
mapfile < five; echo "  default n=${#MAPFILE[@]}"
unset second
mapfile a second < five; echo "  second=[${second-unset}]"
