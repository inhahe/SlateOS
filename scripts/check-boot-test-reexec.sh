#!/usr/bin/env bash
# Prove the boot-test re-exec actually makes a run immune to a mid-run edit.
#
# This does NOT run boot-test.sh -- it runs a miniature script carrying the
# *same* re-exec preamble, lifted out of boot-test.sh by `sed` rather than
# retyped, so the property can be tested in two seconds instead of ninety
# minutes and cannot drift away from the thing it claims to test.
#
# Safe to run while a real boot test is in flight: the leak check below
# compares the snapshots present before and after, so another run's live
# snapshot is not mistaken for one this check leaked.
#
# The control matters as much as the test: without the preamble the same edit
# must visibly corrupt the run, otherwise the test proves nothing about bash's
# seek-back behaviour and would pass against a no-op preamble.
set -uo pipefail

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Snapshots already in flight belong to somebody else's boot test.  Recorded
# before anything runs, so the leak check at the end asks "did *we* leave one"
# rather than "is there one", which would fail whenever a real run overlapped.
snapshots_before="$(find "${TMPDIR:-/tmp}" -maxdepth 1 -name 'boot-test-snapshot.*' 2>/dev/null | wc -l)"

# The payload both variants run: announce, sleep long enough for the editor to
# land, then announce again.  The second line is the one an edit can corrupt,
# because bash has not read that far when the sleep starts.
payload='echo "PHASE-1"
sleep 2
echo "PHASE-2"
'

# --- Control: no preamble.  A mid-run edit must be seen. --------------------
printf '#!/usr/bin/env bash\n%s' "$payload" > "$tmp/control.sh"
chmod +x "$tmp/control.sh"
( sleep 1; printf '#!/usr/bin/env bash\necho "PHASE-1"\nsleep 2\necho "CLOBBERED"\n' > "$tmp/control.sh" ) &
control_out="$(bash "$tmp/control.sh" 2>&1)"
wait

# --- Test: with the preamble lifted verbatim from boot-test.sh. -------------
# Extracted from the real file rather than retyped, so this cannot drift away
# from what boot-test.sh actually does.
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
sed -n '/^if \[ -z "${BOOT_TEST_REEXEC:-}" \]; then$/,/^fi$/p' \
    "$repo/scripts/boot-test.sh" > "$tmp/preamble.sh"
if [ ! -s "$tmp/preamble.sh" ]; then
    echo "FAIL: could not extract the re-exec preamble from boot-test.sh"
    echo "      (it was renamed or reshaped -- this checker is now lying)"
    exit 1
fi
{
    printf '#!/usr/bin/env bash\nset -euo pipefail\n'
    cat "$tmp/preamble.sh"
    printf '%s' "$payload"
} > "$tmp/guarded.sh"
chmod +x "$tmp/guarded.sh"
( sleep 1; printf '#!/usr/bin/env bash\necho "PHASE-1"\nsleep 2\necho "CLOBBERED"\n' > "$tmp/guarded.sh" ) &
# The preamble is lifted as a *block*, and a block does not bring its
# prerequisites with it: `BOOT_TEST_START_EPOCH` is set at the top of
# boot-test.sh, well above the `if`, and the preamble forwards it to the
# re-exec unguarded.  Under `set -u` -- which the guarded script sets
# because boot-test.sh does -- an unset one aborts before the payload runs.
#
# Supplied through the environment rather than by editing the lifted text,
# so the thing under test stays byte-identical to what boot-test.sh runs.
# The value is arbitrary; nothing in the preamble reads it, it only
# forwards it.
# `env -u BOOT_TEST_REEXEC` is what makes this valid when the boot test is
# the caller, and it is the whole test when it is.  The preamble's first act
# is `if [ -z "${BOOT_TEST_REEXEC:-}" ]`, and boot-test.sh re-execs itself
# with that variable set to 1 -- which every child inherits, this checker
# included.  Left in place, the guarded script takes the already-re-exec'd
# branch, skips the snapshot entirely, and runs from disk: precisely the
# unprotected behaviour the control is supposed to be the only example of.
#
# It does not then look like a failure of the guard, which is why this cost
# a boot test to find.  The edit truncates the file, bash's byte offset into
# it lands past the new end, and the script stops after PHASE-1 -- printing
# neither PHASE-2 nor CLOBBERED, i.e. the "inconclusive" arm rather than the
# "snapshot did not isolate" one.
#
# So: the gate was inert in the one context it was wired into.  Cleared here
# rather than at the call site, so the checker is correct however it is run.
guarded_out="$(env -u BOOT_TEST_REEXEC BOOT_TEST_START_EPOCH="$(date +%s)" bash "$tmp/guarded.sh" 2>&1)"
wait

fails=0
echo "--- control (no preamble): the edit SHOULD be visible ---"
echo "$control_out" | sed 's/^/    /'
if echo "$control_out" | grep -q "CLOBBERED"; then
    echo "ok   the unprotected script really did execute the edit"
else
    echo "INCONCLUSIVE: the unprotected script did not pick up the edit either."
    echo "  Nothing below is evidence -- the harness failed to reproduce the"
    echo "  hazard, so the guarded run passing says only that nothing happened."
    fails=$((fails + 1))
fi

echo
echo "--- guarded (boot-test.sh preamble): the edit MUST NOT be visible ---"
echo "$guarded_out" | sed 's/^/    /'
if echo "$guarded_out" | grep -q "CLOBBERED"; then
    echo "FAIL the snapshot did not isolate the run"
    fails=$((fails + 1))
elif echo "$guarded_out" | grep -q "PHASE-2"; then
    echo "ok   the run finished from the snapshot, edit and all"
elif echo "$guarded_out" | grep -q "unbound variable"; then
    # Not a property failure -- the harness could not run the preamble at all.
    # Reporting "the re-exec is broken" here would be a verdict about something
    # that never executed, which is the one thing a gate must not do.
    #
    # This is how the lift really drifts.  The `sed` range keeps the block
    # honest, but a variable the block *reads* can be added above it, and then
    # the extraction is still faithful while the extracted script no longer
    # runs.  That is exactly what BOOT_TEST_START_EPOCH did, and it went
    # unnoticed because nothing ran this checker.  Name the variable, so the
    # next one costs minutes instead of a bisect.
    echo "FAIL the preamble needs a variable this harness does not supply:"
    echo "$guarded_out" |
        sed -n 's/.*: \([A-Za-z_][A-Za-z0-9_]*\): unbound variable.*/       \1/p' |
        sort -u
    echo "     Supply it next to BOOT_TEST_START_EPOCH above.  This is a stale"
    echo "     harness, NOT evidence that the re-exec guard is broken."
    fails=$((fails + 1))
else
    echo "FAIL the guarded run reached neither PHASE-2 nor CLOBBERED"
    fails=$((fails + 1))
fi

# --- The snapshot must not be left behind. ----------------------------------
echo
snapshots_after="$(find "${TMPDIR:-/tmp}" -maxdepth 1 -name 'boot-test-snapshot.*' 2>/dev/null | wc -l)"
leaked=$((snapshots_after - snapshots_before))
if [ "$leaked" -le 0 ]; then
    echo "ok   no snapshot left in ${TMPDIR:-/tmp} -- the EXIT trap fired"
    if [ "$snapshots_before" -gt 0 ]; then
        echo "     ($snapshots_before belonging to another run were ignored)"
    fi
else
    echo "FAIL $leaked snapshot(s) left behind; the trap did not fire"
    fails=$((fails + 1))
fi

echo
echo "$fails failure(s)"
exit $((fails > 0))
