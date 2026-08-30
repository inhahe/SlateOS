# A trap handler's body is numbered from the line the shell had reached when the
# handler fired, not from 1: bash re-reads the handler where it stands, so
# `$LINENO` inside the body — and any diagnostic the body raises — names the
# line of the command that triggered it, and a second line of the body names the
# line after that.
n() { sed 's/^.*: line \([0-9]*\): /line \1: /'; }
echo start
trap 'echo "L=$LINENO"
echo "L2=$LINENO"' DEBUG
echo one
trap - DEBUG
echo ---- ERR
trap 'echo "E=$LINENO"' ERR
false
trap - ERR
echo ---- RETURN
f() { echo in-f; }
set -T
trap 'echo "R=$LINENO"' RETURN
f
trap - RETURN
set +T
echo ---- EXIT-in-subshell
( trap 'echo "X=$LINENO"' EXIT; echo body )
echo ---- diagnostics-are-numbered-the-same-way
{ trap 'nosuch_cmd_xyz_in_trap' DEBUG
  echo two
  trap - DEBUG
} 2>&1 | n
echo "=== done"
