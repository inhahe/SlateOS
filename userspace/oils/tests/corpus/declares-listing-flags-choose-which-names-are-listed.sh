# A `declare` with no name operands is a *listing*, and the flag letters choose
# which names it shows.
#
# `-a` and `-A` are not attributes here but **restrictions** on the source list:
# only the indexed arrays, only the associative ones — and asking for both
# selects nothing, since no name is both kinds at once. Of whatever letters are
# left a name needs **at least one** (they union: `-ri` is "readonly *or*
# integer", not "readonly *and* integer"), and if no letters are left, every
# name the restriction admits passes.
#
# The two combine as restriction-then-union, so `-ai` is "the indexed arrays
# that are integer" while `-ri` is a plain union.
#
# The *sign* matters: a plus-signed word asks for an attribute to be taken off,
# so it selects nothing — but a `p` in it still makes the command a listing.
# `declare +rp` therefore lists exactly what `declare -p` does.
#
# And a listing that selects no attribute at all — a bare `declare`, or one
# whose only flags are plus-signed — prints `set`-style `name=value` lines
# instead of `declare -FLAGS name="value"` ones, and passes over the names that
# were merely declared and never given a value.
#
# The listings walk the whole environment, which differs between the shells
# being compared, so everything is filtered to the names this script makes.

mine() { grep -i '^\(declare [^ ]* \)\?zz' ; :; }

ZZPLAIN=1
export ZZX=2
readonly ZZR=3
declare -i ZZI=4
declare -a ZZARR=(1)
declare -A ZZASSOC=([k]=v)
declare -rx ZZRX=5
declare -l ZZL=AB
declare -u ZZU=ab
declare -n ZZN=ZZPLAIN
declare -ri ZZRI=6
declare -ai ZZAI=(7)
declare -ax ZZAX=(8)
declare -xi ZZXI=9
declare ZZDECLARED

echo "=== one letter at a time"
for f in -rp -xp -ip -np -lp -up -tp; do
  echo "--- declare $f"
  declare $f | mine
done

echo "=== the array letters restrict rather than select"
echo "--- declare -ap"; declare -ap | mine
echo "--- declare -Ap"; declare -Ap | mine
echo "--- declare -aAp selects nothing"; declare -aAp | mine

echo "=== several letters union"
echo "--- declare -rxp"; declare -rxp | mine
echo "--- declare -rip"; declare -rip | mine
echo "--- declare -lup"; declare -lup | mine
echo "--- declare -rxip"; declare -rxip | mine

echo "=== …but a restriction still intersects with the union"
echo "--- declare -aip"; declare -aip | mine
echo "--- declare -xap"; declare -xap | mine
echo "--- declare -raip"; declare -raip | mine
echo "--- declare -rap selects nothing"; declare -rap | mine
echo "--- declare -Aip selects nothing"; declare -Aip | mine

echo "=== a plus-signed word selects nothing, but its p still lists"
echo "--- declare +rp"; declare +rp | mine
echo "--- declare -p"; declare -p | mine
# (Compared over this script's own names: the whole-table listings also carry
# `_` and `BASH_COMMAND`, which necessarily differ between the two commands.)
echo "--- same?"; [ "$(declare +rp | mine)" = "$(declare -p | mine)" ]; echo "  eq=$?"

echo "=== with no letters left it is a set-style listing"
echo "--- bare declare"; declare | mine
echo "--- declare +r"; declare +r | mine
echo "--- declare +x"; declare +x | mine
echo "--- and the merely-declared name is passed over"
declare | grep -c ZZDECLARED
declare -p | grep -c ZZDECLARED

echo "=== a merely-declared array is passed over too, of either kind"
# An empty compound *is* a value, so `q=()` is listed while a bare `declare -a q`
# — which leaves the name declared and unvalued — is not. `declare -p` reports
# both, and tells them apart by printing the `=()`.
declare -a ZZDA
declare -A ZZDAA
declare -a ZZEA=()
declare -A ZZEAA=()
echo "--- bare declare"; declare | mine
echo "--- set"; set | mine
echo "--- declare -p"; declare -p ZZDA ZZDAA ZZEA ZZEAA
echo "--- and the same for an integer and a plain one"
declare -i ZZDI
declare ZZDS
declare | grep -c '^ZZD[IS]'

echo "=== the same letters without -p, which lists the same names"
echo "--- declare -r"; declare -r | mine
echo "--- declare -ai"; declare -ai | mine

echo "=== the typeset spelling asks the same question"
echo "--- typeset -rp"; typeset -rp | mine
echo "--- typeset -ap"; typeset -ap | mine

echo "=== an invalid option is still an invalid option, not a listing"
declare -Q 2>&1 | tail -n +1; echo "  rc=$?"
declare +Q 2>&1 >/dev/null | head -1; echo "  rc=$?"

echo "=== a name operand turns the flags back into a declaration"
declare -r ZZPLAIN
declare -p ZZPLAIN
