#!/usr/bin/env bash
# One side of the `seq` differential test.
#
#   seq-probe.sh SEQ-COMMAND CASE-FILE
#
# Runs SEQ-COMMAND once per case and writes a four-line record for each:
#
#   CASE <name>
#   RC <exit status>
#   OUT <stdout>
#   ERR <stderr>
#
# `scripts/seq-diff.sh` runs this script twice -- once on the host against our
# `seq.exe`, once inside WSL against GNU's -- and compares the two record
# files. Both sides run the *same* script for the same reason both sides read
# the same case file: a difference in how the two sides were driven would show
# up as a difference in `seq`, which is the one thing this test must not do.
#
# The streams are recorded through `od -An -v -c` rather than verbatim. seq's
# output is text and would mostly survive being pasted into a line, but not
# entirely -- a separator can be a newline, and a format can put a NUL or a
# lone CR in the middle of a line. `od -c` is faithful (it escapes what it
# cannot print, and escapes the backslash it escapes with, so distinct streams
# stay distinct) and still readable enough to diagnose a difference by eye.
#
# `timeout` is a backstop, not the bound: the generator only emits cases that
# stop. It is here because `seq 1 inf` is one keystroke away from every case in
# the file, and a probe that hangs holds a 9p mount open until the harness is
# killed.
set -u

if [ $# -ne 2 ]; then
  echo "usage: seq-probe.sh SEQ-COMMAND CASE-FILE" >&2
  exit 2
fi

cmd=$1
casefile=$2

out=$(mktemp)
err=$(mktemp)
trap 'rm -f "$out" "$err"' EXIT

# US (0x1f) rather than a tab: `read -a` collapses runs of whitespace
# separators, which would silently merge an empty argument into its
# neighbours, and `seq -s ''` is a case.
while IFS=$'\x1f' read -r -a field; do
  [ ${#field[@]} -lt 2 ] && continue
  name=${field[0]}
  end=$(( ${#field[@]} - 1 ))   # the END sentinel, which bash would otherwise
                                # drop along with a trailing empty argument
  args=()
  i=1
  while [ "$i" -lt "$end" ]; do
    # The `_` is a guard: `$(...)` strips trailing newlines, and an argument
    # is allowed to end in one.
    decoded=$(printf '%b_' "${field[$i]}")
    args+=("${decoded%_}")
    i=$(( i + 1 ))
  done

  # stdin is closed off: seq never reads it, but `timeout` and the shell share
  # a descriptor with the loop, and a child that read from it would eat the
  # rest of the case file.
  timeout 10 "$cmd" ${args[@]+"${args[@]}"} >"$out" 2>"$err" </dev/null
  rc=$?

  printf 'CASE %s\nRC %s\nOUT%s\nERR%s\n' "$name" "$rc" \
    "$(od -An -v -c <"$out" | tr -s ' \n' ' ')" \
    "$(od -An -v -c <"$err" | tr -s ' \n' ' ')"
done < "$casefile"
