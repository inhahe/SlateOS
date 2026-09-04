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
# Should a checker ever print the traceback banner as part of a legitimate
# finding, it lands in the no-verdict arm too, and the run is still refused —
# the misclassification costs a confusing message, never a missed defect.
#
# ## `--may-skip`: the fourth outcome, and why it is opt-in per call site
#
# Some gates are right to decline. `check-libc-shape.py` grades a build
# artifact and returns 2 when `libc.a` is stale; the four `check-*-vs-bash.py`
# oracles ask real bash through WSL and cannot ask at all on a host without it.
# For those, "the checker did not reach a verdict" is not a defect in the tree
# and not a reason to refuse the build — but under the rule above it aborts, so
# five such gates sat pinned as unwired, i.e. never asked even on the hosts
# where they *would* have worked. That is the worse failure: a gate nothing
# runs enforces nothing.
#
# The loosening is **per call site, never global**, at lane A's explicit
# request, and the reason is that the abort-on-2 rule is load-bearing for
# everything else. A floor — "this run inspected fewer files than it must have"
# — reports 2 as well, and for a floor, aborting is exactly right. A global
# "treat 2 as skip" would convert every floor in the tree into a shrug. So the
# flag names the *skip* case at the one call site that has established the
# right to it, and an unflagged 2 keeps aborting exactly as before.
#
# Four conditions, all required, because a skip that nobody notices is the
# thing this whole file exists to prevent:
#
# 1. **The call site passed `--may-skip`.** Whoever wired the gate asserted that
#    this gate has a legitimate can't-answer.
# 2. **No Python traceback.** A crash that happens to exit 2 is a crash. Without
#    this, adding the flag to a call site would also silence its crashes, and
#    the flag would be a bypass rather than a channel.
# 3. **No `usage:` banner.** This one is not obvious and is the reason the flag
#    is nearly a footgun: **`argparse` exits 2 for a usage error.** So a call
#    site carrying `--may-skip` whose invocation later grows a typo — a renamed
#    flag, an argument that stopped being accepted — would exit 2, print, and be
#    read as a legitimate decline. The gate would then skip on *every* host,
#    forever, reporting a reason that is really argparse complaining. Nothing
#    else in the run would say so. argparse's first line is `usage: …`, which
#    makes it as cheaply detectable as the traceback banner, and is caught the
#    same way for the same reason.
# 4. **The line this would quote as the reason is not blank.** A skip is a claim
#    — "I could not answer, and here is why" — and a claim with no evidence is
#    indistinguishable from a gate that silently did nothing, which is precisely
#    the shape being guarded against. A silent exit 2 is therefore *not* a skip;
#    it takes the no-verdict arm and aborts, flag or no flag.
#
#    Note what is tested: *the reason*, not the log. Those are not the same
#    claim, and testing the log instead — `[ -s "$_rc_log" ]`, which is what this
#    did until 2026-09-03 — lets a checker whose first line is blank or
#    whitespace skip while quoting `(it printed nothing)`, a sentence this file
#    wrote, as though the checker had said it. The blank first line is not a
#    hypothetical: with a merged stdout+stderr log and a block-buffered stdout it
#    is what a Python checker routinely produces. Both halves are fixed —
#    `PYTHONUNBUFFERED` stops the reordering, this stops it mattering — because
#    each alone leaves a way in.
#
# A skip returns **0**, so the call site's `if ! run_checker …` keeps its shape,
# and sets `RUN_CHECKER_SKIPPED` (with `RUN_CHECKER_SKIP_REASON` carrying the
# checker's first line) so a caller that reports "N gates ran" can report "and M
# skipped" truthfully rather than counting a skip as a pass. Both are cleared at
# the top of *every* call, including calls without the flag: a stale flag from
# three gates ago is how a skip gets attributed to the wrong gate.
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

# run_checker [--may-skip] <label> <command> [args...]
run_checker() {
    # Cleared on every call, flagged or not — see the header on stale state.
    # Both are the *outward* channel: this file only ever writes them, and the
    # caller reads them, which is why shellcheck cannot see a use.
    # shellcheck disable=SC2034
    RUN_CHECKER_SKIPPED=
    # shellcheck disable=SC2034
    RUN_CHECKER_SKIP_REASON=
    _rc_may_skip=
    if [ "$1" = "--may-skip" ]; then
        _rc_may_skip=1
        shift
    fi
    _rc_label=$1
    shift
    # A miscall is worth catching here rather than downstream. `"$@"` with
    # nothing in it runs nothing and reports success, so `run_checker --may-skip
    # libc-shape` -- the flag typed where the label goes, or a command that got
    # word-split away -- would be a gate that passes without existing. That is
    # the same green-report-without-a-check failure the rest of this file is
    # about, so it aborts rather than returning.
    if [ "$#" -eq 0 ]; then
        echo "${CHECKER_PROG:-checker}: run_checker: no command given for gate '$_rc_label'." >&2
        echo "usage: run_checker [--may-skip] <label> <command> [args...]" >&2
        exit 1
    fi
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
    # `$*` joins on IFS's first character, and one caller (the hook's doc-links
    # gate) sets IFS to a newline to split a path list — which would render the
    # "re-run it" line one argument per line. Build the string under a known IFS.
    _rc_oldifs=$IFS
    IFS=' '
    _rc_cmd="$*"
    IFS=$_rc_oldifs

    # PYTHONUNBUFFERED is set on the child, not exported into the caller, and it
    # is here rather than as a `-u` at each call site because the correctness of
    # the evidence must not depend on every future caller remembering a flag.
    #
    # The two streams below are merged into one file, and they do not arrive in
    # the order they were written: a redirected stdout is block-buffered while
    # stderr is not, so a checker's stdout sits in a buffer until it exits and
    # lands *after* everything its stderr said in the meantime. That is not a
    # cosmetic reordering here -- `_rc_first` below quotes the first line of this
    # file as the reason a gate declined, so the buffering decides which sentence
    # gets quoted. Observed: `scan-unwrap.py` printing its reason to stdout and
    # its explanation to stderr, and `head -n 1` returning a blank line.
    #
    # (This was fixed once already on lane A and lost in merge a29a07d68, which
    # adopted lane B's --may-skip implementation wholesale -- correctly, but the
    # buffering fix rode along in the file that was replaced. It is restored here
    # rather than in the caller so it cannot be lost the same way twice.)
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

    # The first line of the checker's own output is the discriminator between
    # every reading below, so quote it rather than making the reader open the
    # log to find `Argument list too long` sitting at the top of it.
    _rc_first=$(head -n 1 "$_rc_log" 2>/dev/null | tr -d '\r')
    # Keep the raw reason apart from the display form. Condition 4 in the header
    # -- "the checker printed something" -- is a claim about *the sentence this
    # would quote*, not about whether the file has bytes in it, and the two come
    # apart whenever the first line is blank or whitespace: `[ -s ]` says the log
    # is non-empty, the substitution below then hands the skip arm the reason
    # "(it printed nothing)", and a gate skips on every host quoting a sentence
    # this file wrote about it. Testing the reason instead closes that gap at the
    # place the claim is actually made.
    _rc_reason=$_rc_first
    case $_rc_reason in
    *[![:space:]]*) ;;
    *) _rc_reason= ;;
    esac
    [ -n "$_rc_reason" ] || _rc_first="(it printed nothing)"

    # The declined-verdict arm. All four conditions are checked here rather
    # than folded into one test so that a future reader can see which one a
    # given call failed: the empty-output and `usage:` cases in particular fall
    # through to the no-verdict message below *on purpose*, and that is not
    # obvious from a single `&&` chain. See the header for why a `usage:`
    # banner disqualifies a skip -- argparse exits 2 too.
    if [ -n "$_rc_may_skip" ] && [ "$_rc" = "2" ] &&
       ! grep -q '^Traceback (most recent call last):' "$_rc_log" &&
       ! grep -q '^usage: ' "$_rc_log" &&
       [ -n "$_rc_reason" ]; then
        # shellcheck disable=SC2034  # outward channel; see the top of the function
        RUN_CHECKER_SKIPPED=1
        # shellcheck disable=SC2034
        RUN_CHECKER_SKIP_REASON=$_rc_first
        rm -f "$_rc_log"
        # Loud, and on its own line, because the one thing a skip must never do
        # is read like a pass in a scrollback of twenty-eight gates. The reason
        # is quoted inline for the same purpose the 126/127 arms quote it: the
        # log is deleted here (a skip is an expected outcome on hosts that lack
        # the tool, and one file per skipped gate per run is litter), so the
        # transcript is where the evidence has to live.
        echo "$_rc_prog: SKIPPED $_rc_label -- it declined to answer, and this call site allows that." >&2
        echo "$_rc_prog:   reason: $_rc_first" >&2
        echo "$_rc_prog:   nothing was checked here. This is not a pass." >&2
        return 0
    fi

    case "$_rc" in
    2)
        # Reached only when the skip arm above declined the case, so this text
        # has to cover both shapes: no flag, or the flag with nothing printed.
        _rc_why="2 is this tree's code for \"I did not reach a verdict\" -- a checker saying it
could not judge, rather than judging. Its first line of output was

    $_rc_first

There are three ways to be here, and that line tells them apart.

If it begins \"usage:\", the invocation is wrong -- argparse exits 2 for a bad
flag or a missing argument. Nothing was checked, and no host will behave any
differently: fix the call site. (This case is deliberately never treated as a
skip, even where skipping is allowed, because a typo in a wired gate would
otherwise silence it on every host forever.)

If the checker named something it needed and could not get -- a build artifact,
WSL, a baseline file -- then it behaved correctly and this call site simply does
not allow it to decline. If that gate is one that legitimately cannot answer on
some hosts, pass --may-skip before its label and it will skip loudly instead of
stopping the run. Do NOT add that flag to silence a gate whose inputs *should*
have been there: for a floor -- \"I inspected fewer files than I must have\" --
exiting 2 is the whole point and aborting is the correct response.

If instead it printed nothing at all, that is why it is here even if the call
site did pass --may-skip: a skip has to say what it could not do. A checker that
exits 2 in silence is indistinguishable from one that did nothing, so it is
never taken as a skip. Fix the checker to explain itself."
        ;;
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
