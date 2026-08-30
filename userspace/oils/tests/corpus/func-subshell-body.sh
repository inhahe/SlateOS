# A `f() ( … )` definition has a *subshell* body: the parentheses belong to the
# function, so everything the body does is confined to a child shell. osh used
# to unwrap the parentheses at parse time and run the body in the caller.
echo "=== a subshell body cannot leak variables"
x=outer
f() ( x=inner; echo "  inside x=$x" )
f
echo "  after x=$x"

echo "=== nor a directory change"
mkdir -p sub
here=$PWD
g() ( cd sub; echo "  inside pwd=${PWD#"$here"}" )
g
echo "  after pwd=${PWD#"$here"}"

echo "=== nor an exit: it ends the subshell, not the shell"
h() ( echo "  before"; exit 3; echo "  not reached" )
h
echo "  rc=$?"
echo "  the shell survives it"

echo "=== the exit status is the subshell's"
i() ( false )
i
echo "  rc=$?"

echo "=== arguments and redirections still work"
j() ( echo "  args=$* count=$#" )
j a b c
k() ( echo "  to stderr" >&2 )
k 2>&1

echo "=== and a brace body still shares the caller's shell"
m() { x=braced; }
m
echo "  after x=$x"
