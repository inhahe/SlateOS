#!/usr/bin/env bash
# One side of the `printf` differential test.
#
#   printf-probe.sh PRINTF-COMMAND CASE-FILE
#
# Runs PRINTF-COMMAND once per case and writes a four-line record for each:
#
#   CASE <name>
#   RC <exit status>
#   OUT <stdout>
#   ERR <stderr>
#
# `scripts/printf-diff.sh` runs this script twice -- once on the host against
# our `printf.exe`, once inside WSL against GNU's -- and compares the two
# record files. Both sides run the *same* script for the same reason both
# sides read the same case file: a difference in how the two sides were driven
# would show up as a difference in `printf`, which is the one thing this test
# must not do.
#
# ## Why the command is run through `env`
#
# Two constraints meet here, and `env` is the only spelling that satisfies
# both.
#
# `printf` is a bash *builtin*, so inside WSL a bare `printf` does not run GNU
# coreutils at all. The builtin is a different program with different
# diagnostics -- it says `printf: 0o17: invalid number` where GNU says
# `'0o17': value not completely converted` -- so a harness pointed at it would
# report dozens of differences that say nothing about our code.
#
# But spelling it `/usr/bin/printf` to dodge the builtin breaks the comparison
# a second way: every GNU diagnostic is prefixed with `argv[0]`, so each one
# would read `/usr/bin/printf: ...` against our `printf: ...`, and the
# `Try '...' --help` line would differ too. Every error case in the file would
# fail for a reason that is purely about how the harness invoked it.
#
# `env printf` resolves the name through PATH -- which contains no builtins --
# and execs it with `argv[0]` still the bare word `printf`. Our side is run
# through `env` as well, for symmetry rather than necessity: our binary spells
# its own name in diagnostics rather than reading `argv[0]`.
#
# The streams are recorded through `od -An -v -c` rather than verbatim.
# printf's whole purpose is to emit bytes chosen by the caller: a case can put
# a NUL, a lone CR, or a terminal escape sequence in the middle of a line, and
# several deliberately do. `od -c` is faithful (it escapes what it cannot
# print, and escapes the backslash it escapes with, so distinct streams stay
# distinct) and still readable enough to diagnose a difference by eye.
#
# `timeout` is a backstop, not the bound: no printf case loops. It is here
# because a probe that hangs holds a 9p mount open until the harness is killed.
set -u

if [ $# -ne 2 ]; then
  echo "usage: printf-probe.sh PRINTF-COMMAND CASE-FILE" >&2
  exit 2
fi

cmd=$1
casefile=$2

out=$(mktemp)
err=$(mktemp)
trap 'rm -f "$out" "$err"' EXIT

# US (0x1f) rather than a tab: `read -a` collapses runs of whitespace
# separators, which would silently merge an empty argument into its
# neighbours, and `printf ''` is a case.
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

  # stdin is closed off: printf never reads it, but `timeout` and the shell
  # share a descriptor with the loop, and a child that read from it would eat
  # the rest of the case file.
  timeout 10 env "$cmd" ${args[@]+"${args[@]}"} >"$out" 2>"$err" </dev/null
  rc=$?

  printf 'CASE %s\nRC %s\nOUT%s\nERR%s\n' "$name" "$rc" \
    "$(od -An -v -c <"$out" | tr -s ' \n' ' ')" \
    "$(od -An -v -c <"$err" | tr -s ' \n' ' ')"
done < "$casefile"
