# `.`/`source` look their operand up on `$PATH` before they look for it beside
# the caller, and the difference is visible whenever both exist. The fallback to
# the current directory is a bash extension, so posix mode takes it away: there a
# bare name that `$PATH` does not turn up is `file not found`, and — `.` being a
# special builtin — that ends a non-interactive shell.
#
# A name with a `/` in it was never a search, in either mode, and neither is
# `$PATH` consulted for one.
#
# The lookup is for a *readable* file, not an executable one, which is why it is
# a different search from the one a command name gets. That half is not shown
# here: this host has no execute bit to take away.

mkdir -p d/sub && cd d || exit 1
echo 'echo "  from the current directory"' > lib.sh
echo 'echo "  from PATH"' > sub/lib.sh
echo 'echo "  only on PATH"' > sub/onlypath.sh
echo 'echo "  reported as [${BASH_SOURCE[0]##*/}] at depth ${#BASH_SOURCE[@]}"' > sub/who.sh
here=$PWD

echo "=== PATH wins over the file beside the caller"
PATH=$here/sub; . lib.sh; echo "  rc=$?"
PATH=$here/sub; source lib.sh; echo "  rc=$?"

echo "=== a name that is only on PATH is found at all"
PATH=$here/sub; . onlypath.sh; echo "  rc=$?"

echo "=== and it is the file that was read that gets reported"
PATH=$here/sub; . who.sh

echo "=== with no hit on PATH the current directory is the fallback"
PATH=/nonexistent-dir; . lib.sh; echo "  rc=$?"
unset PATH; . lib.sh; echo "  rc=$?"
PATH=; . lib.sh; echo "  rc=$?"

echo "=== a name with a separator is not a search"
PATH=$here/sub; . ./lib.sh; echo "  rc=$?"
PATH=$here/sub; . sub/lib.sh; echo "  rc=$?"

echo "=== a missing file is reported without the builtin's name"
PATH=/nonexistent-dir; . nosuch.sh; echo "  rc=$?"
PATH=/nonexistent-dir; . ./nosuch.sh; echo "  rc=$?"

echo "=== posix mode drops the fallback, and says so differently"
( set -o posix; PATH=/nonexistent-dir; . lib.sh; echo "  not reached" ); echo "  rc=$?"
( set -o posix; PATH=/nonexistent-dir; source lib.sh ); echo "  rc=$?"
# `command` spares the shell, so the rest of the subshell runs.
( set -o posix; PATH=/nonexistent-dir; command . lib.sh; echo "  rc=$? and still here" )

echo "=== but keeps everything that was not the fallback"
( set -o posix; PATH=$here/sub; . lib.sh; echo "  rc=$?" )
( set -o posix; PATH=/nonexistent-dir; . ./lib.sh; echo "  rc=$?" )
( set -o posix; unset PATH; . lib.sh; echo "  rc=$?" )

echo "=== and the fallback comes back with the mode"
( set -o posix; set +o posix; PATH=/nonexistent-dir; . lib.sh; echo "  rc=$?" )
