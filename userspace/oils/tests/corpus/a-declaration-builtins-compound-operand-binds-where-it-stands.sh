# A declaration builtin's word list is not expanded and *then* acted on: bash
# performs a **compound** assignment as the expansion pass reaches it, at the
# operand's own position in the list. A **scalar** operand is only *expanded*
# there — the assignment itself is deferred to the builtin, which applies them
# in order once every word is in hand. Measured against bash 5.2.
#
# The whole rule falls out of that one asymmetry:
#
#   * a word written after a compound operand is expanded with it already
#     bound, whether that word is a scalar operand or another compound one;
#   * a word written after a *scalar* one is not, because nothing has been
#     assigned yet — so `declare a=1 b=($a)` leaves `b` empty and
#     `declare a=1 b=$a` leaves `b` unset;
#   * a scalar's value is expanded at its own position but assigned later, so
#     it reads the name's *old* value (`a=OLD; declare a=NEW s=$a` → `OLD`)
#     while the builtin still applies its own scalars in order
#     (`declare c=x c+=y` → `xy`);
#   * a compound that has already bound stays bound when a *later* operand
#     fails, whether the failure is the builtin's refusal or a word-expansion
#     error — and an *earlier* failure means the compound is never reached at
#     all, so it never binds.
#
# `-g` retargets the binding but does not reach the value expansion: the
# operand's value is expanded in the scope the command was written in.
#
# The failure cases have to keep their bindings in *this* shell, so they may not
# be run down a pipeline or in a `( )`. That rules out `2>&1 |` for reading
# their diagnostics, hence the scratch file — and for the word-expansion errors
# an `eval`, because such an error abandons the rest of the parse unit it
# happened in, which would swallow the very `declare -p` that asks what bound.
# The builtin's own refusals need neither: they are ordinary command failures,
# and their diagnostics do go to the redirection (a *scalar* operand naming a
# readonly variable is refused by the builtin, not while the words expand).
#
# Diagnostics name the shell and the line, so they are folded away.
sq() { sed -e 's/^.*: line [0-9]*: /SH: /' -e 's/^/    /'; }

echo "=== a scalar operand sees a compound written before it"
declare -a r=(x y) s=${r[1]}
echo "  s=[$s]"
declare -A A=([k]=v) t=${A[k]}
echo "  t=[$t]"
declare a2=1 r2=(x y) s2=${r2[0]}$a2
echo "  s2=[$s2]"

echo "=== …and so does another compound"
declare -a p=(x y) q=(${p[1]} ${p[0]})
declare -p q | sq

echo "=== …but a compound written after a *scalar* sees nothing"
declare aa=1 bb=($aa)
declare -p aa bb | sq

echo "=== …nor does a scalar after a scalar"
declare c1=1 c2=$c1
echo "  c2=[${c2-UNSET}]"

echo "=== …nor a scalar written before the compound"
declare s3=${r3[0]} r3=(x y)
echo "  s3=[$s3]"
declare -p r3 | sq

echo "=== a scalar's value is expanded at its position, assigned later"
a4=OLD
declare a4=NEW s4=$a4
echo "  s4=[$s4] a4=[$a4]"

echo "=== …while the builtin still applies its own scalars in order"
declare c5=x c5+=y
echo "  c5=[$c5]"

echo "=== a compound stays bound when a later operand is refused"
readonly ro6=1
declare -a r6=(x y) ro6=2 2>err
echo "  rc=$?"
sq < err
declare -p r6 | sq

echo "=== …and when a later word fails to expand"
eval 'declare r7=(a) x7=$((1/0))' 2>err
echo "  rc=$?"
sq < err
declare -p r7 | sq

echo "=== …but an earlier failure means it is never reached"
eval 'declare x8=$((1/0)) r8=(a)' 2>err
echo "  rc=$?"
sq < err
declare -p r8 2>&1 | sq

echo "=== a flag written after an operand is a name, and still binds first"
declare q9=(1) -x 2>err
echo "  rc=$?"
sq < err
declare -p q9 | sq

echo "=== the same inside a function, for local"
f10() {
  local -a lr=(x y) ls=${lr[1]}
  echo "  ls=[$ls]"
  local m=1 n=($m)
  declare -p n | sq
}
f10

echo "=== readonly and export bind theirs the same way"
readonly -a rr11=(x y) rs11=${rr11[1]}
echo "  rs11=[$rs11]"
export -a er11=(p q) es11=${er11[0]}
echo "  es11=[$es11]"

echo "=== -g retargets the binding but not the value's expansion"
g12() {
  local x=L
  declare -g ga=(1) gy=$x
  echo "  gy=[$gy]"
}
g12
declare -p ga | sq

echo "=== the trace shows each compound where it was written"
set -x
declare -a tr13=(m n) ts13=${tr13[0]}
set +x

echo "=== …and the builtin's own line shows a bare name in its place"
set -x
P14=1 declare -x SC14=1 arr14=(1) SD14=2
set +x

echo "=== -p is asked about the compound operands too"
declare pp15=1
declare -p pp15 qq15=(1) | sq
