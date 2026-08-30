# Every form that opens a file for *writing* refuses a directory, and says so
# with the same message however it was spelled: `ad: Is a directory`, status 1,
# and — since a redirection failure is not fatal to a non-interactive shell —
# the script carries on. `<>` is refused too, because it opens for writing as
# well as reading.
#
# `.`/`source` refuses one as well, but with a message of its own: labelled with
# the builtin and spelled with a lower-case "is". That is bash checking for a
# directory rather than letting the read fail, which is why the two messages do
# not match. In posix mode the answer changes again, and not because of the
# directory: the `$PATH` search steps past one, and posix mode has no
# current-directory fallback left to find it with, so the operand is simply
# `file not found` — and that, being a failed `.`, ends the shell.
#
# Reading is the other way round entirely: `< dir` *succeeds*, and it is the
# first read through the descriptor that answers `Is a directory`. So the status
# belongs to the reading command rather than to the redirect, and a command that
# never reads (`true < dir`) does not notice at all.
#
# Only the shell's own readers are exercised here. bash's `cat < dir` fails
# inside `cat`, and what a host's `cat` makes of a directory descriptor is not
# the shell's business — see
# TD-OILS-A-DIRECTORY-AS-A-REDIRECT-TARGET-REPORTS-THE-WRONG-ERROR-AND-FAILS-A-READ.

mkdir -p ad || exit 1

echo "=== the forms that open for writing"
echo x > ad;   echo "  > rc=$?"
echo x >> ad;  echo "  >> rc=$?"
echo x >| ad;  echo "  >| rc=$?"
echo x &> ad;  echo "  &> rc=$?"
echo x >& ad;  echo "  &-first spelling rc=$?"
echo x 2> ad;  echo "  2> rc=$?"
echo x <> ad;  echo "  <> rc=$?"

echo "=== and the same through exec, in a subshell so fd 1 survives"
( exec > ad );  echo "  exec > rc=$?"
( exec >> ad ); echo "  exec >> rc=$?"
( exec &> ad ); echo "  exec &> rc=$?"

echo "=== a name that reaches one through an expansion is no different"
d=ad
echo x > $d; echo "  \$d rc=$?"
echo x > ./ad; echo "  ./ad rc=$?"
echo x > ad/; echo "  a trailing slash rc=$?"

echo "=== the failure is the command's, and the command did not run"
rm -f marker
echo hi > ad && echo "  not reached"; echo "  rc=$?"
: > marker
echo x > ad > marker2; echo "  the second target was never opened: rc=$? [$(ls marker2 2>/dev/null)]"

echo "=== sourcing one says it differently"
. ad;      echo "  . rc=$?"
source ad; echo "  source rc=$?"
. ./ad;    echo "  . with a slash rc=$?"

echo "=== and in posix mode it is not even found"
( set -o posix; . ad; echo "  not reached" ); echo "  posix rc=$?"

echo "=== but a directory opens for *reading*: the read is what fails"
( exec < ad; echo "  exec < rc=$?" )
true < ad; echo "  a command that never reads rc=$?"
read x < ad; echo "  read rc=$? x=[$x]"
{ read x; } < ad; echo "  read in a group rc=$?"
read x < ad/; echo "  a trailing slash rc=$?"
read x 0< ad; echo "  spelled 0< rc=$?"
read -N 1 x < ad; echo "  read -N rc=$?"
read -d "" x < ad; echo "  read -d rc=$?"
mapfile v < ad; echo "  mapfile rc=$? n=${#v[@]}"
while read x; do echo "  not reached"; done < ad; echo "  while rc=$?"
x=$(< ad); echo "  \$(< ) rc=$? [$x]"

echo "=== and the descriptor carries it, however it is named or copied"
( exec 3< ad; read -u 3 x; echo "  read -u 3 rc=$?" )
( exec 3< ad; exec 4<&3; read x <&4; echo "  through a dup rc=$?" )
( { read x <&1; } 1< ad; echo "  on fd 1 rc=$?" )

echo "still here"
