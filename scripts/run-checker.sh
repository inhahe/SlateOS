# shellcheck shell=sh
#
# `run_checker` — the one implementation of "did that checker reach a verdict?"
#
# **Sourced, not run.** `.` this file, set the four `CHECKER_*` variables below,
# and call `run_checker` in place of a bare `"$py" scripts/check-thing.py`.
#
# ## Why a checker that crashes must not be reported as a finding
#
# A Python script that dies of an uncaught exception exits 1 — the same code it
# uses for "I found violations". Every gate in this project asked
# `if ! "$py" …`, so the two were indistinguishable, and the gate answered a
# crash with its own confident refusal text: *your* diagnostic names a file
# without quoting it, *your* self-test asserts on text no literal can produce.
# That is a false accusation, and the remedy each refusal goes on to offer —
# bypass the gate, fix the assertion — does not fix anything, because the check
# never ran. A gate that spends its credibility on a verdict it did not reach
# teaches its reader to bypass it, which is the one failure these gates cannot
# survive.
#
# It is not hypothetical, and it has now happened at both boundaries:
#
# * **2026-09-01, pre-push gate 8.** A push was refused whose tree passed
#   `quote-names.py --check` cleanly minutes later, unchanged. Two hours went
#   into looking for a defect in `cp` that was never there.
# * **2026-09-01, boot-test.** `check-selftest-format-wording.py` died with
#   `MemoryError` inside `strip_noise`, and `boot-test.sh` printed *"Each report
#   above is a self-test assertion demanding text that no string literal in the
#   kernel can produce"* over a report list that was empty. The advice it gave —
#   re-read what the code emits, or widen `ALLOWED` — would have been damage.
#
# Both are the same line of code written twice, which is why this lives in one
# file that both callers source rather than in each of them.
#
# ## The three outcomes
#
# Exit 0 is clean. Exit 1 *without* a Python traceback is a finding, and returns
# 1 so the call site keeps the shape it had. Anything else — a traceback, a
# usage error, a signal, an interpreter killed by the memory manager — is **no
# verdict**, and does not return to the call site at all: it exits, because
# there is no gate for which "the checker fell over" is a reason to proceed.
#
# ## The fourth outcome, which exists only where a call site asks for it
#
# "No verdict" collapses two different things: *the checker fell over*, and
# *the checker ran fine and found it had nothing to look at*. The second is a
# legitimate answer for a gate that grades something which may not be there —
# `check-libc-shape.py` reads a build artifact and exits 2 when `libc.a` is
# stale; the four `check-*-vs-bash.py` oracles shell out to real bash and
# cannot answer on a host with no WSL. Under the rules above, wiring any of
# them stops the build on every worktree that has not built a sysroot, or on
# every machine without a Linux distro. So they were not wired at all, which
# is the worse end of the trade: a gate nobody runs finds nothing, forever.
#
# `run_checker --may-skip=<rc> <label> …` is the opt-in. It is **per call
# site** and deliberately not a mode: there is no environment variable for it
# and no way to set it once for a run, because the judgement "this particular
# gate has a legitimate can't-look answer, and it is this exit code" belongs
# next to the gate, in the file a reader is already looking at. A global
# loosening would apply to the twenty-odd gates for which no such answer
# exists, and abort-on-no-verdict is load-bearing for all of them.
#
# Three constraints keep the channel from becoming the hole it is replacing:
#
# * **0 and 1 are refused.** Allowing `--may-skip=1` would let a call site
#   reclassify a *finding* as a skip, which is the one thing no amount of
#   convenience justifies. 126 and 127 are refused for the same reason in the
#   other direction: this file already reads them as the shell's codes for a
#   failed invocation, and no checker returns them as a verdict.
# * **A traceback still wins.** A checker that crashes with the skip code is
#   crashing, not skipping, so the skip arm requires the absence of the
#   traceback banner exactly as the finding arm does. Otherwise `--may-skip=2`
#   would silently absorb every `SystemExit(2)` raised from a bug.
# * **A skip is announced, and it is not a pass.** It prints the checker's own
#   first line — the reason it could not look — and says in as many words that
#   nothing was checked. If `CHECKER_SKIPLOG` names a file, the skip is also
#   appended to it, so a caller can say "27 gates ran, 2 could not look" in its
#   summary instead of the flat "ok" that a silent `return 0` would earn. A
#   gate that skips on every run must read differently from one that passes,
#   or this channel has merely moved the silence somewhere less visible.
#
# Should a checker ever print the traceback banner as part of a legitimate
# finding, it lands in the no-verdict arm too, and the run is still refused —
# the misclassification costs a confusing message, never a missed defect.
#
# ## The no-verdict message reads the exit code before advising
#
# "No verdict" is one outcome but not one *cause*, and until 2026-09-02 the
# message ended with a single paragraph naming the only cause it knew: another
# rustc-heavy job exhausting memory, so re-run it alone. That is right for a
# checker the OOM killer reached, and wrong for one that was never launched.
# The case that exposed it was exit 126 with `Argument list too long`, where
# "re-run it alone" costs a sixteen-minute push to reach an identical wall,
# because E2BIG is a limit on the command, not on the machine.
#
# So 126 and 127 get their own readings. Neither is ever a checker's own
# verdict — verified across `scripts/`, no `check-*.py` returns either — which
# is what makes it safe to interpret them as the shell's codes rather than the
# tool's.
#
# **127 is not simply the mirror of 126, and this is the trap.** The obvious
# symmetry — 126/127 mean a bad invocation, everything else means memory —
# is wrong on this host, where a `fork()` refused by the Windows commit limit
# also surfaces as 127 (`scripts/boot-history.py`, `HARNESS_ABORT_EXITS`).
# That is a resource shortage wearing a launch failure's exit code, and
# re-running alone genuinely does fix it. 127 therefore offers both readings
# and hands the reader the discriminator instead of guessing for them: the
# checker's own first line of output, which every arm now quotes inline —
# it is where `Argument list too long` was sitting unread the whole time.
#
# ## Kept, not streamed away
#
# The output is written to a file first and echoed after, rather than streamed.
# That costs liveness on a slow checker and buys the thing whose absence made
# the 2026-09-01 pre-push failure undiagnosable: the console it was read from
# keeps only the last N lines, so the refusal boilerplate survived and the lines
# naming the offending site did not. The file is deleted when the checker passes
# and kept when it does not, at a path derivable without having read the message
# that names it.
#
# ## The interpreter is an argument, not an ambient `$py`
#
# `run_checker <label> <command> [args…]` runs `<command>` itself; it does not
# reach for a `$py` in the caller's scope. It could — bash's `local` is
# dynamically scoped, so a function's `py` is visible to what it calls — but
# `boot-test.sh` declares `local py` inside each of its twenty-eight gates, and
# a dependency that works only while every call site happens not to be a
# subshell is a trap laid for whoever writes the twenty-ninth.
#
# ## Configuration
#
# All four are read at call time, so a caller may set them once at the top:
#
#   CHECKER_PROG      what to prefix this library's own sentences with, e.g.
#                     `pre-push`. Default `checker`.
#   CHECKER_LOGDIR    where a failing checker's output is kept. Default
#                     `${TMPDIR:-/tmp}`. The pre-push hook points this at the
#                     worktree's own git dir so three lanes pushing at once do
#                     not overwrite each other's evidence. That separates the
#                     *lanes*; `$$` in the filename below separates concurrent
#                     runs within one lane, which the directory cannot.
#   CHECKER_REFUSING  the word after "REFUSING to" — `push`, `build`. Default
#                     `continue`.
#   CHECKER_NOTE      one extra paragraph for the no-verdict message, or empty.
#                     The hook uses it to say why the gate's bypass is the wrong
#                     reaction; boot-test has no bypass and leaves it unset.
#   CHECKER_SKIPLOG   a file to append one line per skipped gate to, or empty.
#                     Only `--may-skip` call sites can write to it. A caller
#                     that sets it can report how many gates could not look;
#                     one that does not still gets the message on stderr.
#
# Note that there is deliberately no `CHECKER_MAY_SKIP`: see "The fourth
# outcome" above for why the permission is an argument and not a setting.

# run_checker [--may-skip=<rc>] <label> <command> [args...]
run_checker() {
    # Parsed before the label so the flag reads left-to-right at the call site.
    # `--may-skip=N`, not `--may-skip N`, because the joined form cannot be
    # separated from its value by a line continuation and then lose it.
    #
    # `_rc_skip_given` is tracked separately from `_rc_skip` because
    # `--may-skip=` -- the flag with its value left off -- strips to the empty
    # string, which is indistinguishable from "no flag passed" if emptiness is
    # the only test. That spelling then read as an ordinary call and was
    # obeyed silently, so a call site that had asked for something got no
    # complaint and no skip. Caught by this file's own suite, which is the
    # argument for having written the refusal cases first.
    _rc_skip=''
    _rc_skip_given=no
    case $1 in
    --may-skip=*)
        _rc_skip=${1#--may-skip=}
        _rc_skip_given=yes
        shift
        ;;
    esac

    _rc_label=$1
    shift
    _rc_prog=${CHECKER_PROG:-checker}
    _rc_dir=${CHECKER_LOGDIR:-${TMPDIR:-/tmp}}
    # `$$` disambiguates concurrent runs, and it is not belt-and-braces on top
    # of the distinct-label rule the callers already follow — it covers a case
    # that rule cannot reach. Distinct labels stop one *invocation* from
    # overwriting its own earlier gate's log; they do nothing when two
    # invocations run at once, because both compute the same label for the same
    # gate. Two pushes from one worktree then share a path, and the first to
    # finish clean does `rm -f` on it — deleting, in the worst case, the kept
    # evidence of the other's genuine refusal while its "full output kept at"
    # line still names it. Observed 2026-09-03: three overlapping pushes of one
    # sha, and `cat: …pre-push-raced-global-<sha>.log: No such file` from the
    # loser.
    #
    # `$$` is the shell's pid and does not change in a subshell, so every gate
    # of one hook run still shares a prefix and stays greppable together.
    _rc_log="$_rc_dir/$_rc_prog-$_rc_label.$$.log"

    # Validated before the checker runs, not after it exits, so a typo is a
    # loud failure on every run rather than a dormant one that surfaces only on
    # the day the gate first tries to skip -- which is the day nobody is
    # looking, because until then the call site behaved exactly as intended.
    if [ "$_rc_skip_given" = yes ]; then
        case $_rc_skip in
        *[!0-9]* | '')
            echo "$_rc_prog: $_rc_label: --may-skip needs an exit code, got '$_rc_skip'." >&2
            exit 1
            ;;
        0 | 1)
            echo "$_rc_prog: $_rc_label: --may-skip=$_rc_skip is refused. 0 already means" >&2
            echo "clean and 1 means a finding; letting a call site rename either one would" >&2
            echo "turn this library into a way to silence gates rather than to run them." >&2
            exit 1
            ;;
        126 | 127)
            echo "$_rc_prog: $_rc_label: --may-skip=$_rc_skip is refused. Those are the" >&2
            echo "shell's codes for 'could not execute it', which no checker returns as a" >&2
            echo "verdict of its own -- so a gate cannot use one to mean 'I could not look'." >&2
            exit 1
            ;;
        esac
    fi
    # `$*` joins on IFS's first character, and one caller (the hook's doc-links
    # gate) sets IFS to a newline to split a path list — which would render the
    # "re-run it" line one argument per line. Build the string under a known IFS.
    _rc_oldifs=$IFS
    IFS=' '
    _rc_cmd="$*"
    IFS=$_rc_oldifs

    # `PYTHONUNBUFFERED=1` because stdout and stderr are merged into one file
    # here, and a merged file is only worth reading if it is in the order the
    # checker printed it. Redirected stdout is block-buffered while stderr is
    # not, so a checker's progress lines sit in a buffer until exit and then
    # land *after* the error they chronologically preceded. The log then reads
    # as though the gate recovered from its own failure.
    #
    # Found via the skip announcement, which quotes the checker's last line as
    # the reason it gave up: a fixture printing progress on stdout and its
    # reason on stderr was recorded as having skipped because "cases loaded:
    # 214". Individual call sites passing `python -u` masked it, which is
    # precisely why the fix belongs here -- correctness of the evidence must
    # not depend on every future call site remembering a flag.
    PYTHONUNBUFFERED=1 "$@" >"$_rc_log" 2>&1
    _rc=$?
    cat "$_rc_log" >&2

    if [ "$_rc" = "0" ]; then
        rm -f "$_rc_log"
        return 0
    fi
    if [ "$_rc" = "1" ] && ! grep -q '^Traceback (most recent call last):' "$_rc_log"; then
        echo "$_rc_prog: $_rc_label: full output kept at $_rc_log" >&2
        return 1
    fi

    # The skip arm. Ordered after the finding arm so that no reachable value of
    # `_rc_skip` can shadow it -- 0 and 1 are refused above, but the ordering is
    # what makes that a second line of defence rather than the only one.
    #
    # The traceback test is the same one the finding arm uses, and for the same
    # reason: a checker that dies with `SystemExit(2)` from a bug exits 2 just
    # as loudly as one that looked and found nothing to look at. Without this,
    # `--may-skip=2` would convert that entire class of crash into a pass, and
    # it would do it on exactly the gates that are hardest to notice going
    # quiet, since they are the ones expected to skip sometimes.
    if [ -n "$_rc_skip" ] && [ "$_rc" = "$_rc_skip" ] &&
        ! grep -q '^Traceback (most recent call last):' "$_rc_log"; then
        # The LAST non-empty line, not the first.  A gate that skips rarely
        # discovers it cannot look before printing anything: it does some
        # setup, says so, and only then finds the instrument missing.  Quoting
        # `head -n 1` therefore reported the last thing that WORKED as the
        # reason nothing was checked -- `check-shellquote-vs-bash` skipping for
        # want of WSL was announced as "port verified against shellquote.rs",
        # which reads as a success and names the wrong subsystem entirely.
        # The reason a gate gives up is the last thing it says before it does.
        _rc_why=$(grep -v '^[[:space:]]*$' "$_rc_log" 2>/dev/null | tail -n 1)
        [ -n "$_rc_why" ] || _rc_why="(it printed nothing)"
        cat >&2 <<EOF
$_rc_prog: $_rc_label: SKIPPED — it exited $_rc, which this call site declares
means "I could not look". Its last line was

    $_rc_why

This is NOT a pass: nothing about the tree was checked, and a defect of the
kind this gate exists to find would look exactly like what you just saw.
EOF
        if [ -n "${CHECKER_SKIPLOG:-}" ]; then
            printf '%s\t%s\t%s\n' "$_rc_label" "$_rc" "$_rc_why" \
                >>"$CHECKER_SKIPLOG" 2>/dev/null || :
        fi
        rm -f "$_rc_log"
        return 0
    fi

    # The first line of the checker's own output is the discriminator between
    # every reading below, so quote it rather than making the reader open the
    # log to find `Argument list too long` sitting at the top of it.
    _rc_first=$(head -n 1 "$_rc_log" 2>/dev/null)
    [ -n "$_rc_first" ] || _rc_first="(it printed nothing)"

    case "$_rc" in
    126)
        _rc_why="126 is the shell's code for \"found it, could not execute it\", which no
checker in this tree ever returns as a verdict of its own. Something about the
*invocation* failed before any checking happened. Its first line of output was

    $_rc_first

If that reads \"Argument list too long\", it is a hard limit on the size of one
command, not contention: re-running this alone will arrive at the identical
wall, however quiet the machine is. The fix is to batch the arguments (see
scripts/check-doc-links.py, which does), not to retry. Other causes at 126 are
a missing execute bit and a bad interpreter line."
        ;;
    127)
        _rc_why="127 is the shell's code for \"could not run it at all\", which no checker in
this tree ever returns as a verdict of its own. On this host it has two quite
different causes, and its first line of output tells them apart:

    $_rc_first

If that names a command or file as not found, the invocation is wrong — a
typo, or a tool that is not installed — and re-running it unchanged will fail
identically. If instead it names a resource, or says nothing at all, this is
the other cause: a fork() refused by the Windows commit limit looks exactly
like 127 from out here (scripts/boot-history.py, HARNESS_ABORT_EXITS). That
one *is* contention, and re-running it alone is the right move."
        ;;
    *)
        _rc_why="On this host the usual cause is a second rustc-heavy job running concurrently
and exhausting memory; see known-issues.md ->
B-TOOLING-INTERMITTENT-HOST-FAILURES-LOSE-THEIR-OWN-EVIDENCE. Re-run it alone
before concluding anything."
        ;;
    esac

    cat >&2 <<EOF

$_rc_prog: REFUSING to ${CHECKER_REFUSING:-continue} — the $_rc_label checker never reached a verdict.

It exited $_rc, so it did not say whether the tree is clean or not. This is
NOT a finding against your code, and nothing printed above it is one either.
${CHECKER_NOTE:-}
Its full output is at
    $_rc_log
and re-running it directly is the next step:
    $_rc_cmd

$_rc_why

EOF
    exit 1
}
