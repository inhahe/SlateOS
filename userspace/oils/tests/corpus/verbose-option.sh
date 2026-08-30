# `set -v` / `set -o verbose` — echo input to stderr as the *reader* consumes
# it. Unlike `set -x`, which reports commands as they are *executed* after
# expansion, this is a property of reading: the text goes out raw, before the
# unit runs, and whatever the unit turns out to be.
#
# That distinction is what most of this case pins. The echo carries the comment
# and blank lines before a command, a here-document's body *and* its delimiter,
# a `\<newline>` still spelled with its backslash, and the newlines inside a
# quoted string — none of which survive into the command history's version of
# the same lines. It reaches every source the shell parses (an `eval` string, a
# `source`d file, a trap action) but not a body the parser already swallowed:
# a function's, and — because bash clears the flag in the subshell — a command
# or process substitution's.

echo "=== off by default"
set -o | grep '^verbose'
echo "  dash=[$-]"

set -v

echo "=== on: the flag shows up in \$-, set -o and SHELLOPTS"
set -o | grep '^verbose'
case ":$SHELLOPTS:" in *:verbose:*) echo "  in SHELLOPTS";; *) echo "  not in SHELLOPTS";; esac

echo "=== v sits between u and x, and m between h and n"
set -u -m
echo "  dash=[$-]"
set +u +m

echo "=== a compound command is echoed whole, before any of it runs"
if true; then
  echo compound
fi

# a comment line, and the blank line under it, ride along with the next command

echo "=== the here-document body and its delimiter are echoed too"
cat <<XEOF
body line
XEOF

echo "=== a line continuation keeps its backslash; the history's copy would not"
echo one \
  two

echo "=== newlines inside a quoted word are echoed as typed"
echo 'first
second'

echo "=== an eval string is read, so its units are echoed one at a time"
eval 'echo from-eval
echo second-from-eval'

echo "=== so is a sourced file"
printf 'echo from-source\n' > verbose-src.tmp
. ./verbose-src.tmp
rm -f verbose-src.tmp

echo "=== a function body is echoed when defined, not when called"
vf() { echo in-function; }
vf

echo "=== a substitution's body is not echoed: the flag is off inside it"
vsub=$(echo "sub says [$-]")
echo "  $vsub"
cat < <(echo "procsub says [$-]")

echo "=== but an ordinary subshell inherits it"
( echo "subshell says [$-]" )

echo "=== a trap action is read too"
trap 'echo in-exit-trap' EXIT

echo "=== set +v is itself echoed, being read while the flag was still up"
set +v
echo "  dash=[$-]"
