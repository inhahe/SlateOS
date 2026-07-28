# Every subshell has its *own* working directory. A `cd` inside `( )`, `$( )`,
# a pipeline stage or a command substitution must not move the parent shell,
# and — just as importantly — every relative path the subshell then uses
# (redirects, globs, `test`, `$(< f)`, `source`, external commands) must be
# taken against the subshell's directory, not the parent's.
#
# Two pipeline stages run concurrently, so this is also a check that the two
# directories are genuinely independent rather than one shared setting that
# happens to be restored afterwards.
mkdir -p root/a/b root/c
cd root
ROOT=$PWD
# The tests run in a randomly-named temp directory, so fold the absolute
# prefix away before comparing any printed path.
sq() { sed "s|$ROOT|@|g"; }
here() { pwd | sq; }

echo "=== ( )"
( cd a; here )
here

echo "=== \$( )"
echo "sub=$(cd a && pwd | sq)"
here

echo "=== pipeline stage"
cd a | cat
echo "rc=$?"
here
{ cd a; here; } | cat
here

echo "=== relative paths inside the subshell"
( cd a; echo body >f; cat f; ls >list )
cat a/f
echo "list=$(cat a/list | tr '\n' ',')"
echo "outer=$(ls | tr '\n' ',')"

echo "=== glob, test and \$(< f)"
( cd a; echo b*; [ -d b ] && echo dir-ok; [ -f f ] && echo file-ok; echo "read=$(<f)" )
echo b*
[ -f f ] && echo "leaked-f" || echo "no-f-here"

echo "=== redirect creates in the subshell's directory"
( cd c; : >made )
[ -f c/made ] && echo made-ok
[ -f made ] && echo made-leaked || echo made-not-leaked

echo "=== source"
printf 'echo "sourced=${PWD##*/}"\n' >a/s.sh
( cd a; . s.sh )

echo "=== concurrent stages keep separate directories"
{ cd a; pwd; } | { read -r p; cd c; echo "$p $PWD"; } | sq
here

echo "=== OLDPWD"
cd a
cd ..
( cd - >/dev/null; here )
here
