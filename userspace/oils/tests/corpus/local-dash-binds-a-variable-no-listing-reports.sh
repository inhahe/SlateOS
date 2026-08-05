# `local -` does two things. The documented one makes the `set` options local
# to the call. The other is that it binds a variable *named* `-`, and that cell
# is what a lookup by name finds — but no listing that walks the variable table
# ever reports it, so the only way to see it is to ask for it.
#
# The listings are counted rather than printed wherever the table itself would
# be machine-specific; the count is the whole of what is being asserted.

e() { sed -e 's/^.*: line [0-9]*: /SH: /' -e 's/^/    /'; }
r() { echo "  rc=$?"; }

echo "=== a lookup finds it where it was made"
f1() { local -; declare -p -; r; }
f1 2>&1 | e

echo "=== …and from an inner frame, by the ordinary scope walk"
f2() { declare -p -; r; }
f3() { local -; f2; }
f3 2>&1 | e

echo "=== …but not before the frame, nor after it returns"
{ declare -p -; r; } 2>&1 | e
f4() { local -; }
f4
{ declare -p -; r; } 2>&1 | e
# …nor after the frame that made it has returned into one that did not.
f5() { f4; declare -p -; r; }
f5 2>&1 | e

echo "=== the cell never holds anything"
f6() { local -; test -v - && echo "  valued" || echo "  unset"; }
f6
# `$-` is a special parameter and not this cell, so it goes on answering with
# the option letters — under the plain name and through an indirection alike.
f7() { local -; s=$-; n=-; [ "${!n}" = "$s" ] && echo "  \$- untouched"; }
f7

echo "=== no full listing reports it"
f8() {
  local -
  declare -p | grep -c '^declare -- -$'
  declare | grep -c '^-$'
  set | grep -c '^-='
  compgen -v 2>/dev/null | grep -c '^-$'
  compgen -A variable 2>/dev/null | grep -c '^-$'
  declare -x | grep -c -- ' -$'
  declare -r | grep -c -- ' -$'
  export -p | grep -c -- '-$'
  readonly -p | grep -c -- '-$'
}
f8 2>&1 | e

echo "=== local -p reports it, but as the declaration that made it"
f9() { local w=W; local -; local x=X; local -p; }
f9 2>&1 | e
# Once, however many `local -` the frame ran…
fa() { local -; local -; local w=W; local -p; }
fa 2>&1 | e
# …and on its own when it is all the frame has.
fb() { local -; local -p; }
fb 2>&1 | e
# A sign among other operands is still the sign: it is read as an operand
# wherever it stands, and the words around it are declared as usual.
fc() { local - w=W; local -p; }
fc 2>&1 | e
fd() { local w=W -; local -p; }
fd 2>&1 | e

echo "=== local -p asks about this frame only, so an inner one does not find it"
fe() { local -p -; r; }
ff() { local -; fe; }
ff 2>&1 | e

echo "=== a lookup by name answers alongside ordinary names"
v=V
fg() { local -; declare -p - v; r; }
fg 2>&1 | e

echo "=== it takes no attributes: a declaration checks the identifier first"
fh() { local -; declare -r -; r; declare -x -; r; declare -p -; }
fh 2>&1 | e
fq() { local -; typeset -; r; declare -; r; }
fq 2>&1 | e

echo "=== unset says it worked and leaves it alone"
fj() { local -; unset -; r; declare -p -; }
fj 2>&1 | e

echo "=== a subshell inherits it"
fk() { local -; ( declare -p -; r ); }
fk 2>&1 | e

echo "=== two frames deep, each with its own"
fl() { local -; local -p; declare -p -; }
fm() { local -; fl; local -p; }
fm 2>&1 | e

echo "=== and the option half still works"
set +u
fn() { local -; set -u; declare -p -; }
fn 2>&1 | e
case $- in *u*) echo "  leaked" ;; *) echo "  not leaked" ;; esac
