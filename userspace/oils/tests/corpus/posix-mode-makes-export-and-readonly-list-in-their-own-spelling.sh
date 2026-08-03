# Outside posix mode, `export -p` and `readonly -p` borrow `declare -p`'s output
# form: `declare -x NAME="v"`, `declare -ar NAME=(…)`, and — for `-f` — the
# whole reconstructed function definition followed by a `declare -fx NAME` line.
#
# In posix mode both print in their *own* spelling instead: the builtin's
# keyword in place of `declare`, and of the attribute letters only the array
# kind survives. So `declare -rx RX="1"` lists as `export RX="1"` under
# `export -p` and as `readonly RX="1"` under `readonly -p` — each builtin shows
# only its own attribute, never the other's — while `declare -air N=(…)` keeps
# just the `-a`. A name with no value keeps its bare form.
#
# `declare -p` is untouched by the mode, and so is the `typeset` spelling: bash
# decides this from the command word it dispatched on, not from the variable.
#
# The two listings enumerate the whole environment, which differs between the
# shells being compared, so everything here is filtered to the names this script
# makes. They all contain `zz`.

mine() { grep -i zz; :; }

echo "=== export -p, outside posix mode and inside it"
export ZZA=1
export ZZUNSET
declare -rx ZZRX=2
declare -ax ZZARR=(a b)
declare -Ax ZZASSOC=([k]=v)
declare -aix ZZINT=(3 4)
export -p | mine
echo "--- and now in posix mode"
set -o posix
export -p | mine
echo "--- a bare \`export\` lists the same way"
export | mine
echo "--- \`-a\` still restricts the listing to indexed arrays"
export -pa | mine
set +o posix

echo "=== readonly -p, outside posix mode and inside it"
readonly ZZR=5
readonly ZZRUNSET
declare -ar ZZRARR=(c d)
declare -Ar ZZRASSOC=([j]=w)
declare -ir ZZRINT=6
readonly -p | mine
echo "--- and now in posix mode"
set -o posix
readonly -p | mine
echo "--- a bare \`readonly\` lists the same way"
readonly | mine
set +o posix

echo "=== the function listings drop the body in posix mode"
zzf() { echo one; }
zzg() { echo two; }
export -f zzf zzg
readonly -f zzf
echo "--- export -pf"
export -pf
set -o posix
echo "--- export -pf, posix"
export -pf
set +o posix
echo "--- readonly -pf"
readonly -pf
set -o posix
echo "--- readonly -pf, posix"
readonly -pf
set +o posix

echo "=== but declare -p and the typeset spelling are untouched"
set -o posix
declare -p ZZRX ZZR
typeset -p ZZA
declare -p | mine
typeset -rp | mine
set +o posix

echo "=== …and the mode is read when the listing runs, not when the name was made"
set -o posix
export ZZLATE=7
set +o posix
export -p | mine
set -o posix
export -p | mine
set +o posix
