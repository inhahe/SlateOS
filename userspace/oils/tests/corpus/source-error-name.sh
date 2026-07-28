# bash labels a diagnostic with the file it is *currently reading*, so errors
# raised while a `source`d script runs name that script — not the top-level one
# — and the name reverts as soon as the source returns. This holds for runtime
# errors, syntax errors, errors from a function the sourced file defined (a
# function's label is its *definition* file), and nested sources.
printf 'echo insub
nosuchcmd_sub
echo tail
' > e1.sh
printf 'echo insub2
v=%s
' "'abc" > e2.sh
printf 'g() {
nosuchcmd_g
}
' > e3.sh
printf 'echo x
readonly rv=1
rv=2
' > e4.sh
printf 'echo deep
. ./e1.sh
' > e5.sh

echo one
. ./e1.sh
echo two
. ./e2.sh
echo three
. ./e3.sh
g
echo four
. ./e4.sh
echo five
. ./e5.sh
echo six
nosuchcmd_outer
