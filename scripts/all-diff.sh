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
# Three outcomes, not two. A harness that *could not run* -- no WSL, no GNU
# reference, no C compiler to build one with -- is neither green nor red, and
# for a long time this runner had no way to say so: it printed each harness's
# last output line, and a skip's last line is a parenthetical hint. `ls` sat in
# the column for many sweeps reading
#
#     ls             (a C compiler and make are needed; or set GNU=/path/to/a/9.5/ls)
#
# among rows of "N passed, 0 differed", looking like a footnote rather than an
# absence, while our `ls` was in fact uncertified. (`known-issues.md`,
# `B-LS-DIFF-HAS-BEEN-SKIPPING`.) The aggregate exit status was correctly
# non-zero the whole time; nobody reads an exit status when the column looks
# fine.
#
# So a skip is now detected from the *whole* output rather than its tail, named
# as `SKIPPED: <reason>`, and counted separately in the final line. It still
# makes the exit status non-zero -- a harness that did not run has not passed --
# but it is visibly a different thing from a difference, which matters more as
# `DIFF_GNU_SOURCE` spreads and a host with no compiler starts skipping several
# harnesses at once.
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

# One reused log per harness, because a skip has to be recognised from the
# whole output and the tail line alone cannot show it. Removed on every exit
# path including a Ctrl-C, since a half-hour run is exactly the one someone
# interrupts.
log=$(mktemp) || { echo "all-diff: cannot create a temporary file" >&2; exit 1; }
trap 'rm -f "$log"' EXIT
trap 'rm -f "$log"; exit 130' INT
trap 'rm -f "$log"; exit 143' TERM

skip=" $* "
rc=0
green=0
red=0
skipped=0
asked=0
for h in scripts/*-diff.sh; do
    here=$(cd "$(dirname "$h")" && pwd)/$(basename "$h")
    [ "$here" = "$self" ] && continue
    name=$(basename "$h" -diff.sh)
    case "$skip" in
        # Asked for on the command line, so it is not a failure -- the caller
        # already knows. Counted apart from the involuntary skips below for
        # that reason: conflating "you told me not to" with "I could not" is
        # how a permanent skip hides inside a deliberate one.
        *" $name "*)
            printf '%-12s NOT RUN (asked for on the command line)\n' "$name"
            asked=$((asked + 1))
            continue ;;
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
    # Quoted, because `sh` and `bash` are also command names and an unquoted
    # `run=sh` is indistinguishable from a forgotten `$(...)` -- SC2209, which
    # is a real bug class even though it is not one here. The quotes say
    # "the string `sh`", which is what is meant, to shellcheck and to a reader.
    case $(head -n 1 "$h") in
        *bash) run='bash' ;;
        *)     run='sh' ;;
    esac
    "$run" "$h" >"$log" 2>&1
    out=$(tail -n 1 "$log")

    # Did it run at all? Every whole-harness skip path -- the five in
    # `diff-wsl.sh` and the one in `calc-diff.sh` -- announces itself as
    # `<name>-diff: <reason>; skipping` or `...; SKIPPED` and then exits 0
    # without a summary. Both halves of that pattern are required here:
    #
    #  * the `-diff: ` prefix, so that the many *partial* skips are not caught.
    #    A harness that skipped some cases and ran the rest has genuinely run,
    #    and several say so -- `du` and `find` print "the unreadable-directory
    #    cases were skipped" when run as root, `df` prints one when there is no
    #    mount namespace, and `digest` prints "skipping it" for one binary of
    #    several. None of those carry the prefix, and none of them mean this.
    #  * the absence of a summary line, so that a harness which mentioned a skip
    #    somewhere and then reported real counts is scored on the counts. A red
    #    harness must not be able to disguise itself as a skip.
    case "$out" in
        *" passed,"* | *" matched,"* | "no differences") has_summary=yes ;;
        *) has_summary=no ;;
    esac
    if [ "$has_summary" = no ]; then
        why=$(grep -m1 -E -- '-diff: .*(SKIPPED|skipping)' "$log" 2>/dev/null)
        if [ -n "$why" ]; then
            # `<name>-diff: no GNU cp inside WSL; skipping` -> `no GNU cp
            # inside WSL`. The trailing marker is dropped because the column
            # header already says SKIPPED; what the reader needs is the reason.
            # Two separators because the harnesses have two -- `diff-wsl.sh`
            # writes `; skipping`, `calc-diff.sh` writes ` -- skipping.` -- and
            # the full stop comes off first so that both forms then match.
            why=${why#*-diff: }
            why=${why%.}
            why=${why%; skipping}
            why=${why%; SKIPPED}
            why=${why% -- skipping}
            printf '%-12s SKIPPED: %s\n' "$name" "$why"
            skipped=$((skipped + 1))
            rc=1
            continue
        fi
    fi

    printf '%-12s %s\n' "$name" "$out"
    was_red=no
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
        *) was_red=yes ;;
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
        *"unexpectedly agreed"* | *"NO LONGER differ"*) was_red=yes ;;
    esac

    if [ "$was_red" = yes ]; then
        red=$((red + 1))
        rc=1
    else
        green=$((green + 1))
    fi
done

# The tally, so that "everything is fine" is something the runner *says* rather
# than something the reader infers from a screen of rows that all look alike.
# The skip count is the point of it: a red row is loud, a skipped row is not,
# and one number at the bottom is what makes a skip impossible to read past.
summary="$green green, $skipped skipped, $red red"
[ "$asked" -gt 0 ] && summary="$summary, $asked not run by request"
printf '%s\n' "$summary"
exit "$rc"
