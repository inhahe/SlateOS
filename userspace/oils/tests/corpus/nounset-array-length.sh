# A subscripted *length* asks about the variable's shape, and under `set -u` bash
# faults when there is no shape to ask about: `${#x[0]}` on a scalar, on a name
# only declared (`declare -a q`, `export E`, bare `readonly R`), or on nothing at
# all reports the base name *alone* — the subscript is neither evaluated nor
# quoted, and that answer comes before any complaint the subscript would have
# earned. An array that exists but is empty (`x=()`, or one whose last element
# was `unset`) has a shape, so what counts is the table entry rather than the
# element, and `unset x` takes the shape away again. Because the shape question
# comes first the subscript is never evaluated, so a subscript that would have
# been called bad goes unmentioned; on an array that does have a shape that same
# subscript is fatal, spelled — oddly, but this is bash — as the tail of the
# reference rather than as the whole of it (`-5]`, not `x[-5]`). A nameref is
# quoted as the writer typed it, and the shell's own arrays have a shape wherever
# they are visible.
n() { sed 's/^.*: line [0-9]*: //'; }
q() { ( set -u; eval "$1"; echo "rc=$?" ) 2>&1 | n; }

echo "=== a name with no array shape faults, named without its subscript"
q 'x=scalar; echo "[${#x[0]}]"'
q 'x=scalar; echo "[${#x[@]}]"'
q 'x=; echo "[${#x[0]}]"'
q 'echo "[${#nosuch[0]}]"'
q 'x=scalar; echo "[${#x[0]}]"; echo tail'

echo "=== a declaration is not an assignment, so it has no shape either"
q 'declare -a k; echo "[${#k[@]}]"'
q 'declare -A m; echo "[${#m[@]}]"'
q 'declare -i d; echo "[${#d[0]}]"'
q 'export E; echo "[${#E[0]}]"'
q 'readonly R; echo "[${#R[@]}]"'
q 'lo() { local -a l; echo "[${#l[@]}]"; }; lo'

echo "=== the shape is asked about first, so the subscript is never evaluated"
q 'x=scalar; echo "[${#x[-5]}]"'
q 'x=scalar; echo "[${#x[$(echo 1)]}]"'
q 'x=(a b); echo "[${#x[-5]}]"'

echo "=== unsetting the whole name takes the shape away again"
q 'x=(a b); unset x; echo "[${#x[@]}]"'

echo "=== but an empty array still has a shape, so it answers 0"
q 'x=(); echo "[${#x[@]}][${#x[0]}]"'
q 'lo() { local -a l=(); echo "[${#l[@]}]"; }; lo'
q 'declare -A m; m=([k]=v); echo "[${#m[@]}]"'
q 'declare -A m; m[k]=v; unset "m[k]"; echo "[${#m[@]}]"'
q 'x=(a b); unset "x[0]"; unset "x[1]"; echo "[${#x[@]}][${#x[0]}]"'
q 'declare -a k; k[3]=z; echo "[${#k[@]}][${#k[3]}]"'

echo "=== an unsubscripted length is unaffected"
q 'x=scalar; echo "[${#x}]"'
q 'x=; echo "[${#x}]"'

echo "=== a nameref is quoted as the reference, not as its target"
q 'declare -n r=nosuch; echo "[${#r[0]}]"'
q 'a=scalar; declare -n r=a; echo "[${#r[0]}]"'
q 'a=(p q); declare -n r=a; echo "[${#r[5]}]"'

echo "=== the shell's own arrays have a shape wherever they are visible"
q 'echo "[${#BASH_SOURCE[@]}][${#BASH_LINENO[@]}]"'
q 'echo "[${#GROUPS[@]}][${#DIRSTACK[@]}]"'
q 'echo "[${#BASH_VERSINFO[@]}][${BASH_VERSINFO[0]}]"'
q 'true | true; echo "[${#PIPESTATUS[@]}][${PIPESTATUS[0]}]"'
q 'echo "[${#BASH_ALIASES[@]}][${#BASH_CMDS[@]}]"'
q 'echo "[${#BASH_REMATCH[@]}]"'
q 'f() { echo "[${#FUNCNAME[@]}][${#BASH_SOURCE[@]}][${#BASH_LINENO[@]}]"; }; f'
q 'echo "[${#COMPREPLY[@]}]"'
echo "=== done"
