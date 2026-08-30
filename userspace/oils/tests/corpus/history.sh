# `set -o history` records the *top-level* input stream's lines, one entry per
# parse unit, and `history` lists them with the numbers readline would give.
#
# Everything here is reachable from a non-interactive shell, which is the whole
# reason the option exists there: recording is off by default, but turning it on
# makes the same machinery an interactive bash uses observable.
#
# See known-issues.md TD-OILS-MISSING-INTERACTIVE-BUILTINS.

echo "=== the option is off, and HISTCMD reads 0, until it is turned on"
echo "  HISTCMD=[$HISTCMD]"
echo "  names=[${!HIST*}]"
declare -p HISTCMD
case ":$SHELLOPTS:" in *:history:*) echo "  in SHELLOPTS";; *) echo "  not in SHELLOPTS";; esac
set -o | grep '^history'

echo "=== turning it on creates the sizing variables and starts recording"
set -o history
echo "  names=[${!HIST*}]"
echo "  HISTSIZE=[$HISTSIZE] HISTFILESIZE=[$HISTFILESIZE]"
case ":$SHELLOPTS:" in *:history:*) echo "  in SHELLOPTS";; *) echo "  not in SHELLOPTS";; esac
history

echo "=== a line is recorded when it is read, so HISTCMD counts its own line"
echo "  HISTCMD=[$HISTCMD]"
echo "  HISTCMD=[$HISTCMD]"

echo "=== a multi-line command is one entry, rejoined"
for i in 1 2
do
  echo "  i=$i"
done
if true; then
  echo "  then"
fi
x=$(echo a
echo b)
echo "  x=[$x]"

echo "=== a comment, a here-document and a quoted newline each keep their line"
echo "  with a comment" # trailing comment
cat <<END
  heredoc body
END
echo "  quoted
  newline"

echo "=== a joined-away line continuation is not stored; a kept one is"
history -c
: a && \
:
x=$(echo a \
b)
echo "  one \
two"
cat <<END
  body \
next
END
cat <<'END'
  literal \
kept
END
[[ ab =~ a\
b ]]; echo "  regex rc=$?"
history

echo "=== blank lines are dropped"

echo "  after blanks"
history

echo "=== only the top-level reader records; source/eval/functions do not"
f() {
  echo "  in f"
}
cat > inner.sh <<'INNER'
echo "  sourced"
INNER
. ./inner.sh
eval 'echo "  evaled"'
f
history

echo "=== the history is cloned into subshells and command substitutions"
( history | tail -2 )
echo "  [$(history | tail -1)]"

echo "=== -s replaces the entry the -s line itself made"
history -s replacement text
history | tail -2

echo "=== -d takes an offset, a negative offset, or a START-END range"
history -d 1
history | head -2
history -d -1
history -d 2-3
history | head -3

echo "=== -d rejects what it cannot reach, and is silent on a reversed range"
history -d 0; echo "  rc=$?"
history -d 999; echo "  rc=$?"
history -d foo; echo "  rc=$?"
history -d 3-; echo "  rc=$?"
history -d 9-8; echo "  rc=$?"

echo "=== -p prints its arguments, -c empties the list and restarts numbering"
history -p one two
history -c
echo "  HISTCMD=[$HISTCMD]"
echo "  next"
history

echo "=== a count operand lists only the tail; 0 lists nothing"
echo "  filler"
history 2
history 0
history 99 | head -1

echo "=== bad usage is a usage error"
history -z; echo "  rc=$?"
history -d; echo "  rc=$?"
history bogus; echo "  rc=$?"

echo "=== HISTSIZE caps the list, and the numbers climb past the cap"
history -c
HISTSIZE=3
echo "  one"
echo "  two"
echo "  three"
echo "  four"
history
echo "  HISTCMD=[$HISTCMD]"

echo "=== HISTSIZE=0 stores nothing at all"
HISTSIZE=0
echo "  dropped"
history
echo "  HISTCMD=[$HISTCMD]"

echo "=== turning the option off stops recording but keeps the list"
HISTSIZE=500
history -c
echo "  kept"
set +o history
echo "  not kept"
history
set -o history
echo "  kept again"
history
