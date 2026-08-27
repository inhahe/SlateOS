#!/bin/bash
# Run shellcheck over every script in this directory.
#
# Why this exists: 48 scripts here carry `# shellcheck source=…` and
# `# shellcheck disable=…` annotations, which are instructions to a tool that
# was not installed on this machine — so for most of the project's life nothing
# read them and nothing checked the scripts they annotate. `bash -n` catches
# only syntax errors, and the bugs that actually bite a differential harness
# are semantic: an unquoted expansion that word-splits a filename with a space,
# a `$?` read after the wrong command, a `local` that silently swallows the
# exit status of the command it is assigned from.
#
# The tool ships a **static** Linux binary, so it needs no root and no package
# manager: fetch `shellcheck-stable.linux.x86_64.tar.xz` from the
# koalaman/shellcheck releases and drop the single file in `~/bin`. This
# script finds it there or on PATH.
#
# (That paragraph deliberately opens with "The tool" rather than the tool's own
# name: a comment whose first word after `#` is `shellcheck` is parsed as a
# *directive*, so writing the name at the start of a sentence is SC1072/SC1073
# — an error, not a style note. This file tripped it on its first run.)
#
# `-x` is required, not optional: every harness sources `diff-wsl.sh` for the
# shared preamble, and without `-x` shellcheck does not follow the `.` and
# reports every variable the preamble exports as undefined.
set -u

if ! command -v wsl >/dev/null 2>&1 && [ ! -d /mnt ]; then
  : # already in a Unix-y place
fi

sc=""
for cand in "$HOME/bin/shellcheck" shellcheck; do
  if command -v "$cand" >/dev/null 2>&1; then sc=$cand; break; fi
done
if [ -z "$sc" ]; then
  echo "shellcheck not found. Install the static binary:" >&2
  echo "  curl -fsSL -o /tmp/sc.tar.xz \\" >&2
  echo "    https://github.com/koalaman/shellcheck/releases/download/stable/shellcheck-stable.linux.x86_64.tar.xz" >&2
  echo "  tar -xJf /tmp/sc.tar.xz -C /tmp && mkdir -p ~/bin" >&2
  echo "  cp /tmp/shellcheck-stable/shellcheck ~/bin/ && chmod +x ~/bin/shellcheck" >&2
  exit 2
fi

cd "$(dirname "$0")" || exit 1

# `$1` is shellcheck's severity floor: error, warning, info (default) or style.
# It exists because the summary is only useful when it can be narrowed: at
# `info` this tree reports ~230 findings, most of them SC1003/SC2016 on
# *deliberate* literal backslashes and single-quoted `$` in harness test data,
# which are the payloads being tested and must not be "fixed". At `error` the
# list is short enough to be a gate.
severity=${1:-info}
# `--full` prints the findings themselves rather than a per-code tally.
full=${2:-}

total=0
flagged=0
findings=0
for f in *.sh; do
  [ -e "$f" ] || continue
  total=$((total + 1))
  out=$("$sc" -x -S "$severity" "$f" 2>&1)
  if [ -n "$out" ]; then
    flagged=$((flagged + 1))
    n=$(printf '%s\n' "$out" | grep -cE '^In .* line [0-9]+:')
    findings=$((findings + n))
    printf '%s: %d finding(s)\n' "$f" "$n"
    if [ "$full" = "--full" ]; then
      printf '%s\n' "$out" | sed 's/^/    /'
    else
      printf '%s\n' "$out" | grep -oE 'SC[0-9]+ \([a-z]+\)' | sort | uniq -c \
        | sed 's/^/    /'
    fi
  fi
done

printf '\n%d script(s), %d with findings at severity %s, %d finding(s) total\n' \
  "$total" "$flagged" "$severity" "$findings"
[ "$findings" -eq 0 ]
