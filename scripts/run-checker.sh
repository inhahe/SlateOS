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
#                     not overwrite each other's evidence.
#   CHECKER_REFUSING  the word after "REFUSING to" — `push`, `build`. Default
#                     `continue`.
#   CHECKER_NOTE      one extra paragraph for the no-verdict message, or empty.
#                     The hook uses it to say why the gate's bypass is the wrong
#                     reaction; boot-test has no bypass and leaves it unset.

# run_checker <label> <command> [args...]
run_checker() {
    _rc_label=$1
    shift
    _rc_prog=${CHECKER_PROG:-checker}
    _rc_dir=${CHECKER_LOGDIR:-${TMPDIR:-/tmp}}
    _rc_log="$_rc_dir/$_rc_prog-$_rc_label.log"
    # `$*` joins on IFS's first character, and one caller (the hook's doc-links
    # gate) sets IFS to a newline to split a path list — which would render the
    # "re-run it" line one argument per line. Build the string under a known IFS.
    _rc_oldifs=$IFS
    IFS=' '
    _rc_cmd="$*"
    IFS=$_rc_oldifs

    "$@" >"$_rc_log" 2>&1
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

    cat >&2 <<EOF

$_rc_prog: REFUSING to ${CHECKER_REFUSING:-continue} — the $_rc_label checker never reached a verdict.

It exited $_rc, so it did not say whether the tree is clean or not. This is
NOT a finding against your code, and nothing printed above it is one either.
${CHECKER_NOTE:-}
Its full output is at
    $_rc_log
and re-running it directly is the next step:
    $_rc_cmd

On this host the usual cause is a second rustc-heavy job running concurrently
and exhausting memory; see known-issues.md ->
B-TOOLING-INTERMITTENT-HOST-FAILURES-LOSE-THEIR-OWN-EVIDENCE. Re-run it alone
before concluding anything.

EOF
    exit 1
}
