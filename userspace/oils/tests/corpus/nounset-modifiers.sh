# A modifier is not a default: under `set -u` it does not excuse an unset
# parameter. `${x^^}`, `${x#p}`, `${x:0:1}`, `${x/a/b}` and every `${x@…}`
# transform fault exactly as a bare `${x}` does — only the operators that
# *supply* something for the unset case (`:-`, `:=`, `:+`, `:?`) are exempt.
# The complaint names the reference as the source wrote it, which for an
# indirection is the `!ref` the writer typed rather than the variable it
# reached.
n() { sed 's/^.*: line [0-9]*: //'; }
q() { ( set -u; eval "$1"; echo "rc=$?" ) 2>&1 | n; }

echo "=== a modifier does not excuse an unset parameter"
for m in '^^' ',,' '/a/b' '#a' '%a' ':0:1' '@Q' '@U' '@u' '@L' '@E' '@K' '@k' '@a' '@A'; do
  q "echo \"[\${nosuch$m}]\""
done

echo "=== nor inside a function, where a positional is unset"
for m in '^^' ',,' '/a/b' '#a' '%a' ':0:1' '@Q' '@A'; do
  q "f() { echo \"[\${1$m}]\"; }; f"
done

echo "=== a declared-but-unassigned variable is unset for a transform too"
q 'declare -i d; echo "[${d@a}]"'
q 'declare -i d; echo "[${d@A}]"'

echo "=== the default-value family is still exempt"
q 'echo "[${nosuch:-d}]"'
q 'echo "[${nosuch:=d}]"'
q 'echo "[${nosuch+s}]"'
q 'echo "[${nosuch:+s}]"'

echo "=== a set parameter is unaffected"
q 'v=Abc; echo "[${v^^}][${v,,}][${v/b/X}][${v#A}][${v%c}][${v:1:1}][${v@Q}]"'
q 'v=; echo "[${v^^}][${v#a}][${v@Q}]"'

echo "=== an indirection is named as the reference, not as its target"
q 'r=nosuch; echo "[${!r}]"'
q 'r=nosuch; echo "[${!r^^}]"'
q 'r=nosuch; echo "[${!r#a}]"'
q 'r=nosuch; echo "[${!r@Q}]"'
q 'r=nosuch; echo "[${!r:-d}]"'
q 'v=set; r=v; echo "[${!r}][${!r^^}]"'
echo "=== done"
