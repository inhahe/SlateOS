# `n[*]` and `n[@]` name an array **whole**, which is no place to put one value.
# bash refuses every *store* through such a subscript with
# `base[sub]: bad array subscript`, and the refusal is spelled with the whole
# reference rather than with the subscript alone.
#
# Three things make it worth pinning down:
#
#   * It is a **token**, not an expression. `n["*"]=v` and `n[ * ]=v` are
#     ordinary subscripts and fail as arithmetic (`*: syntax error`); only the
#     bare two bytes are recognised.
#   * An **associative** array is exempt when the subscript is *written*: there
#     `*` is a key like any other and `m[*]=v` stores under it. The same
#     subscript arriving through a **reference** is refused all the same, so the
#     two spellings of what looks like the same store part company.
#   * What follows the refusal differs by who asked. A plain assignment and
#     `${g:=v}` drop the rest of the parse unit; `declare`/`local` report status
#     1 and carry on; `printf -v` and `read` report status 1; and arithmetic
#     reports it *without* failing — `(( g += 1 ))` complains twice, once for
#     the read walk and once for the write, and still exits 0.

p() { printf '%-22s ' "$1"; shift; printf '<%s>' "$@"; echo; }

echo "=== a written subscript, on an indexed array"
declare -a n=(1 2 3)
n[*]=SV; echo "not reached 1"
echo "line after"; p 'n' "${n[@]}"
n[@]=AV; echo "not reached 2"
echo "line after"; p 'n' "${n[@]}"
n[*]+=P; echo "not reached 3"
echo "line after"; p 'n' "${n[@]}"

echo "=== …but only as the bare token"
n["*"]=Q; echo "not reached 4"
echo "line after"; p 'n' "${n[@]}"
n[ * ]=Q; echo "not reached 5"
echo "line after"; p 'n' "${n[@]}"
s='*'
n[$s]=Q; echo "not reached 6"
echo "line after"; p 'n' "${n[@]}"

echo "=== an associative array takes the written one as a key"
declare -A A=([k]=v)
A[*]=SV; echo "star stored: $?"
A[@]=AV; echo "at stored:   $?"
p 'A keys' "${!A[@]}"
p 'A vals' "${A[@]}"

echo "=== declare and local report and carry on"
declare 'n[*]=D'; echo "declare status=$?"
f() { local 'n[*]=L'; echo "local status=$?"; }
f
p 'n' "${n[@]}"

echo "=== printf -v and read refuse and report"
printf -v 'n[*]' X; echo "printf status=$?"
read -r 'n[@]' <<< hello; echo "read status=$?"
p 'n' "${n[@]}"

echo "=== the refusal comes before the readonly guard"
declare -a ro=(9); readonly ro
ro[*]=X; echo "not reached 7"
echo "line after"

echo "=== and before the array is brought into being"
nope[*]=Q; echo "not reached 8"
declare -p nope 2>&1 | head -1

echo "=== a reference that names an array whole"
declare -a k=(3 5 7)
declare -A ka=([q]=9)
declare -n g='k[*]'
declare -n G='k[@]'
declare -n M='ka[*]'
g=NEW; echo "not reached 9"
echo "line after"; p 'k' "${k[@]}"
g+=AP; echo "not reached 10"
echo "line after"; p 'k' "${k[@]}"
printf -v g X; echo "printf status=$?"
read -r g <<< hello; echo "read status=$?"
p 'k' "${k[@]}"

echo "=== an associative one is refused too, where the written subscript was not"
M=SV; echo "not reached 11"
echo "line after"; p 'ka keys' "${!ka[@]}"

echo "=== arithmetic reports and does not fail"
(( g += 1 )); echo "+= status=$?"
(( G = 42 )); echo "=  status=$?"
(( M += 1 )); echo "assoc status=$?"
let 'g = 5'; echo "let status=$?"
p 'k' "${k[@]}"
p 'ka keys' "${!ka[@]}"

echo "=== a written whole subscript in arithmetic is the same refusal"
(( k[*] = 2 )); echo "status=$?"
p 'k' "${k[@]}"

echo "=== := through the reference, once the array has nothing to read"
declare -a e=()
declare -n eg='e[*]'
echo "got <${eg:=DD}>"; echo "not reached 12"
echo "line after"; p 'e' "${e[@]}"

echo "=== a base that does not exist yet"
declare -n hg='nada[*]'
hg=QQ; echo "not reached 13"
echo "line after"; declare -p nada 2>&1 | head -1

echo "=== declare through the reference reports and carries on"
declare hg=D; echo "declare status=$?"

echo "=== what is not a store goes through untouched"
unset g; echo "unset status=$?"; p 'k' "${k[@]}"
getopts x 'n[*]' 2>&1 | head -1
mapfile -t 'n[*]' < /dev/null; echo "mapfile status=$?"
