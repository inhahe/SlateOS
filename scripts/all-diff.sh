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
# here is a real difference again. See `scripts/diff-wsl.sh`, under "Why the
# subject is built, every run".
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
    # Each harness's *own* interpreter, taken from its shebang, and not `sh`
    # for all of them. They are not all the same shell: every harness but
    # `osh-diff.sh` is `#!/usr/bin/env bash` and uses bash syntax, while
    # `osh-diff.sh` is deliberately `#!/bin/sh`.
    #
    # Running the lot under `sh` -- which is what this did until 2026-08-30 --
    # did not report those harnesses as red. It reported whatever dash said
    # last, and dash's complaint is one line: `uniq` came out as
    # `scripts/uniq-diff.sh: 63: Syntax error: "(" unexpected`, from the
    # `ENVV=()` on that line, and `test` came out as the tail of a comment. So
    # a harness with 200 green cases silently ran none of them, in an aggregate
    # runner whose entire job is to notice that. Both are red under the
    # `case` below, which is the only reason it was caught at all -- and the
    # `rc` they set was in turn swallowed by a caller that piped this through
    # `tail`, which the header already warns against for a different reason.
    case $(head -n 1 "$h") in
        *bash) run=bash ;;
        *)     run=sh ;;
    esac
    out=$("$run" "$h" 2>&1 | tail -1)
    printf '%-12s %s\n' "$name" "$out"
    # The harnesses do not share a summary wording -- most end "N passed, 0
    # differed, M differ on purpose", `extfloat` ends "no differences", and
    # `osh` ends "N matched, M waived, 0 failed" because its corpus is shell
    # scripts rather than argv cases. All three are green, and matching only
    # some of them would report a passing harness as a failure, which is the
    # way an aggregate runner stops being believed.
    #
    # Each pattern anchors on the count being zero, so a wording that changes
    # underneath this stops matching and is reported red. That is the safe
    # direction to be wrong in: a false red gets looked at, a false green does
    # not.
    case "$out" in
        *" 0 differed"* | *" 0 failed" | "no differences") ;;
        *) rc=1 ;;
    esac
    # And an xfail that has started *agreeing* is red too, even though its
    # "differed" count is zero and the arm above therefore let it through. It
    # means the harness is still certifying a difference that is no longer
    # there: either the difference was fixed and nobody promoted the case, or
    # -- as here -- the case stopped measuring anything.
    #
    # The instance that prompted this was `tee`, reporting "71 passed, 0
    # differed, 5 differ on purpose, 1 NO LONGER differ (update the harness)"
    # while this runner called it green. The XPASS turned out to be the `sh`
    # bug above and not a stale case at all: under dash, line 277's bash-only
    # expansion raised `Bad substitution`, the exit status of the two sides was
    # never captured, and a case xfailed for "GNU dies of SIGPIPE (141);
    # SlateOS has no signals" compared two missing statuses and agreed. Under
    # bash `tee` is 71 passed, 0 differed, 6 differ on purpose. So this arm has
    # never yet caught a real stale xfail -- but a runner that reports "update
    # the harness" as green is not one worth running, whichever cause is
    # underneath.
    #
    # Two wordings, because the harnesses have two. The zero case has to be
    # excluded first: `time` prints its count unconditionally, so a bare match
    # on the phrase would make it permanently red.
    case "$out" in
        *" 0 unexpectedly agreed"*) ;;
        *"unexpectedly agreed"* | *"NO LONGER differ"*) rc=1 ;;
    esac
done
exit "$rc"
