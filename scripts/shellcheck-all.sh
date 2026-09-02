#!/bin/bash
# Run shellcheck over every script in this directory and below it.
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

# WHICH FILES. `*.sh` in this directory was the rule until 2026-09-02, and it
# silently excluded 16 of our own scripts out of 103 -- everything in a
# subdirectory (`lib/worktree.sh`, the five `*-spike/` runners and their
# helpers) and the one script here with no extension, `hooks/pre-push`.
#
# That last exclusion is the one that matters. `hooks/pre-push` is the only
# code in the repository that decides what does and does not reach a shared
# remote, it is eleven independent gates of hand-written `sh`, and it had never
# been linted once. What sat there unnoticed was mild -- a single SC2034 -- but
# the *category* is not: the failure this whole file was created for, an
# unquoted expansion that word-split a path and created a stray `D:\visual` on
# the operator's disk, is a warning-level finding in exactly this style of code.
# It was equally invisible in `lib/worktree.sh`, which every spike runner
# sources, and which could not be linted at all: no shebang means SC2148, and
# SC2148 is an *error*, so the file failed to be analysed rather than failing a
# check. A gate whose file list is a glob is a gate that anything can leave by
# being filed one directory down or named without a suffix.
#
# The 11 findings across those 16 files were cleared in the commit before this
# one, so this stays what the boot-test gate above it requires: a clean-tree
# test with no baseline to drift, which can only fire on something newly
# introduced.
#
# `find` and not `git ls-files`: this must still work in a checkout where the
# work is not yet staged, which is precisely when someone runs it. The tradeoff
# is that an untracked scratch `.sh` under `scripts/` gets linted too -- which
# is the right way round, since that file is about to be committed.
#
# `-name '*.sh'` OR a shell shebang, rather than a hard-coded exception for
# `hooks/pre-push`: naming the one file we know about today reproduces this bug
# for the next extensionless hook. Hooks are the natural home of such files --
# git requires the name be exactly `pre-push`, so it cannot end in `.sh`.
#
# The shebang probe is restricted to files with *no* extension (`! -name '*.*'`)
# rather than run over everything. That is not only cheaper -- it is one `sh`
# fork per candidate, and `scripts/` holds 310 files of which exactly one has no
# extension -- it is also the rule actually being expressed: a shell script here
# either ends in `.sh` or is a file git will not let us name that way. Probing
# `.py` and `.jsonl` files for a shebang would additionally misfire on any
# heredoc, since `head -1` is the only cheap way to ask and it cannot tell a
# script from a fixture that contains one.
list_scripts() {
  find . -name __pycache__ -prune -o -type f -name '*.sh' -print
  find . -name __pycache__ -prune -o -type f ! -name '*.*' -print \
    | while IFS= read -r f; do
        if head -1 "$f" 2>/dev/null \
             | grep -qE '^#!.*(bin/(ba|da|k|z)?sh|env +(ba|da|k|z)?sh)'; then
          printf '%s\n' "$f"
        fi
      done
}

# Collected up front into an array rather than piped into the loop: a `while
# read` fed by a pipe or a here-document shares its stdin with everything in the
# body, so one child that reads stdin would eat the rest of the file list and
# the run would silently check a prefix. `mapfile` leaves stdin alone. (The
# names cannot contain newlines -- they are ours -- but they can contain spaces,
# so `for f in $(...)` is not an option either.)
mapfile -t scripts < <(list_scripts | sed 's|^\./||' | sort)

total=0
flagged=0
findings=0
for f in "${scripts[@]}"; do
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
