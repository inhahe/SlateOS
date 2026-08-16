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
#     in QEMU at once.
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
for _needle in 'while ! mkdir' 'release_boot_lock()' 'is gone' 'Breaking stale boot lock'; do
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
# BOOT_LOCK_WAIT=0 makes the "give up and boot anyway" branch fire on the first
# poll, so a case that does not break the lock returns immediately instead of
# sleeping. The branch order inside the loop is break-checks first, expiry
# second, so this cleanly separates "broke the lock" from "yielded to it".
# ---------------------------------------------------------------------------
run_region() {  # $1 = lock dir
    (
        set -u
        PROJECT_ROOT="$PROJECT_ROOT"
        BOOT_LOCK_DIR="$1"
        BOOT_LOCK_OWNER=""
        BOOT_LOCK_WAIT=0
        # shellcheck disable=SC1090
        . "$REGION"
    ) 2>&1
}

# A pid that is certainly dead: spawn one and reap it.
dead_pid() { ( : ) & local p=$!; wait "$p" 2>/dev/null; echo "$p"; }

new_lock() {  # $1 = dir, $2 = owner string or "" for none, $3 = age seconds
    rm -rf "$1"; mkdir -p "$1"
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
L="$TMPROOT/l1"; rm -rf "$L"
out="$(run_region "$L")"
check "acquires when free" "$out" "Boot lock acquired:"

echo
echo "== case 2: lock held by a LIVE owner is not broken =="
L="$TMPROOT/l2"; new_lock "$L" "lane-B/pid-$LIVE/1" 300
out="$(run_region "$L")"
check_not "does not dead-break a live owner" "$out" "is gone"
check_not "does not age-break a live owner" "$out" "Breaking stale boot lock"
check "yields to the live owner" "$out" "booting anyway"
if [ "$(cat "$L/owner" 2>/dev/null)" = "lane-B/pid-$LIVE/1" ]; then
    pass "live owner's lock survives untouched"
else
    fail "live owner's lock survives untouched" "lock dir or owner file was modified"
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
check "so it waits on the slow-but-live owner" "$out" "booting anyway"
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
L="$TMPROOT/l8"; new_lock "$L" "lane-B/pid-$LIVE/1" 300
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
if [ "$(cat "$L/owner" 2>/dev/null)" = "lane-B/pid-$LIVE/1" ]; then
    pass "release_boot_lock leaves another lane's lock alone"
else
    fail "release_boot_lock leaves another lane's lock alone" "it deleted a foreign lock"
fi

echo
echo "== case 9: after acquiring, release removes our own lock =="
# The other half of case 8: release must actually work, or every run leaks a
# lock and the next lane waits out the age rule on a lock nobody holds.
L="$TMPROOT/l9"; rm -rf "$L"
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

echo
if [ "$failures" -eq 0 ]; then
    echo "=== boot-lock tests PASSED ==="
    exit 0
fi
echo "=== boot-lock tests FAILED ($failures) ==="
exit 1
