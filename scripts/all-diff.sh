#!/bin/sh
# Run every `*-diff.sh` harness and print one summary line each.
#
# The harnesses are the only place where our utilities are compared against
# real GNU output, so after a change to something every utility shares --
# `quote.rs`'s diagnostics above all -- running one of them proves nothing
# about the rest. This runs the lot and reports each tail line, so a
# regression anywhere shows up in a single screen.
#
# Arguments are harness names to skip, for any known to be red for a reason
# already written down:
#
#     sh scripts/all-diff.sh calc
#
# Nothing is skipped at present. `calc` was, for "the three `bc` bugs in
# `known-issues.md`" -- which did not exist. The harness was running
# `coreutils`'s bc rather than `userspace/bc`'s: both packages produce
# `target/.../debug/bc.exe`, and whichever built last won. Every harness now
# builds its own subject and names the package it comes from, so a red harness
# here is a real difference again. See `scripts/diff-subject.sh`.
#
# Progress goes to stderr as each harness starts, because the slowest take
# minutes and a run that prints nothing for that long is indistinguishable
# from a hang. Do not pipe this through `tail`: that withholds everything
# until the run ends, which is exactly the visibility this is providing.
#
# A full run takes on the order of half an hour, so do not edit *this file*
# while one is in flight. `sh` reads a script incrementally from a saved file
# offset rather than slurping it, so an edit that changes the byte length
# makes the running shell resume mid-token and die with something like
# `syntax error near unexpected token 'done'` -- pointing at a line that is
# perfectly valid, after every harness has already run and printed. That is a
# confusing way to lose only the final exit status; it happened once. Edit a
# copy, or wait.
set -u
cd "$(dirname "$0")/.." || exit 1

# This script's own name matches `*-diff.sh`, so the glob below picks it up and
# it would run itself -- unboundedly, each copy starting another. That is not
# hypothetical: it happened, and about 2900 MSYS shells accumulated in six
# minutes before it was noticed. Excluding it by *name* would be a rule that
# breaks the moment the file is renamed, so the guard compares resolved paths,
# which cannot drift.
self=$(cd "$(dirname "$0")" && pwd)/$(basename "$0")

skip=" $* "
rc=0
for h in scripts/*-diff.sh; do
    here=$(cd "$(dirname "$h")" && pwd)/$(basename "$h")
    [ "$here" = "$self" ] && continue
    name=$(basename "$h" -diff.sh)
    case "$skip" in
        *" $name "*) printf '%-12s SKIPPED\n' "$name"; continue ;;
    esac
    printf 'running %s...\n' "$name" >&2
    out=$(sh "$h" 2>&1 | tail -1)
    printf '%-12s %s\n' "$name" "$out"
    # The harnesses do not share a summary wording -- most end "N passed, 0
    # differed, M differ on purpose", `extfloat` ends "no differences". Both
    # are green, and matching only the first would report a passing harness as
    # a failure, which is the way an aggregate runner stops being believed.
    case "$out" in
        *" 0 differed"* | "no differences") ;;
        *) rc=1 ;;
    esac
done
exit "$rc"
