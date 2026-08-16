#!/usr/bin/env bash
# Regression test for `boot-test.sh`'s cross-worktree boot lock.
#
# Why this exists as a separate script: the acquire loop lives *after* the
# kernel build in `boot-test.sh`, so reaching it through a normal run costs a
# full build (~7 min) per case, and the cases that matter need a second lane
# holding the lock at a controlled age with a controlled pid. None of that is
# reachable from an ordinary boot test, which is why the lock has historically
# been the least-tested part of the script and has now carried two real bugs:
#
#   - a lock whose owner died was held until the 1200s age breaker fired,
#     stalling the next lane for up to 20 minutes
#     (requests/b-a-boot-lock-survives-its-dead-owner.md);
#   - a lock with no owner file yet — the window between the `mkdir` that
#     acquires and the write that stamps it — was scored as age 999999 and so
#     broken *immediately* by that same age breaker, which could put two lanes
#     in QEMU at once;
#   - acquisition was a bare `mkdir` race with no queue, so a lane that
#     released and immediately re-ran beat a waiter every time and could starve
#     it indefinitely
#     (requests/b-a-the-boot-lock-has-no-queue-so-a-waiting-lane-can-starve.md).
#
# Rather than restate the logic here (a copy would drift from the original and
# then test nothing), the region between the BOOT-LOCK-REGION markers in
# `boot-test.sh` is extracted verbatim and executed. If someone deletes or
# renames those markers this script fails loudly instead of silently passing.
#
# Usage: scripts/test-boot-lock.sh        (no arguments; ~1 second)
# Exit:  0 all cases pass, 1 otherwise.

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BOOT_TEST="$SCRIPT_DIR/boot-test.sh"

TMPROOT="$(mktemp -d)"
trap 'rm -rf "$TMPROOT"' EXIT

failures=0
pass() { echo "  ok   - $1"; }
fail() { echo "  FAIL - $1"; echo "         $2"; failures=$(( failures + 1 )); }

# ---------------------------------------------------------------------------
# Extract the region under test, verbatim.
# ---------------------------------------------------------------------------
REGION="$TMPROOT/region.sh"
awk '/^# --- BEGIN BOOT-LOCK-REGION ---$/{f=1;next} /^# --- END BOOT-LOCK-REGION ---$/{f=0} f' \
    "$BOOT_TEST" > "$REGION"
if [ ! -s "$REGION" ]; then
    echo "FAIL: could not extract BOOT-LOCK-REGION from $BOOT_TEST"
    echo "      (markers missing or renamed — the lock logic is now untested)"
    exit 1
fi
# Sanity-check that we extracted the thing we think we did, so a future edit
# that moves the acquire loop out of the region can't quietly pass everything.
for _needle in 'mkdir "$BOOT_LOCK_DIR"' 'release_boot_lock()' 'is gone' \
               'Breaking stale boot lock' '_boot_lock_head' '_boot_lock_sweep_tickets' \
               'refusing to boot alongside it'; do
    if ! grep -qF "$_needle" "$REGION"; then
        echo "FAIL: extracted region is missing '$_needle' — markers no longer"
        echo "      bracket the boot-lock acquire logic."
        exit 1
    fi
done
echo "extracted $(wc -l < "$REGION") lines of boot-lock logic from boot-test.sh"

# ---------------------------------------------------------------------------
# Harness: run the region against a lock dir we control.
#
# BOOT_LOCK_WAIT=0 makes the give-up branch fire on the first poll, so a case
# that does not acquire returns immediately instead of sleeping. The branch
# order inside the loop is acquire, then break-checks, then expiry, so this
# cleanly separates "took it", "broke it" and "yielded to it".
#
# The give-up branch has two endings and both are load-bearing: a provably-live
# owner gets a refusal (`exit 4`), anything else still boots anyway. Cases below
# assert which one fired, so `run_region` returns the region's exit status too.
# ---------------------------------------------------------------------------
LAST_STATUS=0
# Note the `return "$status"` rather than setting LAST_STATUS here: callers
# invoke this inside `$( )`, which is its own subshell, so any variable this
# function assigns is discarded on the way out. Only the exit status survives.
run_region() {  # $1 = lock dir, $2 = optional BOOT_LOCK_WAIT (default 0)
    local out status
    out="$(
        (
            set -u
            PROJECT_ROOT="$PROJECT_ROOT"
            BOOT_LOCK_DIR="$1"
            BOOT_LOCK_OWNER=""
            BOOT_LOCK_WAIT="${2:-0}"
            # shellcheck disable=SC1090
            . "$REGION"
        ) 2>&1
    )"
    status=$?
    printf '%s\n' "$out"
    return "$status"
}

# Plant a foreign waiter's ticket: `<epoch>-<pid>` in the queue directory beside
# the lock. Age and identity live in the *name*, which is exactly how the region
# reads them, so no `touch -d` is needed.
plant_ticket() {  # $1 = lock dir, $2 = age seconds, $3 = pid
    mkdir -p "$1.waiters"
    : > "$1.waiters/$(( $(date +%s) - $2 ))-$3"
}

queue_is_empty() {  # $1 = lock dir
    [ -z "$(ls -A "$1.waiters" 2>/dev/null || true)" ]
}

# Every case starts from a clean lock *and* a clean queue; a ticket left by an
# earlier case would silently change the next one's head-of-queue.
fresh() {  # $1 = lock dir
    rm -rf "$1" "$1.waiters"
}

# A pid that is certainly dead: spawn one and reap it.
dead_pid() { ( : ) & local p=$!; wait "$p" 2>/dev/null; echo "$p"; }

new_lock() {  # $1 = dir, $2 = owner string or "" for none, $3 = age seconds
    rm -rf "$1" "$1.waiters"; mkdir -p "$1"
    local when; when="@$(( $(date +%s) - $3 ))"
    if [ -n "$2" ]; then
        printf '%s\n' "$2" > "$1/owner"
        touch -d "$when" "$1/owner"
    fi
    touch -d "$when" "$1"
}

check() {  # $1 = label, $2 = haystack, $3 = needle
    case "$2" in
        *"$3"*) pass "$1" ;;
        *) fail "$1" "expected output to contain: $3" ;;
    esac
}
check_not() {
    case "$2" in
        *"$3"*) fail "$1" "output must NOT contain: $3" ;;
        *) pass "$1" ;;
    esac
}

DEAD="$(dead_pid)"
if kill -0 "$DEAD" 2>/dev/null; then
    echo "FAIL: harness could not produce a dead pid (got $DEAD, still alive)"
    exit 1
fi
# A live holder, for the cases that must NOT break.
sleep 120 & LIVE=$!
trap 'kill "$LIVE" 2>/dev/null; rm -rf "$TMPROOT"' EXIT

echo
echo "== case 1: free lock is acquired =="
L="$TMPROOT/l1"; fresh "$L"
out="$(run_region "$L")"
check "acquires when free" "$out" "Boot lock acquired:"

echo
echo "== case 2: lock held by a LIVE owner is not broken, and we refuse to boot =="
# Giving up used to mean "boot anyway", which on a provably-live owner starts a
# second QEMU on the host — the exact outcome the lock exists to prevent,
# arrived at an hour later and then blamed on the code under test.
L="$TMPROOT/l2"; new_lock "$L" "lane-B/pid-$LIVE/1" 300
out="$(run_region "$L")"; LAST_STATUS=$?
check_not "does not dead-break a live owner" "$out" "is gone"
check_not "does not age-break a live owner" "$out" "Breaking stale boot lock"
check "refuses rather than booting alongside" "$out" "refusing to boot alongside it"
check_not "and specifically does not boot anyway" "$out" "booting anyway"
if [ "$LAST_STATUS" -eq 4 ]; then
    pass "exits 4 so a caller can tell contention from a kernel failure"
else
    fail "exits 4 so a caller can tell contention from a kernel failure" "got status $LAST_STATUS"
fi
if [ "$(cat "$L/owner" 2>/dev/null)" = "lane-B/pid-$LIVE/1" ]; then
    pass "live owner's lock survives untouched"
else
    fail "live owner's lock survives untouched" "lock dir or owner file was modified"
fi
if queue_is_empty "$L"; then
    pass "the refusing waiter drops its ticket on the way out"
else
    fail "the refusing waiter drops its ticket on the way out" "queue: $(ls "$L.waiters" 2>/dev/null)"
fi

echo
echo "== case 2b: a LIVE owner past 1200s is still not broken =="
# The age rule predates the liveness check and used age as the only available
# proxy for death. A boot that outruns the 1200s estimate is slow, not dead,
# and breaking its lock puts two QEMUs on one host — worse than waiting.
L="$TMPROOT/l2b"; new_lock "$L" "lane-B/pid-$LIVE/1" 3000
out="$(run_region "$L")"
check_not "age rule does not override proven liveness" "$out" "Breaking stale boot lock"
check_not "and it is not dead-broken either" "$out" "is gone"
check "so it refuses rather than joining the slow-but-live owner" "$out" "refusing to boot alongside it"
if [ "$(cat "$L/owner" 2>/dev/null)" = "lane-B/pid-$LIVE/1" ]; then
    pass "slow live owner keeps its lock"
else
    fail "slow live owner keeps its lock" "a live lane's lock was stolen"
fi

echo
echo "== case 3: lock held by a DEAD owner is broken (the reported bug) =="
L="$TMPROOT/l3"; new_lock "$L" "lane-B/pid-$DEAD/1" 300
out="$(run_region "$L")"
check "breaks a dead owner's lock" "$out" "owner pid $DEAD is gone"
check "and then acquires it" "$out" "Boot lock acquired:"
check_not "without waiting out the 1200s age rule" "$out" "Breaking stale boot lock"

echo
echo "== case 4: a DEAD owner younger than the 60s floor is left alone =="
L="$TMPROOT/l4"; new_lock "$L" "lane-B/pid-$DEAD/1" 10
out="$(run_region "$L")"
check_not "respects the liveness grace floor" "$out" "is gone"
check "yields instead" "$out" "booting anyway"

echo
echo "== case 5: a lock with no owner file yet is NOT broken (regression) =="
# This is the mkdir-then-write window. Before the fix the missing owner file
# scored as age 999999, so this case broke a lock that had just been taken.
L="$TMPROOT/l5"; new_lock "$L" "" 2
out="$(run_region "$L")"
check_not "does not break a just-acquired lock" "$out" "Breaking stale boot lock"
check_not "and does not dead-break it either" "$out" "is gone"
check "yields to it" "$out" "booting anyway"
if [ -d "$L" ]; then pass "the young ownerless lock still exists"
else fail "the young ownerless lock still exists" "it was deleted"; fi

echo
echo "== case 6: a genuinely old lock still falls to the age breaker =="
L="$TMPROOT/l6"; new_lock "$L" "" 5000
out="$(run_region "$L")"
check "age rule still breaks a truly stale lock" "$out" "Breaking stale boot lock"
check "and then acquires it" "$out" "Boot lock acquired:"

echo
echo "== case 7: an unparseable owner falls through to the age rule =="
L="$TMPROOT/l7"; new_lock "$L" "garbage-with-no-pid-field" 300
out="$(run_region "$L")"
check_not "no break on an owner we cannot parse" "$out" "is gone"
check_not "and not old enough for the age rule" "$out" "Breaking stale boot lock"
check "so it yields" "$out" "booting anyway"

echo
echo "== case 8: release after yielding never touches the holder's lock =="
# The dangerous shape: we tried to acquire, gave up, and then ran our exit
# path anyway. `release_boot_lock` must be a no-op — the lock it can see is
# someone else's. (BOOT_LOCK_OWNER cannot be preset from the harness: the
# region assigns it, which is itself part of what makes release safe.)
#
# The holder here is dead-but-young rather than live, so the region *returns*
# (yielding via "booting anyway") instead of exiting 4, which is what lets the
# explicit `release_boot_lock` below run at all. The live-owner variant is
# covered by case 2, where the same guarantee is enforced by the EXIT trap.
L="$TMPROOT/l8"; new_lock "$L" "lane-B/pid-$DEAD/1" 10
(
    set -u
    PROJECT_ROOT="$PROJECT_ROOT"
    BOOT_LOCK_DIR="$L"
    BOOT_LOCK_OWNER=""
    BOOT_LOCK_WAIT=0
    # shellcheck disable=SC1090
    . "$REGION"
    release_boot_lock
) >/dev/null 2>&1
if [ "$(cat "$L/owner" 2>/dev/null)" = "lane-B/pid-$DEAD/1" ]; then
    pass "release_boot_lock leaves another lane's lock alone"
else
    fail "release_boot_lock leaves another lane's lock alone" "it deleted a foreign lock"
fi

echo
echo "== case 9: after acquiring, release removes our own lock =="
# The other half of case 8: release must actually work, or every run leaks a
# lock and the next lane waits out the age rule on a lock nobody holds.
L="$TMPROOT/l9"; fresh "$L"
(
    set -u
    PROJECT_ROOT="$PROJECT_ROOT"
    BOOT_LOCK_DIR="$L"
    BOOT_LOCK_OWNER=""
    BOOT_LOCK_WAIT=0
    # shellcheck disable=SC1090
    . "$REGION"
    release_boot_lock
) >/dev/null 2>&1
if [ ! -d "$L" ]; then pass "our own lock is released on exit"
else fail "our own lock is released on exit" "lock dir survived: $(cat "$L/owner" 2>/dev/null)"; fi

# ---------------------------------------------------------------------------
# Cases 10-15: the ticket queue.
#
# These are the starvation half. Cases 1-9 all ask "may I break this lock?";
# these ask "will I ever get a turn?" — the question the earlier round did not
# reach, and the one that had lane B watching five consecutive lane A runs hold
# the lock across forty minutes without once winning the `mkdir`.
# ---------------------------------------------------------------------------

echo
echo "== case 10: a free lock is NOT taken while an older waiter is queued =="
# This is the reported bug, reduced: lane A has just released (so the lock is
# free) and immediately re-run, while lane B has been queued the whole time.
# Under the old bare-`mkdir` race lane A wins here, every time, forever.
#
# BOOT_LOCK_WAIT=1 rather than 0 so the loop completes one full poll before
# giving up: the progress message is printed after the expiry check, so a
# zero-second budget would skip the very line this case is asserting. Costs one
# 5s sleep, and is the only case in this file that sleeps at all.
L="$TMPROOT/l10"; fresh "$L"
plant_ticket "$L" 300 "$LIVE"
out="$(run_region "$L" 1)"; LAST_STATUS=$?
check_not "does not barge past an older live ticket" "$out" "Boot lock acquired:"
check "reports what it is queued behind" "$out" "queued behind ticket"
if [ ! -d "$L" ]; then pass "and takes no lock at all"
else fail "and takes no lock at all" "the lock dir was created anyway"; fi
# The lock is *free* here, so the owner-liveness test says nothing. Refusing
# still has to happen, because the live waiter at the head of the queue is the
# process most likely to be in QEMU seconds from now.
check "refuses rather than booting beside the head waiter" "$out" "refusing to boot alongside it"
check_not "specifically, it does not boot anyway" "$out" "booting anyway"
if [ "$LAST_STATUS" -eq 4 ]; then
    pass "starvation cannot escalate into two concurrent QEMUs"
else
    fail "starvation cannot escalate into two concurrent QEMUs" "got status $LAST_STATUS"
fi

echo
echo "== case 11: being at the head of the queue is what grants the lock =="
# Same free lock, but now the other ticket is *younger* than ours. The
# difference between this case and case 10 is the entire fairness property.
L="$TMPROOT/l11"; fresh "$L"
mkdir -p "$L.waiters"; : > "$L.waiters/9999999999-$LIVE"
out="$(run_region "$L")"
check "the oldest ticket acquires" "$out" "Boot lock acquired:"
check_not "and is not held up by a later one" "$out" "queued behind ticket"

echo
echo "== case 12: a DEAD waiter's ticket is swept, not obeyed =="
# A run killed by run-timeout.py's Job Object executes no exit path, so it
# leaves its ticket behind. A dead ticket at the head of the queue would block
# every lane — a worse failure than the starvation the queue was added to fix.
L="$TMPROOT/l12"; fresh "$L"
plant_ticket "$L" 300 "$DEAD"
out="$(run_region "$L")"
check "sweeps the dead waiter" "$out" "waiter pid $DEAD is gone"
check "and then acquires" "$out" "Boot lock acquired:"

echo
echo "== case 13: a DEAD waiter's ticket younger than 60s is left alone =="
# Same grace floor as the lock's own liveness breaker, for the same reason: a
# pid that cannot be seen is only evidence of death once it has had time to
# become visible. One rule, not two.
L="$TMPROOT/l13"; fresh "$L"
plant_ticket "$L" 10 "$DEAD"
out="$(run_region "$L")"
check_not "respects the ticket grace floor" "$out" "is gone"
check_not "so it does not acquire yet" "$out" "Boot lock acquired:"

echo
echo "== case 14: an unparseable ticket is discarded =="
# It sorts to the front (`sort -n` reads a non-number as 0), so leaving it in
# place would wedge the queue permanently.
L="$TMPROOT/l14"; fresh "$L"
mkdir -p "$L.waiters"; : > "$L.waiters/not-a-ticket"
out="$(run_region "$L")"
check "drops a ticket it cannot parse" "$out" "Dropping unparseable boot-lock ticket"
check "and then acquires" "$out" "Boot lock acquired:"

echo
echo "== case 15: acquiring removes our own ticket =="
# Otherwise every run leaves its ticket behind and the queue becomes a graveyard
# that the next lane has to wait out via the sweep.
L="$TMPROOT/l15"; fresh "$L"
out="$(run_region "$L")"
check "acquires" "$out" "Boot lock acquired:"
if queue_is_empty "$L"; then pass "the queue is empty afterwards"
else fail "the queue is empty afterwards" "left behind: $(ls "$L.waiters" 2>/dev/null)"; fi

echo
if [ "$failures" -eq 0 ]; then
    echo "=== boot-lock tests PASSED ==="
    exit 0
fi
echo "=== boot-lock tests FAILED ($failures) ==="
exit 1
