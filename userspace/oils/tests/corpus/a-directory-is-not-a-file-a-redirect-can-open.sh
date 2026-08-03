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
# (The *reading* forms — `< dir`, `exec < dir` — are left out. bash opens a
# directory for reading successfully and lets the reading command be the one to
# fail; osh cannot on this host, where `CreateFile` refuses a directory outright.
# See TD-OILS-A-DIRECTORY-AS-A-REDIRECT-TARGET-REPORTS-THE-WRONG-ERROR-AND-FAILS-A-READ.)

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

echo "still here"
