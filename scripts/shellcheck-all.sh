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

# RUN IN PARALLEL, REPORT IN SERIES.  This gate is pure analysis of 103
# independent files with no shared state, which is the textbook case for it, and
# it was running them one at a time.  Measured 2026-09-02 on this tree, the two
# shapes run back to back twice each on 12 cores: **124.5 s / 119.6 s serial
# against 33.4 s / 27.7 s here**, with the report `diff`-identical between them.
# Note the spread within each shape -- run-to-run variance on this machine is
# around +/-25%, so treat any two figures closer than that as equal.
#
# Batching instead (`shellcheck f1 f2 ... f103`, one process) was measured and
# is the wrong lever: 93 s against 121 s serial, only 23% off, because the cost
# here is *not* process startup.  It is analysis -- `-x` makes the tool follow
# and re-parse every sourced preamble once per sourcer, and `diff-wsl.sh` alone
# is sourced by most of the harnesses.  Batching also merges the per-file output
# that the tally below is built from, so it would cost the report to buy a fifth
# of what parallelism buys.
#
# The two phases are separate on purpose.  Findings are written to one file per
# script and read back *in the original sorted order*, so the output is
# byte-identical to the serial version -- a gate whose report reorders itself
# run to run cannot be diffed against a previous run, which is the first thing
# anyone does with one.  Interleaving the children's stdout directly would also
# tear individual lines, since nothing guarantees a whole finding is one write.
#
# `xargs -P` and not a hand-rolled bash pool.  The bash version was written
# first and is a trap worth recording, because it *looks* right and its output
# is correct -- only its speed is wrong, so nothing fails:
#
#     running=$((running + 1))
#     if [ "$running" -ge "$jobs_max" ]; then wait -n; running=$((running - 1)); fi
#
# `running` counts children that have not been *reaped*, which is not the same
# as children that are still *alive*.  `wait -n` returns immediately when any
# finished-but-unreaped child exists, so once a few jobs finish in a burst the
# backlog of zombies never drains: each iteration reaps exactly one and launches
# exactly one, and the live count settles wherever the burst left it.  Probed
# with 60 uneven sleeps at `jobs_max`=12, the counter read 11-12 throughout
# while `jobs -r` showed **2 to 6** actually running.  Measured on this tree it
# was ~2x slower than `xargs -P12`.  Fixing it needs the live count, which in
# bash means `jobs -r | wc -l` -- two forks per file to re-derive what xargs
# already tracks correctly.
#
# Not a fixed batch of N-then-wait-for-all either: analysis times here range
# from ~0.2 s to ~4 s, so a batching scheme spends most of its time with one
# straggler running and the other eleven cores idle.
#
# The `sh -c` wrapper exists to give each child its own output file, and is
# handed the index and the path as `$0` and `$1` -- an index rather than a
# mangled form of the path, because paths here contain `/` and spaces and any
# encoding of them into a filename is a second thing to get wrong.  `|| :`
# because shellcheck exits non-zero on findings and xargs would otherwise return
# 123 and, at 255, abort the whole run; the findings are read from the files.
jobs_max=$(nproc 2>/dev/null || echo 4)
outdir=$(mktemp -d) || exit 1
# Cleaned on interrupt as well as on exit: a Ctrl-C during a 30 s run otherwise
# leaves a directory of temporary files behind every time.
trap 'rm -rf "$outdir"' EXIT INT TERM

kept=()
for f in "${scripts[@]}"; do
  [ -e "$f" ] || continue
  kept+=("$f")
done

i=0
# The single quotes on the `sh -c` body are the point, not an oversight: `$SC`,
# `$SEV`, `$OUTDIR`, `$0` and `$1` must reach the child *unexpanded* and be
# resolved there, per child, from the environment and the two arguments xargs
# appends.  Double quotes would expand them in this shell, so every child would
# be handed the same already-substituted string and `$0`/`$1` would be this
# script's own name and first argument.  SC2016 cannot tell the two cases apart.
# shellcheck disable=SC2016
for f in "${kept[@]}"; do
  printf '%s\0%s\0' "$i" "$f"
  i=$((i + 1))
done | SC="$sc" SEV="$severity" OUTDIR="$outdir" \
  xargs -0 -n 2 -P "$jobs_max" \
    sh -c '"$SC" -x -S "$SEV" "$1" > "$OUTDIR/$0.out" 2>&1 || :'

# COUNT THE OUTPUTS BEFORE TRUSTING THEM.  Splitting analysis from reporting
# introduces a failure this script did not previously have: the report is now
# built from files rather than from the tool's exit status, and a *missing* file
# reads as an *empty* one -- that is, as "no findings".  So anything that stops
# the children from running at all (xargs absent, `sh` unable to exec, the temp
# directory gone, a full disk) would produce the words "0 finding(s) total" and
# exit 0.  That is the same false green the exit-2 skip in boot-test.sh's
# `check_shellcheck` was rewritten to stop being: a gate that cannot see must
# never report what a clean tree reports.
#
# One file per script is created by the redirection itself, before shellcheck is
# even exec'd, so the count is a true test of "did every child start".
produced=$(find "$outdir" -maxdepth 1 -type f -name '*.out' | wc -l)
if [ "$produced" -ne "${#kept[@]}" ]; then
  echo "shellcheck-all.sh: internal error -- the parallel analysis phase produced" >&2
  echo "  $produced output file(s) for ${#kept[@]} script(s), so some of them never ran." >&2
  echo "  Refusing to report a result: a missing output is indistinguishable from" >&2
  echo "  a clean one, and reporting it as clean is exactly the failure this gate" >&2
  echo "  exists to prevent.  Check that xargs(1) is present and that $outdir is" >&2
  echo "  writable." >&2
  exit 3
fi

total=0
flagged=0
findings=0
i=-1
for f in "${kept[@]}"; do
  i=$((i + 1))
  total=$((total + 1))
  # `$(<file)` and not `$(cat file)`: bash reads the file itself, with no fork.
  # At one `cat` per script that is 103 processes to read 103 mostly-empty
  # files, which measured ~6 s of the run -- a fifth of what the parallel
  # analysis phase above now costs in total.
  out=$(<"$outdir/$i.out")
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
