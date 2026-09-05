#!/usr/bin/env bash
# boot-test.sh — Build the kernel, boot it in QEMU, verify BOOT_OK.
#
# Exit codes:
#   0 — success marker detected AND no self-test failures
#   1 — Timeout, PANIC, or a non-fatal self-test failure detected
#   2 — Wedge: the serial log stopped growing for --stall-secs with the marker
#       still absent (opt-in; distinct from 1 because a wedge is a hang to be
#       debugged with the captured RIP, not a test that reported a failure)
#   3 — The kernel booted cleanly but the run did not produce the artefact it
#       was asked for: --bench was given and the benchmark recorder failed, so
#       nothing was written to bench/history.jsonl.  Distinct from 1 because the
#       fault is in our tooling, not in the kernel.
#   4 — Gave up waiting for the cross-worktree boot lock after BOOT_LOCK_WAIT
#       seconds while another lane's run was *provably alive* holding it.  The
#       kernel was built but never booted, so this says nothing at all about the
#       code under test — it is the one status where retrying unchanged is the
#       right response.
#   5 — Gave up waiting for the host's *commit charge* to fall far enough to
#       run (see check_commit_headroom).  Nothing was built and nothing was
#       booted.  Like 4 it says nothing about the code under test, and like 4
#       the right response is to retry later: the cause is another lane's
#       build, and it clears on its own.
#   6 — A build tool crashed instead of reporting a finding: clippy-driver or
#       rustc died on a signal or an NTSTATUS rather than exiting with a verdict.
#       Like 4 and 5 this says nothing about the code under test.  It is listed
#       separately because the *evidence* differs: 4 and 5 are this script
#       declining to start, whereas 6 is a gate that ran, produced no judgement,
#       and must not be read as having produced a clean one.
#       Unlike 4 and 5, retrying is NOT the indicated response.  A crash that
#       host memory could explain — commit headroom below the floor at the
#       moment of death — is waited out and retried *inside* the gate, and
#       becomes a 5 if the host never recovers.  So a 6 that reaches the caller
#       has already survived that filter: it crashed with memory to spare, or
#       twice.  Investigate the toolchain; do not re-run expecting better.
#
# 2 and 3 are listed here because they were not: exit 2 has existed since the
# stall detector landed and this header still claimed the script only ever
# returned 0 or 1, so any caller branching on the documented set treated a wedge
# as an ordinary failure.  A status a caller cannot know about is a status the
# caller cannot handle.
#
# IF YOU WRAP THIS IN scripts/run-timeout.py, GIVE IT AT LEAST 7200 SECONDS:
#
#   python scripts/run-timeout.py --poll 60 7200 ./scripts/boot-test.sh
#
# This script runs QEMU under its *own* timeout, 2400s by default (--timeout).
# An outer budget also has to cover the pre-build gates and the kernel build,
# so an outer budget below inner+gates+build is strictly the smaller window and
# the inner timeout can never fire.  That is not a harmless duplication: the
# inner timeout is the diagnostic one -- it reports SYSTEM HANG, dumps the
# guest's state, and reads the faulting RIP back over the HMP monitor.
# run-timeout's expiry gives exit 124 and a killed process tree: no RIP, no
# task table, no marker saying where it stopped.  So an outer budget at or
# below the inner one silently turns every genuine boot hang into an anonymous
# kill, on exactly the runs where the instrumentation matters most.
#
# Measured 2026-08-25: gates + a cold-cache clippy recompile + build took 530s,
# leaving 370s of a 900s outer budget for a boot that reaches BOOT_OK at
# 370-405s.  A healthy guest was killed mid-diagnostics.
#
# Re-measured 2026-08-31, and the gate half has grown far more than the boot
# half: with another lane building concurrently, gates + clippy + build took
# ~3000s before QEMU was even started, and BOOT_OK then landed at 1043s.  So
# the pre-QEMU phase is now the *larger* of the two and swings widely with host
# load -- an outer budget derived from the boot time alone will be wrong.
#
# Budget the outer as gates+build+inner, then round up: 7200 = ~3000 observed
# pre-QEMU + 2400 inner + headroom.  Being generous costs nothing here:
# run-timeout's real job is tearing down the whole process tree, grandchildren
# included, and that is independent of the budget.  An outer budget that is too
# tight does not merely delay the answer, it destroys the diagnostic -- which is
# what happened twice on 2026-08-31 before this comment was rewritten.
# See known-issues.md -> Lesson 50.
#
# Usage:
#   ./scripts/boot-test.sh              # full build + test (waits for BOOT_OK)
#   ./scripts/boot-test.sh --no-build   # skip build (still re-stages target/!)
#   ./scripts/boot-test.sh --no-stage   # boot the existing ESP image verbatim;
#                                       # use this for soaks so a concurrent
#                                       # `cargo build` cannot swap the kernel
#                                       # mid-run
#   ./scripts/boot-test.sh --profile=debug|release
#                                       # force the build profile, independently
#                                       # of --bench.  Without it, --bench means
#                                       # release and everything else means
#                                       # debug.  `--bench --profile=debug` is
#                                       # the only way to run the benchmark
#                                       # suite on a debug build, which is what
#                                       # exercises the debug branch of the
#                                       # per-profile budgets in bench.rs.
#                                       # An unrecognised value is an error, not
#                                       # a fallback: a typo must not silently
#                                       # measure the other profile.
#   ./scripts/boot-test.sh --bench      # wait for BENCH_OK and print benchmark
#                                       # numbers (the micro-benchmarks run in a
#                                       # deferred background task AFTER BOOT_OK,
#                                       # so the default fast path never sees
#                                       # them — use this to catch perf
#                                       # regressions).  Raises the default
#                                       # timeout to 1200s, since the suite
#                                       # runs well past BOOT_OK; an explicit
#                                       # --timeout= still wins.
#
#                                       # DO NOT RUN ANYTHING ELSE ON THIS
#                                       # MACHINE WHILE --bench IS IN ITS QEMU
#                                       # WINDOW.  TCG is pure emulation and
#                                       # entirely CPU-bound, so competing work
#                                       # scales the measurements.  Demonstrated,
#                                       # not assumed: the 2026-08-14T22:22 run
#                                       # measured a 47% spread in the reference
#                                       # access cost while a grep, an Edit and
#                                       # two Python scripts ran alongside it;
#                                       # the next run, on an idle machine, read
#                                       # 0% (5.16 vs 5.17 cycles) at every one
#                                       # of the same 8 sample positions.  The
#                                       # build phase is safe; the QEMU window is
#                                       # not.  Several "regressions" written up
#                                       # on 2026-08-14 were self-inflicted this
#                                       # way.  The canary reports it as
#                                       # CONTAMINATED -- believe it.  See
#                                       # known-issues.md P19.
#
#                                       # READ THAT ONE WAY ONLY.  When the
#                                       # canary FIRES it is right; when it does
#                                       # not fire it has certified nothing.  It
#                                       # counts *guest* cycles, which do not
#                                       # advance while the host is running
#                                       # something else, so it is structurally
#                                       # blind to the host stealing the CPU --
#                                       # the dominant contamination mode here.
#                                       # It read 0% spread, its cleanest
#                                       # possible verdict, on a run that took
#                                       # 2.3x as long as its own twin.  Read
#                                       # the "RUN CONTAMINATED/UNPROVEN/CLEAN"
#                                       # line from bench-history.py instead;
#                                       # see known-issues.md
#                                       # B-CANARY-IS-BLIND-TO-HOST-DESCHEDULING.
#   ./scripts/boot-test.sh --bench --host-load=idle
#                                       # assert that nothing else was running
#                                       # on this machine during the QEMU window
#                                       # (idle|loaded|unknown; default
#                                       # unknown).  It is an assertion by you,
#                                       # not a measurement, and it is recorded
#                                       # as such.  --host-load=loaded marks a
#                                       # deliberately-poisoned control run, and
#                                       # such runs are then excluded from every
#                                       # baseline and band the comparator uses.
#   ./scripts/boot-test.sh --hard-lockup-watchdog
#                                       # attach a QEMU i6300esb PCI watchdog set
#                                       # to inject an NMI on timeout. OFF by
#                                       # default (zero effect on normal runs).
#                                       # For deliberate repro runs of the
#                                       # B-PTHREAD-YIELDBUDGET BSP-dead hang: if
#                                       # the kernel's (future) watchdog driver
#                                       # stops kicking because the BSP wedged
#                                       # with IF=0, QEMU injects an NMI that
#                                       # fires regardless of IF, letting the NMI
#                                       # handler dump the task table. See
#                                       # open-questions.md Q20. Under our default
#                                       # TCG/no-PMU QEMU this is the only NMI
#                                       # source that can catch a single-CPU
#                                       # IF=0 spin.
#   ./scripts/boot-test.sh --no-monitor
#                                       # do NOT attach QEMU's HMP monitor on a
#                                       # TCP socket.  The monitor is ON by
#                                       # default: it is what lets a timeout or
#                                       # a serial stall read the wedged guest's
#                                       # RIP straight out of the emulator, and
#                                       # it works even when the guest takes no
#                                       # interrupts at all (IF=0), because the
#                                       # read is host-side.  Unlike
#                                       # --hard-lockup-watchdog above it adds no
#                                       # guest device and changes no PCI
#                                       # topology, so it is invisible to the
#                                       # guest — which is why it defaults on and
#                                       # the watchdog does not.  Use this flag
#                                       # only if the host TCP listen itself is a
#                                       # problem (e.g. a sandbox that forbids
#                                       # binding a port).
#   ./scripts/boot-test.sh --bootstrap  # if a git-ignored prerequisite is
#                                       # missing (one of the six ring-3 service
#                                       # binaries the kernel embeds, or the
#                                       # limine/ tree staged into the ESP), run
#                                       # scripts/bootstrap-worktree.sh and
#                                       # continue instead of refusing.  Without
#                                       # it the run stops before Step 1 and
#                                       # prints the command — provisioning
#                                       # builds six crates and may clone a
#                                       # bootloader, which a run should not do
#                                       # unasked.  This is what a fresh
#                                       # worktree needs; see known-issues.md
#                                       # A-A-FRESH-CHECKOUT-CANNOT-BOOT-TEST-…
#   ./scripts/boot-test.sh --usb-image  # build build/slateos-usb.img (real
#                                       # protective MBR + GPT + FAT32) from the
#                                       # staged ESP and boot *that*, attached as
#                                       # a usb-storage device, instead of QEMU's
#                                       # virtual FAT.  See bare-metal-boot.md.
#   ./scripts/boot-test.sh --no-rootfs  # do not attach rootfs.ext4, even when it
#                                       # exists.  This is the shape a real USB
#                                       # stick has, so it is what --usb-image
#                                       # runs should be paired with before
#                                       # trusting the image on hardware.  Tagged
#                                       # as an experiment in boot-history, since
#                                       # the Path-Z rungs that read /mnt no-op
#                                       # and their outcome is therefore not
#                                       # evidence about the tree.

set -euo pipefail

# --- Run from a snapshot of ourselves, not from the file in the tree ---------
#
# bash does not read a script into memory. It reads it in chunks, and it seeks
# back to the byte offset it had reached each time it wants more -- so editing
# a running script edits the part it has not executed yet. For a script that
# runs for well over an hour, that is not a theoretical window: this file is
# edited *during* its own runs, because the whole point of backgrounding a boot
# test is to keep working while it goes, and the tree it tests is the tree the
# agent is developing in.
#
# The failure is silent and it does not look like an edit. Bash resumes at the
# old offset in the new file, which now lands in the middle of a different
# line, and executes whatever text follows it: half a word as a command, an
# unbalanced quote that swallows the rest of the file, a `fi` with no `if`.
# What the operator sees is a syntax error on a line that is syntactically
# fine, or -- much worse -- a run that quietly skips a gate and still says
# PASSED.
#
# So the first thing this script does is copy itself somewhere private and hand
# over to that copy. The copy is complete before the first gate runs and no
# editor will ever touch it, which makes the run atomic with respect to the
# tree. `BOOT_TEST_REEXEC` is the guard that stops the copy copying itself, and
# `BOOT_TEST_ORIG_DIR` carries the one thing the copy cannot work out for
# itself: `SCRIPT_DIR` is derived from `$0`, and `$0` in the copy points at the
# temp directory, so every sibling script (`run-checker.sh`, the checkers, the
# QEMU helpers) would be looked up in the wrong place.
#
# The copy is deleted by the trap below rather than by the copy itself, since
# a script cannot reliably remove the file it is still being read from.
if [ -z "${BOOT_TEST_REEXEC:-}" ]; then
    _bt_self="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
    _bt_snapshot="$(mktemp -t boot-test-snapshot.XXXXXX)" || {
        echo "boot-test.sh: could not create a snapshot of myself; refusing to"
        echo "  run directly from the tree, because an edit during the run"
        echo "  would be executed as if it had always been there."
        exit 125
    }
    # `cat` rather than `cp` so the snapshot is a fresh inode with our own
    # permissions: `cp` would preserve the source's, and on a checkout where
    # the script is not executable that would produce a copy we cannot exec.
    cat "$_bt_self" > "$_bt_snapshot"
    chmod +x "$_bt_snapshot"
    # The trap is installed in *this* shell, which stays alive as the parent
    # only if we do not `exec`. That is the trade: one extra process for the
    # lifetime of the run, in exchange for the snapshot being removed on every
    # exit path including a signal.
    trap 'rm -f "$_bt_snapshot"' EXIT INT TERM
    BOOT_TEST_REEXEC=1 \
    BOOT_TEST_ORIG_DIR="$(cd "$(dirname "$0")" && pwd)" \
        bash "$_bt_snapshot" "$@"
    exit $?
fi

# Scan the serial log for self-test failures that do NOT halt the boot.
#
# Many fs/subsystem self-tests are NON-FATAL: on failure main.rs logs a
# "WARNING: <X> self-test failed" (or "[WARN] ..."/"[hpet] WARNING:
# Self-test failed") and boots on, so BOOT_OK still prints and a naive
# "grep BOOT_OK" reports PASSED even though a test regressed (this exact
# gap hid a stale procfs readdir-count assertion — see todo.txt).
#
# We match the wrapper marker "self-test failed" (case-insensitive),
# which every main.rs self-test failure path emits.  We deliberately do
# NOT grep raw "FAIL:"/"WARNING:": those have legitimate occurrences in a
# passing log — e.g. "[drm-atomic] check FAIL: CRTC 9999 not found"
# (intentional negative tests) and "[lockdep] WARNING: potential deadlock"
# (a deliberately-triggered detector test) — so they would false-positive.
#
# Returns 0 if clean, 1 if any self-test failure marker is present.
check_selftest_failures() {
    local file="$1"
    [ -f "$file" ] || return 0
    if grep -iq "self-test failed" "$file"; then
        echo "SELF-TEST FAILURE detected in serial log:"
        grep -in "self-test failed" "$file" || true
        return 1
    fi
    return 0
}

# Fail a boot whose liveness watchdog reported a hang, or admitted a false one.
#
# WHY THIS EXISTS: on 2026-08-15 a green boot (exit 0, BOOT_OK reached) contained
# both a "[liveness] SYSTEM HANG" report at ~140s AND the watchdog's own
# "that report was a FALSE POSITIVE" admission 600s later.  The harness said
# nothing, because it only ever grepped for BOOT_OK.  BUG-LIVENESS-DEADLINE-
# FALSE-FIRE's Verification section had *required* a boot log free of these
# lines since 2026-07-27, but the requirement lived only in prose -- so a run
# that violated both halves of it still exited 0.  A contract nothing checks is
# a contract that is not enforced.
#
# The patterns are anchored to line start and matched against the kernel's exact
# strings.  The "(self-test) " infix is what keeps this from firing on the
# deliberate drills in test_liveness_watchdog, which prove the detectors still
# work by driving them into firing on purpose:
#   real:  "[liveness] SYSTEM HANG: ..."
#   drill: "[liveness] (self-test) SYSTEM HANG: ..."
# Matching on the real shape alone means the drills stay silent here without the
# harness needing to know how many of them there are.
#
# Returns 0 if clean, 1 if any liveness failure marker is present.
check_liveness_failures() {
    local file="$1"
    [ -f "$file" ] || return 0
    # shellcheck disable=SC2016
    local pat='^\[liveness\] (SYSTEM HANG|SUSPECTED LIVELOCK)|^\[liveness\].*BOOT DEADLINE EXCEEDED|^\[liveness\].*FALSE POSITIVE'
    if grep -aEq "$pat" "$file" 2>/dev/null; then
        echo "LIVENESS WATCHDOG failure detected in serial log:"
        grep -aEn "$pat" "$file" || true
        echo "  (a '(self-test)' infix marks a deliberate drill and is NOT matched above;"
        echo "   these are real reports, or the watchdog's own false-positive admission)"
        return 1
    fi
    return 0
}

# Fail a boot whose benchmark suite measured something it then never judged.
#
# WHY THIS EXISTS: bench.rs prints one line per suite stating how many of its
# measurement windows reached the scorecard.  Every window must land in exactly
# one of three places — scored (graded against a target), tracked (recorded,
# ungraded), or *declared* a diagnostic (deliberately print-only).  A window in
# none of those is measured every boot and recorded never: it burns boot time,
# prints a number nobody compares, and cannot regress visibly.  Seven such
# benchmarks accumulated before the instrument existed (see known-issues.md,
# "benchmarks measured every boot and recorded never"), and the reason they went
# unnoticed for so long is precisely that nothing failed when they appeared.
#
# The design point that matters here is the ABSENT case.  Under --bench the
# coverage line MUST be present, and its absence is a failure, not a pass.
# run_all() prints it before main.rs prints BENCH_OK on both the deferred and
# the inline-fallback path, so "BENCH_OK but no coverage line" means the
# instrument itself stopped running — which is the exact condition it exists to
# catch, and treating it as "nothing to complain about" would reproduce the
# original bug one level up.  Outside --bench the suite may legitimately not run
# at all, so an absent line is simply not evidence either way.
#
# Two failure shapes are matched:
#   "... , N unjudged (print-only: ..."  with N > 0  -> a real coverage gap
#   "[bench]   NOTE: ... carried a seq that names no measurement window"
#       -> a BenchResult was hand-built without note_measurement, so the counts
#          above it are unreliable and a clean "0 unjudged" cannot be believed
#
# ANY occurrence fails, not just the last: run_all() resets its state and may be
# run more than once, and each printing is a complete verdict for its own run.
#
# Returns 0 if clean, 1 on a coverage gap / unreliable count / missing line.
check_bench_coverage() {
    local file="$1"
    local require="${2:-0}"
    [ -f "$file" ] || return 0

    # shellcheck disable=SC2016
    local line_pat='^\[bench\] === Scorecard coverage: '
    if ! grep -aEq "$line_pat" "$file" 2>/dev/null; then
        if [ "$require" = "1" ]; then
            echo "BENCH COVERAGE LINE MISSING from serial log:"
            echo "  --bench reached its marker but bench.rs never printed"
            echo "  '[bench] === Scorecard coverage: ...'.  That line is printed"
            echo "  unconditionally by print_scorecard(), before BENCH_OK, so its"
            echo "  absence means the coverage instrument did not run.  A check that"
            echo "  cannot fire is indistinguishable from a check that passes."
            return 1
        fi
        return 0
    fi

    local rc=0

    # Non-zero unjudged.  Anchored on the literal " unjudged (print-only:" suffix
    # so it cannot match the "N are declared diagnostics" field next to it.
    # shellcheck disable=SC2016
    local gap_pat='^\[bench\] === Scorecard coverage: .*, [1-9][0-9]* unjudged \(print-only:'
    if grep -aEq "$gap_pat" "$file" 2>/dev/null; then
        echo "BENCH COVERAGE GAP detected in serial log:"
        grep -aEn "$gap_pat" "$file" || true
        echo "  The named windows below were measured but never recorded:"
        grep -aEn '^\[bench\]   unjudged print-only: ' "$file" || true
        echo "  Each must be given a destination: score() if it has a target,"
        echo "  track() if it is comparable but untargeted, or run_diagnostic()"
        echo "  if it is deliberately print-only.  Untargeted is not uncomparable."
        rc=1
    fi

    # Unreliable counts.  Reported even when unjudged reads 0 — especially then,
    # since an orphan seq marks some *other* window covered, which is the
    # direction that hides work rather than inventing it.
    # shellcheck disable=SC2016
    local orphan_pat='^\[bench\]   NOTE: .*carried a seq that names no'
    if grep -aEq "$orphan_pat" "$file" 2>/dev/null; then
        echo "BENCH COVERAGE COUNTS ARE UNRELIABLE (orphan seq) in serial log:"
        grep -aEn "$orphan_pat" "$file" || true
        echo "  A BenchResult was constructed without calling note_measurement(),"
        echo "  so its seq indexes a window belonging to some other measurement."
        rc=1
    fi

    return "$rc"
}

# Detect a kernel that has already died, so the wait loop can stop waiting.
#
# WHY THIS EXISTS: the poll loop used to test only for the success marker, and
# the "PANIC\|FATAL" check sat AFTER the loop — i.e. it only ran once the full
# TIMEOUT had burned.  A capability self-test failure that hit serial at ~100s
# therefore cost 1004s of wall clock before the harness said a word (observed,
# 2026-08-15, while break-testing test_valid_entries).  Nothing about the
# verdict changes here: a dead kernel never reaches the marker, so an early
# check can only ever reach the SAME verdict SOONER.
#
# The patterns are anchored to line start, and to the exact strings the kernel
# itself emits:
#   kernel/src/main.rs panic handler -> "!!! KERNEL PANIC !!!"
#                                    -> "!!! DOUBLE PANIC (panic inside ...)"
#   every fatal path in main.rs/idt.rs -> serial_println!("FATAL: ...")
# Anchoring matters because this check runs against a LIVE, still-booting log:
# an unanchored "PANIC\|FATAL" would match a userspace fixture echoing the word,
# or a diagnostic that merely mentions it, and would abort a healthy boot.  The
# post-loop check keeps its wider unanchored net — by then the boot has already
# failed to reach the marker, so a loose match there costs nothing.
#
# Returns 0 if the log shows a dead kernel, 1 otherwise.
kernel_is_dead() {
    local file="$1"
    [ -f "$file" ] || return 1
    grep -aEq '^(FATAL:|!!! KERNEL PANIC !!!|!!! DOUBLE PANIC)' "$file" 2>/dev/null
}

# Print the death evidence from a serial log.  Shared by the in-loop early exit
# and the post-loop check so both report the same way.
#
# WHY THE CONTEXT WINDOW: the FATAL line names the *subsystem*, not the test.
# Break-testing CapEntryInfo printed only
#     FATAL: Capability system self-test failed: internal kernel error (-1)
# while the line that actually said what broke -- "[cap]   FAIL: CapEntryInfo is
# 32 bytes / align 8, ABI says 24 / 8" -- sat one line above and was not shown.
# Every self-test in the kernel has that shape: a specific FAIL: diagnostic
# immediately followed by a generic FATAL: wrapper.  Reporting the wrapper alone
# means the harness output can only ever tell you to go read the log yourself,
# which is the difference between a diagnosis and a notification.
report_kernel_death() {
    local file="$1"
    echo "KERNEL PANIC detected!"
    local first
    first="$(grep -anE '^(FATAL:|!!! KERNEL PANIC !!!|!!! DOUBLE PANIC)' "$file" 2>/dev/null | head -1 | cut -d: -f1)"
    if [ -n "$first" ]; then
        local from=$((first - 12))
        [ "$from" -lt 1 ] && from=1
        echo "--- serial log lines ${from}-$((first + 20)) (context around the death) ---"
        sed -n "${from},$((first + 20))p" "$file" | sed 's/^/  /'
        echo "--- end context ---"
    fi
    # Still list every death-ish line in the whole log: a second panic further
    # down (or an EXCEPTION before the first FATAL) is worth seeing.
    echo "All PANIC/FATAL/EXCEPTION lines:"
    grep -an "PANIC\|FATAL\|EXCEPTION" "$file" | head -40 || true
}

# Surface Path-Z rungs that DID NOT RUN because rootfs.ext4 lacked a binary
# they drive.
#
# This is NOT a failure — the image is git-ignored and a diskless boot must
# still pass — so it does not change the exit code.  It is printed loudly
# because the alternative was worse than a failure: a skipped rung used to be
# byte-identical to a passing one in the serial log, and all 26 tcc rungs
# no-op'd unnoticed for weeks once /bin/tcc fell out of the image
# (known-issues.md -> B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT).  A skip that is
# reported gets acted on; a silent one gets believed.
report_pathz_skips() {
    local file="$1"
    [ -f "$file" ] || return 0
    local line
    line="$(grep -a 'Path-Z prerequisites:' "$file" | tail -1)"
    case "$line" in
        *"rung(s) SKIPPED"*)
            echo "=== PATH-Z COVERAGE INCOMPLETE ==="
            echo "  ${line#*\[spawn\] }"
            grep -a '\[spawn\]   SKIP:' "$file" | head -8 | sed 's/^/  /'
            local n
            n="$(grep -ac '\[spawn\]   SKIP:' "$file")"
            [ "$n" -gt 8 ] && echo "  ... and $((n - 8)) more"
            # Two different causes, two different remedies.  Under --no-rootfs
            # the skips are the point of the run, and telling the reader to
            # rebuild an image they deliberately unplugged would send them to
            # fix something that is not broken.
            if [ "${NO_ROOTFS:-0}" = 1 ]; then
                echo "  (expected: --no-rootfs was given, so /mnt is empty by design)"
            else
                echo "  (rebuild the image: wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh)"
            fi
            ;;
    esac
    return 0
}

# Refuse to boot a rootfs.ext4 whose staged binaries no longer match the tree.
#
# WHY A NEW GUARD, when this script already has three staleness checks: every
# one of them asks "was this ELF built from the current source?" -- a question
# about `build/` and `target/`.  None asks "is the ELF we just checked the one
# that is actually INSIDE the image we are about to attach."  rootfs.ext4 is
# produced by a separate, manual step (`wsl -d Ubuntu -- bash
# scripts/create-ext4-rootfs.sh`) that the boot test does not run, so the image
# can lag the tree by days while every existing guard stays green.  The result
# is not a missing warning, it is a FALSE GREEN: the Path-Z rungs run, they
# pass, and what they exercised was last week's binary.  Filed by lane B as
# requests/b-a-boot-test-boots-a-rootfs-image-that-may-predate-the-fixtures-in-it.md.
#
# WHY CONTENT HASHES AND NOT MTIME: mtime cannot answer the question this guard
# asks.  "Is the ELF inside the image the one we just built" is about the bytes
# in the image, and a timestamp describes neither the bytes nor which of them
# came from where.  That reason is independent of everything else and is why
# hashes are correct here permanently.
#
# There used to be a second reason -- QEMU opened the image read-write, so a
# boot updated its mtime and the image was newer than the tree after every run,
# making the obvious `-ot` idiom (used by the staged-kernel guard below) not
# merely weak but inverted.  That is no longer true: the drive is attached with
# `snapshot=on` (see the attach site), so a boot no longer touches the file at
# all and its timestamp once again records when it was *packed*.  The stale
# half of this rationale is recorded rather than deleted because a future reader
# who finds `-ot` working here might otherwise conclude the guard is redundant;
# it is not, for the first reason.  `ctest-fixtures.py image-check`
# compares a sha256 per staged ELF against rootfs.ext4.manifest instead, and
# fails closed when the manifest is absent -- an image that predates the check
# cannot have its contents established, which is not the same as matching.
#
# WHY FATAL: this is the same class of failure as the staged-kernel staleness
# check ("the thing we are about to boot is not the thing we just built"), and
# that one exits 1 before QEMU starts.  A run that boots the wrong bits and
# prints PASSED is worse than one that refuses to start, because only the
# second kind gets noticed.
#
# The one case that is NOT fatal is a host with no python: that is a property
# of this machine, not of the tree under test, and aborting for it would make a
# perfectly good kernel untestable here.  It warns, sets ROOTFS_UNVERIFIED, and
# finish_pass re-prints that at the bottom of the run so a qualified green
# cannot scroll away above several hundred lines of serial log.
ROOTFS_UNVERIFIED=""
check_rootfs_freshness() {
    if [ "${BOOT_TEST_SKIP_ROOTFS_CHECK:-0}" != "0" ]; then
        ROOTFS_UNVERIFIED="BOOT_TEST_SKIP_ROOTFS_CHECK was set"
        echo "=== WARNING: rootfs.ext4 freshness check SKIPPED by request ==="
        echo "    Path-Z results from this run may reflect stale binaries."
        return 0
    fi

    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        ROOTFS_UNVERIFIED="no python interpreter on this host"
        echo "=== WARNING: cannot verify rootfs.ext4 -- no python found ==="
        echo "    The Path-Z rungs will run against whatever is in the image,"
        echo "    which may predate the tree.  Treat their results as"
        echo "    unconfirmed rather than as coverage."
        return 0
    fi

    echo "=== Verifying rootfs.ext4 matches the built fixtures ==="
    if ! "$py" "$SCRIPT_DIR/ctest-fixtures.py" image-check; then
        # Follow the content check with the sysroot check, which answers the
        # question `image-check` structurally cannot: it can say an ELF and its
        # recorded inputs differ, but not which side is behind.  `sysroot-check`
        # says whether `libc.a` — the input every fixture links — is behind the
        # `posix/` sources it was built from, which is the answer that tells the
        # reader whether to rebuild the image or rebuild the sysroot first.
        #
        # This slot used to hold `stamp-ancestry.py`, a git-history walk that
        # named the commits since each stamp that had touched the declared
        # sources.  It is retired: the fixtures are now gitignored
        # build-on-demand artifacts (design-decisions.md §355), so it had no
        # tracked stamps left to walk and failed unconditionally on every run.
        # A content stamp answers the same question better anyway — it sees
        # uncommitted edits, which a history walk never could.  See §277.
        #
        # Deliberately cannot change the outcome.  This path already fails and
        # the exit below is unconditional, so the diagnostic is additive; a
        # diagnostic able to fail the run would be a new way to break a boot
        # test.  Hence the `|| true`.
        echo "" >&2
        echo "--- is the sysroot these fixtures link still current? ---" >&2
        "$py" "$SCRIPT_DIR/ctest-fixtures.py" sysroot-check >&2 || true
        echo "" >&2
        echo "ERROR: rootfs.ext4 does not match the binaries in this tree." >&2
        echo "       Booting it would run the Path-Z self-tests against stale" >&2
        echo "       binaries and then report PASSED.  Rebuild the image:" >&2
        echo "         wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh" >&2
        echo "       To boot it anyway (and get a loud UNVERIFIED banner):" >&2
        echo "         BOOT_TEST_SKIP_ROOTFS_CHECK=1 ./scripts/boot-test.sh" >&2
        exit 1
    fi

    # Run the staleness check on the *passing* path too, which is the case that
    # actually bites and the one a failure-path-only wiring would have missed.
    #
    # `image-check` passing means the image matches the ELFs on disk.  It does
    # not mean those ELFs are current, and the two can diverge without anyone
    # touching them: build the fixtures and pack the image while `libc.a` is
    # fresh, then merge `origin/main` with new `posix/` commits in it.  Nothing
    # on disk changed, so `image-check` still passes — but every fixture now
    # links a libc that is not in the tree.  That is a green boot test whose
    # Path-Z rungs covered a system this tree cannot build, which is the silent
    # version of the failure above and worse than the loud one.
    #
    # `sysroot-check` catches exactly that by hashing the 2312 sources `libc.a`
    # is built from against the stamp recorded when it was built.  It replaces
    # `stamp-ancestry.py`, which walked git history to reach a weaker form of
    # the same conclusion and, since §355 made the fixtures untracked, could
    # only fail.  A content stamp also sees uncommitted `posix/` edits, which a
    # history walk cannot see at all.
    #
    # A warning, not a failure, for two reasons.  Repairing it means rebuilding
    # the sysroot and relinking fixtures under services/**, which is lane B's
    # tree, so failing here would block every lane-A boot test on a repair
    # lane A must not make.  And the boot test is still worth running against
    # slightly-old fixtures — it is only the Path-Z rungs whose coverage is in
    # question, not the kernel under test.  What was missing was never the
    # blocking, it was the reader being told.
    if ! "$py" "$SCRIPT_DIR/ctest-fixtures.py" sysroot-check >/dev/null 2>&1; then
        echo "=== WARNING: fixtures are behind the tree (boot test still valid) ==="
        "$py" "$SCRIPT_DIR/ctest-fixtures.py" sysroot-check 2>&1 || true
        echo "    The content check above passed, so the image matches the ELFs"
        echo "    in this tree -- but those ELFs link a libc.a older than the"
        echo "    posix/ sources named above.  The kernel result below is"
        echo "    unaffected; treat the Path-Z rung results as covering the"
        echo "    older libc rather than the current one."
        echo "    To repair:  powershell -File toolchain/build-sysroot.ps1"
        echo "                python scripts/ctest-fixtures.py build"
        echo "                wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh"
    fi

    check_sysroot_identity
}

# Is the `libc.a` these fixtures were LINKED against the same file as the one
# the checks above VERIFIED?
#
# The gate above answers "is our sysroot behind our posix/ sources?".  That is
# a question about `$PROJECT_ROOT/toolchain/sysroot/lib/libc.a`, because
# ctest-fixtures.py derives LIBC from its own `REPO` (ctest-fixtures.py:196-198,
# `Path(__file__).resolve().parent.parent` -- this worktree).  Correct, and not
# the whole question.
#
# fastpy resolves the sysroot independently, and differently.
# `_find_slateos_sysroot_lib` (fastpy/compiler/toolchain.py:160-186) tries
# $FASTPY_SLATEOS_SYSROOT first and otherwise falls back to a *sibling* `os`
# checkout -- `<fastpy>/../os/toolchain/sysroot/lib`, which is the integration
# worktree, never the lane worktree the build ran from.  Nothing in
# ctest-fixtures.py sets that variable, so from os-lane-{a,b,c} the fallback is
# what fires and the fixtures link a libc.a from a different checkout than the
# one every check in this script reasons about.  Observed 2026-08-31: all three
# lanes sharing one 11-day-old copy.
#
# Note where this is invisible: built from `D:\visual studio projects\os`, the
# two paths ARE the same directory, so the mismatch cannot occur in the tree we
# integrate in and occurs in all three we work in.  That is why no existing gate
# caught it -- each one is individually right about the file it looks at.
#
# WHY CONTENT AND NOT PATH.  Two different paths holding identical bytes is not
# a problem; it is the ordinary case right after everyone rebuilds.  Comparing
# realpaths would fire on every lane run and be tuned out within a day, which is
# the failure mode a gate cannot recover from.  Compare the bytes.
#
# WHY A WARNING AND NOT A FAILURE.  Same reasoning as the gate above, and it is
# the reasoning rather than the conclusion that is being reused: the repair is
# to set FASTPY_SLATEOS_SYSROOT in scripts/ctest-fixtures.py and relink under
# services/**, both lane B's tree (request filed:
# requests/a-b-ctest-fixtures-verify-one-libc-and-link-a-different-one.md).
# Failing here would block every lane-A boot test on a repair lane A must not
# make.  And the kernel result is unaffected either way -- it is only what the
# Path-Z rungs prove about *this* posix that is in question, not the kernel
# under test.  This gate's job is to stop the reader believing something the
# run did not establish.
check_sysroot_identity() {
    local ours="$PROJECT_ROOT/toolchain/sysroot/lib/libc.a"
    if [ ! -f "$ours" ]; then
        # Nothing to compare against.  The gate above already covers a missing
        # sysroot; staying quiet here avoids two warnings for one cause.
        return 0
    fi

    # Mirror _find_slateos_sysroot_lib's candidate order exactly.  If this ever
    # disagrees with fastpy the gate becomes worse than nothing -- it would
    # report on a file fastpy does not use -- so it is written to be read
    # side-by-side with that function.
    local resolved=""
    local c
    if [ -n "${FASTPY_SLATEOS_SYSROOT:-}" ]; then
        for c in "$FASTPY_SLATEOS_SYSROOT/lib" "$FASTPY_SLATEOS_SYSROOT"; do
            if [ -f "$c/libc.a" ]; then resolved="$c"; break; fi
        done
    fi
    if [ -z "$resolved" ]; then
        # ctest-fixtures.py locates fastpy as $FASTPY_DIR or a sibling checkout
        # (ctest-fixtures.py:868-873); fastpy then walks up from there.
        c="${FASTPY_DIR:-$PROJECT_ROOT/../fastpy}/../os/toolchain/sysroot/lib"
        if [ -f "$c/libc.a" ]; then resolved="$c"; fi
    fi

    if [ -z "$resolved" ]; then
        # Say which of the two possible causes this is.  The message used to
        # read "something this host can no longer name", which describes a
        # missing or unidentifiable libc -- and that is not what happened.  Ours
        # is present and named on the next line (the early return above proves
        # it exists).  What failed is fastpy's *search*: its last candidate is a
        # sibling checkout literally named `os`, so from `os-lane-{a,b,c}` it
        # looks only at the integration worktree and never at the tree being
        # tested.  Misnaming a search defect as a missing file sends the reader
        # to rebuild a sysroot that is already there.
        echo "=== WARNING: fastpy cannot see this worktree's sysroot ==="
        echo "    ours (present, never searched): $ours"
        echo "    fastpy's last candidate is a *sibling checkout named 'os'*,"
        echo "    which in the three-worktree layout is the integration tree and"
        echo "    holds no sysroot.  \$FASTPY_SLATEOS_SYSROOT is unset, so no"
        echo "    candidate resolves and the fixtures in the image were linked"
        echo "    without one -- their Path-Z rungs cannot be attributed to any"
        echo "    posix/ revision.  The kernel result below is unaffected."
        echo "    To repair, set the variable when the fixtures are BUILT:"
        # Quoted: every checkout of this project lives under a path with a
        # space in it ("visual studio projects"), so an unquoted assignment
        # here is a repair line that fails when pasted.
        echo "        FASTPY_SLATEOS_SYSROOT=\"$PROJECT_ROOT/toolchain/sysroot\" \\"
        echo "            python scripts/ctest-fixtures.py build"
        echo "        wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh"
        echo "    Setting it only for this check would make the comparison below"
        echo "    read our own libc.a against itself and pass in silence, which"
        echo "    is why this script does not do that (known-issues.md"
        echo "    A-FASTPY-SYSROOT-SEARCH-CANNOT-SEE-A-LANE-WORKTREE)."
        return 0
    fi

    if cmp -s "$resolved/libc.a" "$ours"; then
        return 0
    fi

    # Normalise for the message only.  The candidate walk above deliberately
    # does NOT resolve as it goes -- it mirrors fastpy, which does not either --
    # but the raw fallback reads as `<worktree>/../fastpy/../os/...`, and a
    # reader who has to unpick that is a reader who skips the warning.
    local shown
    shown="$(realpath -m "$resolved/libc.a" 2>/dev/null || echo "$resolved/libc.a")"

    echo "=== WARNING: fixtures link a libc.a from a different checkout ==="
    echo "    verified: $ours"
    echo "    linked:   $shown"
    echo "    These differ in content.  Every check above reasoned about the"
    echo "    first path; fastpy links the second (see"
    echo "    fastpy/compiler/toolchain.py _find_slateos_sysroot_lib).  The"
    echo "    kernel result below is unaffected -- but the Path-Z rungs exercise"
    echo "    the posix/ that built the *linked* libc, which is not this tree's."
    echo "    To repair:  set FASTPY_SLATEOS_SYSROOT=$PROJECT_ROOT/toolchain/sysroot"
    echo "                python scripts/ctest-fixtures.py build"
    echo "                wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh"
}

# Directories whose contents are performance-critical per CLAUDE.md's
# "Performance-Critical Subsystems" table.  A change under any of these is a
# change that CLAUDE.md requires benchmarking, so it is the trigger for
# nagging about a stale benchmark record.
# This list answers only "is this file worth asking about?".  It does NOT say
# a benchmark exists for it -- BENCH_COVERAGE below is the single source of
# truth for that, and these entries are deliberately left unannotated so there
# is no second copy of that mapping to drift out of agreement with the first.
#
# It is only useful if it covers everything the suite measures, and the failure
# mode when it does not is SILENT -- an unwatched path reports "no perf-critical
# changes", which is exactly the false negative this whole mechanism exists to
# prevent.  The first version of this list was derived from CLAUDE.md's
# perf-critical table read as *directories*, and it missed more than half the
# suite: 30+ of the 63 benchmarks measured code in idt.rs, fs/, net/ and
# crypto.rs, none of which were listed.  Cross-check against `python
# scripts/bench-history.py --list` / the recorded entry names when adding a
# benchmark.
BENCH_CRITICAL_PATHS=(
    "kernel/src/mm"
    "kernel/src/sched"
    "kernel/src/ipc"
    "kernel/src/syscall"
    "kernel/src/smp.rs"       # cross-CPU paths behind the above
    "kernel/src/idt.rs"       # CLAUDE.md's "interrupt dispatch" and "page
                              #   fault handling" enter through here
    "kernel/src/fs"
    "kernel/src/net"
    "kernel/src/crypto.rs"
    "kernel/src/apic.rs"      # handle_timer_irq -- the only code isr_latency
                              #   actually times, and it was not being watched
    "kernel/src/sync.rs"      # the kernel Mutex: on essentially every hot path,
                              #   and measured by lock_uncontended
    "kernel/src/lockdep.rs"   # per-acquire cost rides on every tracked lock;
                              #   bench.rs records that an O(edges) scan in
                              #   record_edge "went unread" for two runs
)

# --- Which benchmark, if any, actually measures a given file ----------------
#
# BENCH_CRITICAL_PATHS above is a list of DIRECTORIES, and a directory is a
# claim about every file beneath it.  That claim is mostly false.  Counted
# 2026-08-20: kernel/src/fs is 467 tracked .rs files and the suite reaches
# three of them; kernel/src/mm is 47 and the suite reaches five.  So
# "Performance-critical code changed ... CLAUDE.md requires benchmarking
# these" was, for most files it printed, an instruction that running --bench
# cannot carry out -- the suite has no number for them and comes back green
# whatever the change did.
#
# Worse than coarse: the directory annotations named benchmarks that measured a
# *copy* of the code rather than the code itself.
#
#   net_checksum     -> bench.rs's own internet_checksum()       [FIXED §251]
#   tcp_checksum_v4  -> bench.rs's own tcp_checksum_bench()      [FIXED §251]
#   tcp_checksum_v6  -> bench.rs's own tcp_checksum_v6_bench()   [FIXED §251]
#   dns_build_query  -> bench.rs's own build_dns_query_bench()   [FIXED §251]
#
# Four of the 86 recorded benchmarks measured no kernel file at all -- they
# measured bench.rs.  Making net/dns.rs's real label encoder ten times slower
# left every one of them green.  design-decisions.md §251 deleted all four
# copies and pointed the benchmarks at crate::net; the rules for them in the
# net block below are the result, and are why this paragraph is past tense.
# The finding that motivated it stands as the argument for the rest of this
# map: a benchmark's *name* is not evidence about what it runs.  Two more were
# cited for files they never
# enter: isr_latency's measurement window opens at apic.rs:1059, inside
# handle_timer_irq and well past the idt.rs stub, and page_fault hand-rolls
# map/unmap through page_table with the CPU exception explicitly excluded
# (bench.rs:5450) -- so neither measures kernel/src/idt.rs, and neither
# measures mm/fault.rs, the actual #PF handler.
#
# Hence this map: per-FILE, and read out of bench.rs's call sites rather than
# inferred from a directory name.  The default for anything not listed is "no
# benchmark is known to cover this".  That under-claims -- a file may well be
# exercised incidentally by a benchmark aimed elsewhere -- and under-claiming
# is the correct direction when the documented failure mode of this entire
# mechanism is FALSE ASSURANCE.  A file wrongly listed as uncovered costs one
# line of noise; a file wrongly implied covered costs an unnoticed regression.
#
# Format: "<path prefix>|<space-separated benchmark names>".  Longest matching
# prefix wins, so a directory-wide rule may be added later with per-file
# exceptions.  Every name here is checked against the newest recorded run by
# report_bench_coverage_rot() below: a rule citing a benchmark the suite no
# longer produces is precisely the false assurance this map exists to remove.
BENCH_COVERAGE=(
    # -- mm: 4 of 47 files -------------------------------------------------
    "kernel/src/mm/frame.rs|page_alloc_free page_alloc_zeroed_free page_alloc_zeroed_pool"
    "kernel/src/mm/heap.rs|heap_alloc_free_64 heap_raw_alloc_free_512 heap_raw_alloc_free_4096"
    "kernel/src/mm/compress.rs|compress_zero_page compress_repeating"
    "kernel/src/mm/page_table.rs|page_fault"
    # NOT covered, and deliberately absent: mm/fault.rs (page_fault reproduces
    # the sequence rather than taking a fault), mm/user.rs (nothing in bench.rs
    # calls copy_to_user/copy_from_user), mm/cow.rs, mm/swap.rs, mm/vma.rs, ...
    # mm/frame_owner.rs is absent for a subtler reason: bench.rs does A/B its
    # tagging overhead, but through `timed()`/`ab_interleaved()`, which print
    # and record nothing.  An unrecorded measurement cannot detect a
    # regression, so it is not coverage.

    # -- sched: 4 of 18 files ----------------------------------------------
    "kernel/src/sched/mod.rs|context_switch"
    "kernel/src/sched/context.rs|context_switch"
    "kernel/src/sched/backend.rs|context_switch pick_next sched_pick_next"
    "kernel/src/sched/priority_rr.rs|context_switch pick_next sched_pick_next sched_pick_next_d1 sched_pick_next_d8 sched_pick_next_d64 sched_pick_next_d256"
    # priority_rr.rs earns `context_switch` as well as its own pick_next
    # numbers: PerCpuScheduler::pick_next_local lives there and is on the
    # yield path, dispatching through backend::SchedulerBackend.
    # sched/eevdf.rs is opt-in (BACKEND_EEVDF); the default boot runs
    # PriorityRoundRobin, so no recorded number describes it.  sched/task.rs is
    # absent: the switch reads its types, but the timed work is elsewhere.

    # -- ipc: 10 of 22 files -----------------------------------------------
    "kernel/src/ipc/channel.rs|ipc_channel ipc_channel_sync ipc_channel_roundtrip_64k"
    "kernel/src/ipc/pipe.rs|ipc_pipe"
    "kernel/src/ipc/eventfd.rs|ipc_eventfd"
    "kernel/src/ipc/semaphore.rs|ipc_semaphore"
    "kernel/src/ipc/futex.rs|futex_wake_empty futex_wait_mismatch"
    "kernel/src/ipc/shm.rs|shm_create_close shm_rw_64bytes"
    "kernel/src/ipc/service.rs|service_connect"
    "kernel/src/ipc/completion.rs|cp_try_wait_empty cp_notify_wait_rt"
    "kernel/src/ipc/io_ring.rs|io_ring_nop"
    "kernel/src/ipc/namespace.rs|vfs_stat_breakdown_ns"
    # namespace.rs's rule is thin and knowingly so: vfs_stat_breakdown_ns times
    # namespace::resolve_path, which on the NS_FEATURES_ACTIVE fast path is an
    # atomic load and a return.  It would catch that fast path regressing and
    # nothing else.

    # -- syscall: 2 of 9 files ---------------------------------------------
    "kernel/src/syscall/dispatch.rs|syscall_dispatch"
    "kernel/src/syscall/handlers.rs|syscall_dispatch"
    # syscall/number.rs is absent: SYS_TASK_ID is a compile-time constant, so
    # no instruction from that file is inside the measured window.

    # -- fs: 3 of 467 files ------------------------------------------------
    # `vfs_readdir` was renamed to `vfs_readdir_32` and split from the unscored
    # `vfs_readdir_root`; both are listed because both are recorded and both
    # time Vfs::readdir.  See bench/baselines.toml [vfs_readdir_32].
    "kernel/src/fs/vfs.rs|vfs_read_256 vfs_write_256 vfs_readdir_32 vfs_readdir_root vfs_stat_root vfs_stat_3comp vfs_stat_deep vfs_throughput_16k_read vfs_throughput_16k_write vfs_stat_breakdown_full vfs_stat_breakdown_prologue vfs_stat_breakdown_resolve vfs_stat_breakdown_resolved"
    "kernel/src/fs/path.rs|vfs_stat_breakdown_prologue vfs_stat_breakdown_resolve"
    "kernel/src/fs/compress.rs|http_gzip_1KiB http_gzip_8KiB http_build_response_gzip_1KiB"
    # fs/ext4, fs/zfs, fs/btrfs, fs/f2fs, fs/ntfs and the other ~460 files have
    # no benchmark at all -- see known-issues.md, "The bench gate names fs/zfs
    # as perf-critical, but no benchmark can see it".

    # -- net: 10 of 49 files -----------------------------------------------
    "kernel/src/net/arp.rs|net_arp_lookup net_ns_arp_lookup"
    "kernel/src/net/ethernet.rs|net_ethernet_parse"
    "kernel/src/net/ipv4.rs|net_ipv4_parse net_checksum"
    "kernel/src/net/ipv6.rs|net_ipv6_parse"
    "kernel/src/net/tcp.rs|net_tcp_conn_lookup tcp_checksum_v4 tcp_checksum_v6"
    "kernel/src/net/dns.rs|dns_build_query"
    "kernel/src/net/firewall.rs|firewall_check"
    "kernel/src/net/veth.rs|net_veth_send net_veth_recv net_veth_roundtrip"
    "kernel/src/net/dashboard.rs|dashboard_api_health dashboard_api_metrics dashboard_api_status"
    "kernel/src/net/httpd.rs|http_parse_request http_build_response_1KiB http_build_response_gzip_1KiB http_mime_type http_percent_decode http_etag_4KiB http_gzip_1KiB http_gzip_8KiB"
    # The four checksum/DNS rules above are new as of design-decisions.md §251,
    # which deleted the bench-local copies those benchmarks used to time.  Until
    # then this block said the opposite -- "dns.rs is NOT here: dns_build_query
    # measures bench.rs's copy", and tcp.rs was listed for its connection-table
    # scan only.  The map was right then and is right now; what changed is the
    # code, not the reading of it.  net_checksum is credited to ipv4.rs because
    # it now calls ipv4::ip_checksum; the copy it replaced was
    # character-equivalent, so that series alone is continuous across §251.
    # net/http.rs is absent -- the pre-§250 annotation named it, but the http_*
    # benchmarks all call crate::net::httpd.  So is net/interface.rs, which four
    # benchmarks name but only to construct an Ipv4Addr *outside* the timed
    # closure -- appearing in a benchmark's source is not the same as being
    # inside its measurement.  For the same reason tcp_checksum_v6 is credited
    # to tcp.rs and not to ipv6.rs: it takes an &Ipv6Addr, but the newtype is
    # wrapped before the closure opens.
    # net_ns_arp_lookup is credited to arp.rs and not to netns.rs: the
    # namespace is created and destroyed either side of the closure, which
    # times arp::ns_lookup alone.

    # -- single files ------------------------------------------------------
    "kernel/src/sync.rs|lock_uncontended lock_tracked_nested"
    "kernel/src/lockdep.rs|lock_uncontended lock_tracked_nested"
    # These two rules were added by working the map *backwards* -- asking which
    # recorded benchmarks no rule cites -- which surfaced the inverse of §250's
    # error: coverage that exists while the gate never asks for it.  Both
    # benchmarks time crate::sync's Mutex with lockdep active, so both files are
    # genuinely covered, yet neither was in BENCH_CRITICAL_PATHS: a change to
    # the kernel's lock produced "no perf-critical changes" and skipped a suite
    # that would have caught it.  Worth re-running that backwards check when
    # adding benchmarks; `rdtsc_overhead` is correctly cited by nothing (it
    # measures the harness, not the kernel) and `hpet_read` is a live question
    # -- see known-issues.md.
    "kernel/src/crypto.rs|crypto_sha256_64B crypto_sha256_1KiB crypto_sha512_64B crypto_hmac_sha256 crypto_chacha20_1KiB crypto_poly1305_1KiB crypto_aead_1KiB crypto_ed25519_sign crypto_ed25519_verify crypto_x25519 crypto_crc32_4KiB crypto_crc32c_4KiB"
    "kernel/src/apic.rs|isr_latency"
)

# Longest-prefix lookup: print the benchmarks known to measure $1, or nothing.
bench_coverage_for() {
    local path="$1" best="" best_len=0 rule prefix names
    for rule in "${BENCH_COVERAGE[@]}"; do
        prefix="${rule%%|*}"
        names="${rule#*|}"
        case "$path" in
            "$prefix" | "$prefix"/*)
                if [ "${#prefix}" -gt "$best_len" ]; then
                    best_len="${#prefix}"
                    best="$names"
                fi
                ;;
        esac
    done
    printf '%s' "$best"
}

# Check the map against reality: every benchmark BENCH_COVERAGE claims must
# still be produced by the suite.
#
# Without this the map rots in the one direction that matters.  A benchmark
# that is deleted, renamed, or silently SKIPped at runtime (several print
# "SKIP" and record nothing) leaves its rule behind, and the rule then answers
# "yes, covered" for a file nothing measures -- the same false assurance in a
# new costume.  Checking against the recorded run rather than against bench.rs
# catches the SKIP case too, which a source grep cannot see.
#
# The row is matched as text: it is one line of JSON whose entry keys are the
# quoted benchmark names, so a substring test needs no parser.
report_bench_coverage_rot() {
    local hist="$PROJECT_ROOT/bench/history.jsonl"
    [ -f "$hist" ] || return 0
    local last_row
    last_row="$(grep -a '"commit"' "$hist" | tail -1 || true)"
    [ -n "$last_row" ] || return 0

    local rule names name stale=""
    for rule in "${BENCH_COVERAGE[@]}"; do
        names="${rule#*|}"
        for name in $names; do
            case "$last_row" in
                *"\"$name\""*) ;;
                *) case " $stale " in
                       *" $name "*) ;;
                       *) stale="$stale $name" ;;
                   esac ;;
            esac
        done
    done

    if [ -n "$stale" ]; then
        echo "  !! The benchmark-coverage map in scripts/boot-test.sh cites"
        echo "     benchmarks the last recorded run did not produce:"
        printf '       %s\n' $stale
        echo "     Each one makes the map claim coverage that no number backs."
        echo "     Fix BENCH_COVERAGE, or find out why the benchmark stopped"
        echo "     being recorded (several print SKIP and record nothing)."
    fi
}

# Say — out loud — that this boot produced NO benchmark numbers.
#
# Called only on the PASS paths, and only when --bench was NOT given.  It never
# changes the exit code: a routine boot legitimately skips the suite, because
# --bench roughly doubles the ~405 s TCG cycle.
#
# The point is that "PASSED" must not be readable as "performance was checked".
# It was not: the deferred bench task is spawned on every boot and killed the
# moment BOOT_OK appears, so an ordinary log contains at most the suite's own
# header and never a single result
# (known-issues.md -> TD-BENCHMARKS-ARE-NEVER-ACTUALLY-RUN-BY-THE-BOOT-GATE).
# Same principle as the Path-Z fix above: a silent skip gets believed.
#
# It also answers "should I have run --bench?" instead of leaving it to
# memory.  bench-history.py stamps each recorded run with its git commit, so
# we can diff that commit against HEAD over the perf-critical paths and
# escalate from a one-line note to a real warning only when this boot actually
# contains unbenchmarked changes to code CLAUDE.md says must be benchmarked.
report_bench_absence() {
    local file="$1"
    local hist="$PROJECT_ROOT/bench/history.jsonl"

    # Did the suite at least start before QEMU was torn down?
    local started="no"
    [ -f "$file" ] && grep -qa 'Kernel micro-benchmarks' "$file" && started="yes"

    echo "=== NO BENCHMARK RESULTS THIS RUN (--bench not given) ==="
    if [ "$started" = "yes" ]; then
        echo "  The deferred bench task started but was killed at $WAIT_MARKER before"
        echo "  producing numbers. This run's PASS covers correctness only."
    else
        echo "  The bench task never reached its first result. This run's PASS"
        echo "  covers correctness only."
    fi

    # Escalate only if perf-critical code moved since the last recorded run.
    #
    # `last_dirty` matters as much as `last_commit`: a row written from a tree
    # with uncommitted changes names the nearest *ancestor* of what was actually
    # benchmarked, so `git diff $last_commit HEAD` over-reports (it lists files
    # whose changes were in fact measured, just not committed yet).  That is the
    # safe direction, so the comparison still runs -- but it is said out loud
    # rather than presented as exact.
    local last_commit="" last_row="" last_dirty=""
    if [ -f "$hist" ]; then
        # The last row that *has* a commit, not simply the last row -- the file
        # is appended to from three worktrees and a truncated tail must not be
        # read as "no history".
        last_row="$(grep -a '"commit"' "$hist" | tail -1)"
        last_commit="$(printf '%s' "$last_row" | sed -n 's/.*"commit"[[:space:]]*:[[:space:]]*"\([0-9a-f]*\)".*/\1/p')"
        case "$last_row" in
            *'"dirty": true'*|*'"dirty":true'*) last_dirty=1 ;;
        esac
    fi

    if [ -z "$last_commit" ]; then
        echo "  No previous run in bench/history.jsonl — there is no baseline for this"
        echo "  host yet. Run: ./scripts/boot-test.sh --bench"
        return 0
    fi

    if ! git -C "$PROJECT_ROOT" cat-file -e "${last_commit}^{commit}" 2>/dev/null; then
        echo "  Last recorded run was $last_commit, which is not in this repo"
        echo "  (rebased or not fetched); cannot tell what changed since."
        echo "  Run: ./scripts/boot-test.sh --bench"
        return 0
    fi

    # Compare against the WORKING TREE, not HEAD.
    #
    # `git diff A HEAD` compares two *commits*, so every uncommitted change is
    # invisible to it -- and edit, boot-test, then commit is the normal workflow
    # here, which makes an unbenchmarked change to a listed path most likely to
    # be uncommitted at exactly the moment this gate runs.  So the blind spot was
    # not a corner case; it was the common one.
    #
    # Observed 2026-08-20, with the two halves of the proof one commit apart: a
    # run whose kernel was built from a tree containing a modified
    # kernel/src/mm/user.rs -- a path listed above -- printed "No perf-critical
    # changes since the last benchmarked commit", and the byte-identical tree
    # printed "!! Performance-critical code changed ... kernel/src/mm/user.rs"
    # on the next run, once the file had been committed.  Same code, opposite
    # verdicts, decided by nothing but `git add`.  This is the SILENT false
    # negative the comment on BENCH_CRITICAL_PATHS warns about, arriving through
    # the one door that comment was not watching: not a missing path, a missing
    # *revision*.  The banner at the top of this script already prints
    # `+uncommitted` for this very tree; the gate simply never asked.
    #
    # `git diff <commit> -- <paths>` with no second rev diffs the commit against
    # the working tree, covering committed and uncommitted changes alike.
    # Untracked files need their own query: a brand-new file under a benchmarked
    # path is code the suite has *never* measured, which is the strongest reason
    # to escalate rather than the weakest.
    local changed untracked
    untracked="$(git -C "$PROJECT_ROOT" ls-files --others --exclude-standard -- \
        "${BENCH_CRITICAL_PATHS[@]}" 2>/dev/null || true)"
    changed="$(
        {
            git -C "$PROJECT_ROOT" diff --name-only "$last_commit" -- \
                "${BENCH_CRITICAL_PATHS[@]}" 2>/dev/null || true
            printf '%s\n' "$untracked"
        } | grep -a . | sort -u || true
    )"

    if [ -n "$changed" ]; then
        # Split the list by whether the suite can actually see the file.
        #
        # Printing one undifferentiated list was this gate's other dishonesty.
        # "CLAUDE.md requires benchmarking these ... Run: --bench" reads as an
        # instruction that can be carried out, and for most of the list it
        # cannot: the suite holds no number for those files, so --bench comes
        # back green regardless of what the change did.  Demonstrated on this
        # very tree 2026-08-20 -- the gate named kernel/src/mm/user.rs, the
        # 21-minute --bench cycle it asked for was duly run, and all 86
        # benchmarks completed without once calling copy_to_user or
        # copy_from_user.  A request that cannot be satisfied is worse than no
        # request: it spends the run and hands back an assurance nothing
        # measured.
        local covered="" uncovered="" f names
        while IFS= read -r f; do
            [ -n "$f" ] || continue
            names="$(bench_coverage_for "$f")"
            if [ -n "$names" ]; then
                covered="${covered}${f}  ->  ${names}
"
            else
                uncovered="${uncovered}${f}
"
            fi
        done <<EOF
$changed
EOF

        echo "  !! Performance-critical code changed since the last benchmarked"
        echo "     commit ($last_commit)."
        if [ -n "$covered" ]; then
            local n_cov
            n_cov="$(printf '%s' "$covered" | grep -ac . || true)"
            echo "     $n_cov measured by the suite -- run --bench to compare:"
            printf '%s' "$covered" | head -6 | sed 's/^/       /'
            if [ "$n_cov" -gt 6 ]; then
                echo "       ... and $((n_cov - 6)) more"
            fi
        fi
        if [ -n "$uncovered" ]; then
            local n_unc verb
            n_unc="$(printf '%s' "$uncovered" | grep -ac . || true)"
            verb="are"
            [ "$n_unc" -eq 1 ] && verb="is"
            echo "     $n_unc $verb covered by NO benchmark. Running --bench will not"
            echo "     tell you whether these got slower:"
            printf '%s' "$uncovered" | head -6 | sed 's/^/       /'
            if [ "$n_unc" -gt 6 ]; then
                echo "       ... and $((n_unc - 6)) more"
            fi
        fi
        # Say how many are not in any commit.  It changes what the reader should
        # do: an uncommitted one will not be attributable later, because
        # bench/history.jsonl stamps rows with a commit hash.
        local n_dirty
        n_dirty="$(
            {
                git -C "$PROJECT_ROOT" diff --name-only HEAD -- \
                    "${BENCH_CRITICAL_PATHS[@]}" 2>/dev/null || true
                printf '%s\n' "$untracked"
            } | grep -ac . || true
        )"
        if [ "$n_dirty" -gt 0 ]; then
            local is_are="are"
            [ "$n_dirty" -eq 1 ] && is_are="is"
            echo "     ($n_dirty of these $is_are uncommitted in the tree just built, so a"
            echo "      bench row recorded now would be stamped with an ancestor commit.)"
        fi
        if [ -n "$last_dirty" ]; then
            echo "     (That run measured $last_commit plus uncommitted changes, so some"
            echo "      of the files above may already have been benchmarked.)"
        fi
        # Only ask for the run when the run can answer something.  Recommending
        # a 20-minute --bench cycle for a change no benchmark observes is how
        # this gate came to certify work it had not looked at.
        if [ -n "$covered" ]; then
            echo "     Run: ./scripts/boot-test.sh --bench"
        else
            echo "     Nothing here would be measured by --bench. If these paths"
            echo "     matter for performance, the missing piece is a benchmark,"
            echo "     not a run -- add one and list it in BENCH_COVERAGE above."
        fi
    else
        echo "  No perf-critical changes since the last benchmarked commit ($last_commit),"
        echo "  so skipping the suite is reasonable here."
        if [ -n "$last_dirty" ]; then
            echo "  Caveat: that run's tree was dirty, so it measured $last_commit plus"
            echo "  changes that are not in any commit; 'no changes since' is only as"
            echo "  precise as that."
        fi
    fi
    return 0
}

# `$0` is the snapshot in the temp directory, not this file's home in the tree
# (see the re-exec at the top), so the real directory is passed in rather than
# derived. The fallback keeps the script runnable if it is ever sourced or
# invoked in a way that skips the re-exec.
SCRIPT_DIR="${BOOT_TEST_ORIG_DIR:-$(cd "$(dirname "$0")" && pwd)}"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Every gate below asks a Python checker whether the tree is clean, and until
# 2026-09-01 each asked with a bare `if "$py" .../check-thing.py`, which cannot
# tell "I found violations" from "I raised an exception" -- both exit 1.  So a
# checker that fell over was answered with the gate's own refusal text, over an
# empty finding list, telling the reader to fix code that had never been looked
# at.  Measured that day: `check-selftest-format-wording.py` died of
# `MemoryError` in `strip_noise` while two other lanes were building, and this
# script printed "Each report above is a self-test assertion demanding text that
# no string literal in the kernel can produce" with no reports above it.
#
# `run_checker` is the shared answer, also sourced by scripts/hooks/pre-push --
# one implementation, because this is the same line of code written twice.  It
# is sourced before any gate runs so that a gate can never reach an undefined
# function: that is "command not found", which exits non-zero, which every gate
# reads as a finding.
BOOT_CHECKER_LIB="$SCRIPT_DIR/run-checker.sh"
if [ ! -f "$BOOT_CHECKER_LIB" ]; then
    echo "ERROR: refusing to build.  Cannot find $BOOT_CHECKER_LIB." >&2
    echo "Without it no gate below can tell a finding from a crash." >&2
    exit 1
fi
# Quoted because `boot-test` unquoted reads as the subtraction `boot - test`
# to shellcheck (SC2100), and a warning nobody can act on is a warning everyone
# learns to scroll past.
CHECKER_PROG='boot-test'
CHECKER_REFUSING='build'
CHECKER_LOGDIR="$(git -C "$PROJECT_ROOT" rev-parse --absolute-git-dir 2>/dev/null)" || CHECKER_LOGDIR=""
[ -n "$CHECKER_LOGDIR" ] && [ -d "$CHECKER_LOGDIR" ] || CHECKER_LOGDIR="${TMPDIR:-/tmp}"
export CHECKER_PROG CHECKER_REFUSING CHECKER_LOGDIR
# The directive has to sit immediately above the `.`, not above the block: it
# annotates the next source command, and from twelve lines up it annotates
# nothing (SC1090).
# shellcheck source=run-checker.sh
. "$BOOT_CHECKER_LIB"

# Announce the tree under test, first line of the log, before anything else.
#
# PROJECT_ROOT comes from `dirname "$0"`, so with three worktrees on this
# machine a *relative* invocation (`./scripts/boot-test.sh`) tests whichever
# tree the caller's cwd happened to be — which is not always the one they
# think.  Observed 2026-08-16: a boot test launched to validate uncommitted
# lane-a work ran against the integration tree's kernel from an hour earlier,
# and the only evidence was qemu's `-drive file:` argument.  Fifteen minutes of
# process forensics to learn something the harness knew at startup.
#
# So say it.  A wrong tree, a wrong branch or an unexpectedly clean/dirty
# worktree is then visible in line 1 rather than inferable from a pid tree.
# These three are captured once, here, and are deliberately NOT recomputed when
# the run ends -- they are what the recorders stamp their rows with.  A boot
# test takes ten to twenty minutes and committing during one is normal and
# encouraged, so a recorder that asks git for HEAD on its way out attributes the
# run to a commit that was never built.  Observed 2026-08-18:
# bench/boot-history.jsonl gained a PASS row for 88e93fecf, a commit created
# while QEMU was already running, whose entire content was a paragraph of
# known-issues.md.
#
# That is not a cosmetic mislabel.  report_bench_absence() above diffs HEAD
# against the last recorded commit to decide whether perf-critical code needs
# re-benchmarking, so a row stamped *newer* than the tree it measured hides
# precisely the changes the check exists to catch -- and it fails silently, by
# printing the reassuring branch.
#
# The two files this harness itself writes are excluded from the dirty check,
# and that exclusion is the whole point of the pathspec below.  Both are
# tracked; both are appended to by the recorders at the *end* of a run.  So a
# clean tree stops being clean the moment the first run finishes, and every
# subsequent run at the same commit stamps itself `"dirty": true` — not because
# anything under test changed, but because the previous run recorded itself.
# The signature is visible in bench/history.jsonl: 40515da89 has one clean row
# followed by five dirty ones, 5e9a30a22 one clean then one dirty, with no
# source commit in between.
#
# It is not a cosmetic mislabel either.  `dirty` means "the commit hash does not
# identify the source that was built", and consumers act on it: layout_arms()
# in bench-history.py drops dirty records outright, because a layout band is
# only meaningful across arms that share identical source.  A six-arm sweep
# would therefore have contributed exactly one usable arm — one below the
# three-arm minimum — and reported no band at all, silently, after three hours
# of QEMU.  A check that cannot fire, presenting as a check that found nothing.
#
# Excluding them is safe in the direction that matters.  Widening `dirty` hides
# nothing; narrowing it admits records into comparisons.  These two files are
# never compiled and never read by the kernel, so no edit to them can change
# what was built — which is precisely and only what `dirty` is claiming about.
BT_BRANCH="$(git -C "$PROJECT_ROOT" rev-parse --abbrev-ref HEAD 2>/dev/null || echo '?')"
BT_HEAD="$(git -C "$PROJECT_ROOT" rev-parse --short HEAD 2>/dev/null || echo '?')"
BT_DIRTY=0
git -C "$PROJECT_ROOT" diff --quiet HEAD -- . \
    ':(exclude)bench/history.jsonl' \
    ':(exclude)bench/boot-history.jsonl' 2>/dev/null || BT_DIRTY=1
_bt_dirty=""
[ "$BT_DIRTY" = 1 ] && _bt_dirty=" +uncommitted"
echo "=== Tree under test: $PROJECT_ROOT [$BT_BRANCH @ $BT_HEAD$_bt_dirty] ==="
unset _bt_dirty

# A digest of the source this run is about to build, recorded alongside the
# commit because the commit is not an identity and never was.
#
# Two things it fixes, both of which have already cost a sweep:
#
#   - `commit` SPLITS identical builds.  A layout sweep spends ~75 min per arm,
#     so any commit landing while it runs -- a documentation commit will do --
#     gives later arms a different hash.  bench-history.py's layout_arms() then
#     groups six identical arms into six one-pad groups and reports no band at
#     all, silently.  That is exactly what happened to the WHPX sweep of
#     2026-08-19, whose six arms carry six different commits.
#
#   - `dirty` MERGES different builds.  `git diff --quiet HEAD` cannot see
#     untracked files, and the kernel include_bytes!s six gitignored service
#     binaries while every boot attaches rootfs.ext4.  Rebuild a service
#     between two arms and both rows still say `dirty: false` at one commit.
#
# Captured HERE, before the build, for the same reason BT_HEAD is: the record
# must describe the source that went *into* the kernel under test.  Re-deriving
# it at exit would describe whatever the tree became afterwards, and would do
# so in the direction that hides changes.
#
# Never fatal.  A missing digest is recorded as absent, which downstream treats
# as "unknown" and therefore refuses to group -- the safe direction.  A boot
# test that failed because a digest could not be computed would be strictly
# worse than one that simply did not record it.
BT_SRC_DIGEST=""
if command -v python &>/dev/null; then
    BT_SRC_DIGEST="$(python "$PROJECT_ROOT/scripts/src_digest.py" \
                     --root "$PROJECT_ROOT" 2>/dev/null || true)"
elif command -v python3 &>/dev/null; then
    BT_SRC_DIGEST="$(python3 "$PROJECT_ROOT/scripts/src_digest.py" \
                     --root "$PROJECT_ROOT" 2>/dev/null || true)"
fi
if [ -n "$BT_SRC_DIGEST" ]; then
    echo "=== Source digest: $BT_SRC_DIGEST ==="
else
    echo "=== Source digest: unavailable (rows will not be groupable) ==="
fi

# Convert to Windows paths if running under MSYS/Git Bash (QEMU needs them).
to_win_path() {
    if command -v cygpath &>/dev/null; then
        cygpath -w "$1"
    else
        echo "$1"
    fi
}

# Default (debug) artefact path.  Reassigned unconditionally after arg parsing
# from the resolved BENCH_PROFILE (--profile=, else --bench's default) — see the
# CARGO_PROFILE_ARGS block below for why.  This initial value therefore only
# matters if that block is ever bypassed; it is kept in sync deliberately.
KERNEL_BIN="$PROJECT_ROOT/target/x86_64-unknown-none/debug/kernel"
ESP_DIR="$PROJECT_ROOT/build/esp"
# --usb-image only: the real GPT/FAT32 image built from $ESP_DIR by
# scripts/build-usb-image.py, attached as a USB mass-storage device instead of
# QEMU's virtual FAT.  See the --usb-image case in the arg parser.
USB_IMG="$PROJECT_ROOT/build/slateos-usb.img"
SERIAL_FILE="$PROJECT_ROOT/build/serial-test.txt"
# QEMU's own stderr, which until now was captured nowhere -- it went to the
# harness console and died with it.
#
# It is the only place a *host* failure announces itself.  On 2026-09-01 a boot
# died because Windows could not grow its paging file fast enough for QEMU's
# memory backend; QEMU printed "Failed to CreateFileMapping", the guest saw its
# GPU refuse an allocation, a boundary self-test correctly called that fatal,
# and boot-history wrote the run down as "kernel died" and zeroed a nine-boot
# clean streak.  Every layer behaved correctly and the recorded conclusion was
# false (known-issues.md -> "A host out-of-memory during QEMU is recorded as a
# kernel PANIC").
#
# It must be a FILE, and it must be qemu's stderr rather than the serial log:
# the guest writes the serial log, so a kernel that printed the same words could
# otherwise excuse itself.  This stream is written by the emulator and by MSYS,
# below the guest, where nothing in the tree can reach it.
QEMU_STDERR="$PROJECT_ROOT/build/qemu-stderr.txt"
# The serial line each conditionally-called self-test declares it prints, from
# `check-self-tests-wired.py --emit-markers` in the pre-build gates; read by
# boot-history.py at the end of the run.  Gitignored build output, not a
# checked-in fact: it is derived from main.rs and would go stale the moment a
# gated call site moved.
GATED_MARKERS="$PROJECT_ROOT/build/gated-markers.json"
# Set to 1 by check_self_tests_wired once it has regenerated the file above.
# Only then may the recorder be pointed at it; see record_boot_history.
GATED_MARKERS_FRESH=0
# QEMU writes its OS-level PID here so we can reap it reliably.  Under MSYS,
# `kill "$!"` uses the Cygwin PID and does NOT reliably TerminateProcess a
# native (non-Cygwin) qemu-system-x86_64.exe: the emulator survives as an
# orphan that keeps its `-serial file:` handle open, so the NEXT boot's
# `rm serial-test.txt` fails with "Device or resource busy" and its qemu
# dies instantly.  This silently broke every repeated run (e.g. wedge-soak).
# The pidfile holds qemu's real Windows PID; kill_qemu() taskkills it.
PIDFILE="$PROJECT_ROOT/build/qemu.pid"
# QEMU args need Windows paths
ESP_DIR_WIN="$(to_win_path "$ESP_DIR")"
USB_IMG_WIN="$(to_win_path "$USB_IMG")"
SERIAL_FILE_WIN="$(to_win_path "$SERIAL_FILE")"
PIDFILE_WIN="$(to_win_path "$PIDFILE")"

# Reliably terminate the QEMU launched by this script.
#
# $1 = the MSYS/Cygwin PID from `$!` (used for the `wait` and a first,
# best-effort Cygwin-side kill).  We then read the OS PID that qemu wrote to
# its -pidfile and `taskkill //F //PID` it, which is the only thing that
# reliably kills a native Windows qemu from MSYS.  Idempotent.
#
# A missing pidfile is the NORMAL case on a clean exit, not an anomaly: qemu
# unlinks it on the way out.  There is deliberately no kill-by-image-name
# fallback -- this file may run while another lane's boot test has its own qemu
# alive, and killing by image name would take theirs down with ours.  Losing
# the pid means the process we wanted to kill has already gone.
kill_qemu() {
    local cyg_pid="${1:-}"
    # Stamp the end of the QEMU window at the FIRST teardown, not the last.
    # kill_qemu is idempotent and is called again from the EXIT trap, so an
    # unconditional stamp here would measure the harness's own post-run log
    # processing as if it were guest time.  Every exit path funnels through
    # this function, which is why the stamp lives here rather than being
    # repeated at each of them (one of which would eventually be missed).
    if [ -z "${QEMU_END_EPOCH:-}" ] && [ -n "${QEMU_START_EPOCH:-}" ]; then
        QEMU_END_EPOCH=$(date +%s)
    fi
    # Best-effort Cygwin-side signal first (harmless if it does nothing).
    [ -n "$cyg_pid" ] && kill "$cyg_pid" 2>/dev/null || true
    # Authoritative kill via the OS PID qemu recorded in its pidfile.
    #
    # Read it unconditionally rather than testing for it first.  QEMU unlinks
    # its own -pidfile as it exits, so a test-then-read races the very teardown
    # this function performs -- and loses most often on the *happy* path, where
    # the guest reached BOOT_OK and qemu is already on its way out under its own
    # power.  An absent file yields an empty pid, which the `-n` below already
    # handles, so the test bought nothing and only read as though it made the
    # line beneath it safe.
    #
    # The redirect is on the group, not on `tr`.  A failed input redirection is
    # diagnosed by the shell, which never execs `tr` at all -- so `tr`'s own
    # `2>/dev/null` is not in effect when the message is produced, and it went
    # to the script's stderr while `|| true` swallowed the status.  That is how
    # this printed a bare "line 1329: build/qemu.pid: No such file or directory"
    # on runs that passed, with nothing else wrong to explain it.
    local win_pid
    win_pid="$( { tr -cd '0-9' < "$PIDFILE"; } 2>/dev/null || true)"
    if [ -n "$win_pid" ]; then
        taskkill //F //PID "$win_pid" >/dev/null 2>&1 || true
    fi
    # Reap the Cygwin-side child so the shell doesn't leave a zombie/handle.
    [ -n "$cyg_pid" ] && wait "$cyg_pid" 2>/dev/null || true
    rm -f "$PIDFILE" 2>/dev/null || true
}

# Replay whatever QEMU wrote to stderr, once, on the way out.
#
# Redirecting qemu's stderr to a file (see QEMU_STDERR) is what lets
# boot-history.py tell a host failure from a kernel one, but the redirect would
# otherwise COST the operator something they had before: those lines used to
# land in the harness console live.  "Failed to CreateFileMapping" is the whole
# explanation for a run that is about to look like an unexplained hang, and
# silently filing it in build/ where nobody looks would trade one blindness for
# another.  So the file is the record and this is the echo; both, not either.
#
# Printed to stderr, under a banner, so it cannot be mistaken for guest output
# in a scrollback -- and only when non-empty, since qemu is silent on a healthy
# run and an empty banner every boot is noise that trains the reader to skip it.
echo_qemu_stderr() {
    [ -s "$QEMU_STDERR" ] || return 0
    {
        echo "--- qemu stderr (build/qemu-stderr.txt) ---"
        cat "$QEMU_STDERR"
        echo "--- end qemu stderr ---"
    } >&2 || true
    return 0
}
# Default boot timeout.  The boot path runs the full self-test suite before
# printing BOOT_OK, including the Path-Z ring-3 toolchain tests (each spawns a
# real glibc/tcc/make/dash process under ld.so), which now dominate boot time.
#
# This number has to be maintained.  It was 480s against a measured ~305s
# boot; by 2026-08-14 a healthy boot reached BOOT_OK at ~456s — a 24s margin,
# and the in-kernel liveness detector was already firing "BOOT DEADLINE
# EXCEEDED" with a task-table dump on every clean run.  The failure mode when
# the margin runs out is nasty: a perfectly healthy kernel is killed mid-boot
# and reported as a hang, which costs a diagnosis cycle before anyone thinks
# to check the clock.  So the default is set at roughly 2x the observed boot,
# not just above it.
#
# Detecting a *real* hang quickly is not this knob's job — that is
# --stall-secs=N, which watches for the serial log going silent and does not
# care how long a healthy boot takes.
#
# Measured: BOOT_OK at ~456s (2026-08-14, TCG, qemu64), which is where the old
# 900 came from.  Re-measured 2026-08-31: BOOT_OK at 1043s on a host where
# another lane was building concurrently -- i.e. the suite outgrew the budget,
# and 900 had stopped being 2x anything.  It was killing healthy boots: two
# runs that day died at exactly 900s, both reported by the harness as
# "STILL PRODUCING OUTPUT ... a budget that was too small, not a hang".
#
# Sized against the *tail*, not the median, because the tail is what trips the
# gate.  `--boot-history` over 368 debug/none TCG boots gives median 438s with
# a range of 18-1175s: the median is unchanged since 2026-08-14, so this is not
# a boot that got uniformly slower, it is one whose spread now crosses 900.
# 2x the observed maximum, per the rule above, is ~2350.
#
# Erring high is the cheap direction.  Too low kills a healthy kernel and
# reports it as a hang -- and because this gate runs before anything is built,
# a false red here blocks every lane, not just the one that tripped it.  Too
# high only delays the verdict on a genuine hang, which is not this knob's job
# anyway (see --stall-secs above, and the in-kernel liveness detector, both of
# which catch a real wedge without waiting for this clock).
TIMEOUT=2400
# Did the caller pass --timeout= explicitly?  Only used to decide whether
# --bench may raise the default (see BENCH_TIMEOUT below); an explicit
# --timeout= always wins.
TIMEOUT_EXPLICIT=0
# Timeout used when --bench is given and --timeout= is not.  The benchmark
# suite runs *after* BOOT_OK, as a deferred low-priority task, and it is not
# cheap under TCG: the asymmetric-crypto benchmarks alone are tens of seconds
# each (ed25519_sign averages ~433ms per iteration for 50 iterations).  A
# --bench run at the 480s boot default therefore reaches BOOT_OK and is then
# killed part-way through the crypto section, reporting "Boot test FAILED
# (BENCH_OK not reached)" for a kernel that booted perfectly.  That is a
# harness false negative, and it happened.
BENCH_TIMEOUT=1200
NO_BUILD=0
NO_STAGE=0
BENCH=0
# Serial-stall wedge detector (opt-in; 0 = disabled).  A genuinely wedged kernel
# stops emitting serial output, whereas a merely-slow boot keeps printing as it
# grinds through the self-test suite.  When --stall-secs=N (N>0) is set, the wait
# loop watches the serial log's growth: if it goes silent for N consecutive
# seconds while QEMU is still alive and the wait marker has not appeared, that is
# a real hang (not just a slow host) — we capture the frozen RIP and exit 2 with
# a distinct "WEDGE: serial stalled" verdict.  This is the discriminator the
# corruption-hunt soak needs so a slow-but-healthy boot is never mistaken for a
# wedge (which would abort the whole campaign on a false positive).  Off by
# default so normal/shared harness runs are byte-for-byte unchanged.
STALL_SECS=0
# Attach the QEMU i6300esb PCI watchdog (inject-nmi on timeout)?  OFF by
# default so the shared harness is byte-for-byte unchanged on normal runs;
# only --hard-lockup-watchdog opts in (see Q20 in open-questions.md).
HARD_LOCKUP_WATCHDOG=0
# Attach QEMU's HMP monitor on a TCP socket so a timeout or a serial stall can
# read the frozen guest's RIP straight out of the emulator?  ON by default;
# --no-monitor opts out.  Unlike the watchdog above this adds no guest device
# and is invisible to the guest — see the MONITOR_ARGS block for why the two
# used to be coupled and why that was wrong.
MONITOR_ENABLED=1
# Which serial marker the wait loop treats as "boot finished".  Default is
# BOOT_OK (the fast path); --bench switches it to BENCH_OK so we wait for the
# deferred micro-benchmark task to finish and can scrape its numbers.
WAIT_MARKER="BOOT_OK"
# Explicit build-profile override, empty unless --profile=... is given.  See the
# profile-selection block below for why this exists separately from --bench.
PROFILE_REQ=""
# What the caller asserts the host was doing during the QEMU window.  Default
# "unknown" deliberately: "nobody said" must never be silently upgraded to "the
# host was quiet", which is the error that let a run taking 2.3x as long as its
# own twin be written up as the cleanest run the instruments could describe.
# See known-issues.md B-CANARY-IS-BLIND-TO-HOST-DESCHEDULING.
HOST_LOAD="unknown"
# Refuse to start a build below this many GiB free on the build volume
# (0 disables).  Q47 option C in open-questions.md, which is Lane A's to take
# unilaterally because it is purely protective: it frees nothing and changes no
# build, it only converts a corrupting failure into an honest refusal.
#
# WHY, concretely: on 2026-08-15 `D:` reached *zero* bytes free.  An edit that
# was half-written when the space ran out left a kernel source file **empty** —
# 18 KB of code replaced by nothing.  That one was already committed and came
# back from git in under a minute; five other files being edited at that moment
# were not committed and would have been lost outright.
#
# A disk-full build does not fail cleanly, which is the real argument for a
# floor rather than a post-hoc check.  A link step that dies part-way can leave
# a stale kernel image staged in the ESP, and a later --no-build run then boots
# that image *as if it were current* — so the harness reports on code that was
# never compiled.  Failing before the build starts is the only point at which
# that is cheap.
#
# 20 GiB is the figure proposed in Q47 and is a floor, not an estimate of what a
# build needs: measured the same day, the four worktrees held 138 GB of build
# output between them (59.1 / 40.4 / 35.0 / 3.5), so 20 GiB is well under one
# full rebuild of all four.  It is meant to leave enough room that the *editor*
# and git keep working while a build is refused — recovering costs a
# `cargo clean`, and losing an uncommitted file costs the work.
MIN_FREE_GB="${BOOT_TEST_MIN_FREE_GB:-20}"

# The same floor, for the volume the *toolchain* writes its scratch to, which
# on this machine is not the volume the tree is on.
#
# Empty means "a quarter of MIN_FREE_GB", resolved after arg parsing so that
# --min-free-gb= moves both.  Deliberately much lower than the tree's floor:
# 20 GiB is sized against one full rebuild of all four worktrees, and scratch
# is nowhere near that, so a guard that refused to build on a machine with 15
# GiB of temp free would be a worse bug than the one it fixes.
MIN_FREE_TEMP_GB="${BOOT_TEST_MIN_FREE_TEMP_GB:-}"

# Opt-in: when the floor trips, run scripts/reclaim-space.py and retry once
# instead of refusing outright.  Off by default because freeing space deletes
# another tree's build output, and a run should not do that merely because it
# happened to be the one that noticed.  See check_tree_free_space.  It applies
# to the tree's volume only: the toolchain's scratch volume is checked too, but
# never reclaimed -- see check_temp_free_space for why.
RECLAIM_SPACE="${BOOT_TEST_RECLAIM_SPACE:-0}"

# After a green run, if free space has fallen below this, prune *this*
# worktree's build cache of units cargo has not invoked in a fortnight.
# 0 disables.
#
# WHY THIS IS ON BY DEFAULT WHEN --reclaim-space IS NOT.  They answer opposite
# problems.  --reclaim-space fires at the floor and its only remedy is to
# delete another tree's build output, which is why a run should not do it
# merely because it noticed.  This one fires far *above* the floor, touches
# only the tree the run just built, and cannot cost anybody a rebuild: cargo
# does not garbage-collect, so a unit it has not asked for in fourteen days --
# across every build every lane ran in that fortnight -- is one it will not
# ask for now.  See scripts/prune-build-cache.py for why "invoked" is the
# sound test and mtime is not.
#
# WHY IT IS NEEDED AT ALL.  CLAUDE.md has told the lanes to clean up after
# themselves since August, and on 2026-09-02 two worktrees were over 100 GB and
# a third just under 50.  A rule nothing enforces is not a rule.  The reason it
# went unnoticed is structural rather than negligent: cargo mints a new
# -<hash> artifact whenever a unit's inputs change and keeps the old one
# forever, so the growth is invisible in any single build and shows up only as
# a volume that is mysteriously full weeks later.  Nothing was ever going to
# notice it by hand.
#
# WHY *HERE*.  The prune's cost is minutes of metadata I/O, so it must not sit
# in front of anybody's feedback: at the head of the run it would delay every
# build, and at the floor it is already too late to be the gentle option.  A
# green finish is the one moment when the run is over, the operator is not
# waiting on the next line, and the tree is in a known state.
#
# WHY 100 AND NOT THE FLOOR.  A full debug build of this workspace is ~40 GiB
# and there are four worktrees, so 100 GiB is roughly "less than two more
# builds of headroom" -- late enough to be rare, early enough that the cheap
# remedy still has room to work.  At the 20 GiB floor the only remedy left is
# deleting a live tree.
PRUNE_CACHE_BELOW_GB="${BOOT_TEST_PRUNE_CACHE_BELOW_GB:-100}"

# Opt-in: when a git-ignored prerequisite is missing, run
# scripts/bootstrap-worktree.sh and continue instead of refusing.  Off by
# default for the same reason --reclaim-space is: provisioning builds six
# crates and may clone a bootloader over the network, and a run should not
# spend minutes on that merely because it happened to be the one that noticed.
# See check_prerequisites.
BOOTSTRAP="${BOOT_TEST_BOOTSTRAP:-0}"

# Boot a real GPT/FAT32 disk image over USB instead of QEMU's virtual FAT.
# See the --usb-image case in the arg parser for why this is not the default.
USB_IMAGE=0

# Suppress the rootfs.ext4 attachment even when the file is present.
# See the --no-rootfs case in the arg parser.
NO_ROOTFS=0

# Parse args
for arg in "$@"; do
    case "$arg" in
        --no-build) NO_BUILD=1 ;;
        # --no-stage implies --no-build: boot exactly the image already in the
        # ESP, touching neither the compiler nor build/esp.  This is what makes a
        # long soak reproducible.  `--no-build` alone is NOT enough: staging runs
        # unconditionally, so a `cargo build` in another terminal silently swaps
        # the kernel under a running soak mid-experiment (this happened — see the
        # note on B-KNULLJUMP-SIGNAL).
        --no-stage) NO_BUILD=1; NO_STAGE=1 ;;
        --bench) BENCH=1; WAIT_MARKER="BENCH_OK" ;;
        # --profile decouples "which build to measure" from "how long to wait".
        # Those are independent questions that --bench used to answer jointly,
        # which left one combination unreachable: a DEBUG build waited on to
        # BENCH_OK.  That gap was not academic -- bench.rs carries per-profile
        # budgets whose debug branch could not be exercised at all, so a
        # mis-sized debug constant would go unnoticed until it fired on someone
        # else's ordinary boot.  Rejecting an unknown value rather than falling
        # back to a default: a typo'd --profile=relese must not silently measure
        # the other profile and label the record with it.
        --profile=*)
            PROFILE_REQ="${arg#*=}"
            case "$PROFILE_REQ" in
                debug|release) ;;
                *) echo "ERROR: --profile must be 'debug' or 'release', got '$PROFILE_REQ'" >&2
                   exit 1 ;;
            esac
            ;;
        --timeout=*) TIMEOUT="${arg#*=}"; TIMEOUT_EXPLICIT=1 ;;
        --stall-secs=*) STALL_SECS="${arg#*=}" ;;
        --hard-lockup-watchdog) HARD_LOCKUP_WATCHDOG=1 ;;
        --no-monitor) MONITOR_ENABLED=0 ;;
        --host-load=*) HOST_LOAD="${arg#*=}" ;;
        --min-free-gb=*) MIN_FREE_GB="${arg#*=}" ;;
        --min-free-temp-gb=*) MIN_FREE_TEMP_GB="${arg#*=}" ;;
        --reclaim-space) RECLAIM_SPACE=1 ;;
        --prune-cache-below-gb=*) PRUNE_CACHE_BELOW_GB="${arg#*=}" ;;
        --no-prune-cache) PRUNE_CACHE_BELOW_GB=0 ;;
        --bootstrap) BOOTSTRAP=1 ;;
        # --usb-image boots the same bytes a flash drive would hold.
        #
        # The default path hands QEMU `fat:rw:build/esp`, which synthesises a
        # filesystem from a host directory.  Everything a real firmware must
        # parse before it can reach the kernel -- the protective MBR, both GPT
        # copies, the FAT32 BPB, the on-disk directory entries -- is therefore
        # produced by QEMU and never by us, so a defect in any of it is
        # invisible here and fatal on hardware.  This flag builds the real image
        # with scripts/build-usb-image.py and attaches it as a `usb-storage`
        # device on the xHCI controller the harness already has, which is how
        # firmware will actually see a stick: enumerated over USB, not as a
        # SATA disk.  OVMF is an independent GPT+FAT32 implementation and Limine
        # is a second one, so a boot that reaches the kernel here has had the
        # image validated twice by code that is not ours.
        #
        # Kept opt-in rather than made the default: the virtual-FAT path needs
        # no image rebuild, which is what makes the ordinary edit-boot loop
        # fast, and a soak wants the ESP it already has (see --no-stage).
        --usb-image) USB_IMAGE=1 ;;
        # --no-rootfs boots without rootfs.ext4, which is the shape a USB stick
        # actually has.
        #
        # Step 3b attaches the rootfs on the file's mere existence -- there was
        # no way to say "not this time" short of renaming it, which races every
        # other process in the worktree.  But a flash drive carries one FAT32
        # ESP and nothing else: there is no second virtio-blk disk for a stick,
        # so every Path-Z rung that reads /mnt will behave on hardware the way
        # it behaves here with this flag, and no other way.  Pairing this with
        # --usb-image is what makes a QEMU run actually rehearse the bare-metal
        # configuration rather than a strictly more capable one.
        #
        # READ THE RESULT CAREFULLY.  Removing the rootfs removes the rungs that
        # currently fail (the inherited posix_spawn_file_actions_init crashes),
        # so such a run reads *greener* than the tracking run while testing
        # strictly less.  That is exactly the confusion the experiment tag
        # exists to prevent, so record_boot_history() adds one -- the run is
        # excluded from the consecutive-clean streak in both directions.
        --no-rootfs) NO_ROOTFS=1 ;;
    esac
done

# A quarter of the tree's floor, unless the caller named one.  Resolved here
# rather than at the assignment above so that --min-free-gb=N moves both, which
# is what someone raising or lowering "the floor" means.  A quarter because the
# scratch volume holds only in-flight temporaries, not the target/ trees the
# tree's floor is sized for.
#
# Both floors are rejected outright if they are not numbers, rather than being
# coerced or left to fail later.  `[ "$avail_gb" -lt twenty ]` prints "integer
# expression expected" to stderr and evaluates *false*, and `set -e` does not
# catch it because it sits in an `if` -- so a typo'd --min-free-gb= would
# silently switch the guard off while the run looked entirely normal.  That is
# precisely the ok/unknown conflation the three-outcome rule below exists to
# stop, so it must not be reintroduced by the floor itself.
for _floor_var in MIN_FREE_GB MIN_FREE_TEMP_GB; do
    eval "_floor_val=\$$_floor_var"
    # SC2154 (referenced but not assigned): the assignment is the `eval` on the
    # line above, which shellcheck does not follow.  It suggests `_floor_var`,
    # which is the loop variable and would compare the floor's *name* against
    # the digits -- i.e. the suggested "fix" silently disables the check this
    # loop exists to perform.
    # shellcheck disable=SC2154
    case "$_floor_val" in
        # Empty MIN_FREE_TEMP_GB is the documented "use the default" value and
        # is resolved just below; empty MIN_FREE_GB is not, it has a default.
        '') [ "$_floor_var" = "MIN_FREE_TEMP_GB" ] && continue ;;
        *[!0-9]*) ;;
        *) continue ;;
    esac
    echo "ERROR: $_floor_var must be a whole number of GiB (got '$_floor_val')." >&2
    echo "Set it with --min-free-gb=N / --min-free-temp-gb=N, or the matching" >&2
    echo "BOOT_TEST_* variable.  0 disables that floor." >&2
    exit 1
done
unset _floor_var _floor_val

if [ -z "$MIN_FREE_TEMP_GB" ]; then
    MIN_FREE_TEMP_GB=$((MIN_FREE_GB / 4))
fi

# Free-space floor (Q47 option C).  See MIN_FREE_GB above for why.
#
# Two volumes can run a build out of room: the one holding the tree, and the
# one the toolchain writes scratch to.  They are usually the same and often
# are not; check_free_space checks both.
#
# Reports one of three outcomes and never conflates them, because "the check
# could not run" reads exactly like "the check passed" if you let it:
#   ok      — measured, and above the floor
#   refuse  — measured, and below it (exit 1 before anything is built)
#   unknown — df did not produce a number; warn loudly and continue
#
# Continuing on `unknown` is deliberate: this guard is protective, not
# load-bearing, and a df that cannot parse must not be able to block every boot
# test on every machine.  But it says so, rather than printing nothing and
# letting a silent skip pass for a clean bill of health.
#
# Split out from check_tree_free_space so the floor can be re-tested after a
# reclaim without duplicating the parsing or recursing back into the refusal
# path, and so the temp volume can be measured with the same code.  Takes the
# directory to measure, defaulting to the tree.  Prints the free space in GiB
# and returns 0; prints nothing and returns 1 if df could not produce a number.
measure_free_gb() {
    # -P forces POSIX single-line output: without it a long filesystem name
    # wraps onto its own line and $4 is then the wrong column.
    local target="${1:-$PROJECT_ROOT}"
    local avail_kib
    avail_kib="$(df -Pk "$target" 2>/dev/null | awk 'NR==2 {print $4}')"
    case "$avail_kib" in
        ''|*[!0-9]*) return 1 ;;
    esac
    echo $((avail_kib / 1024 / 1024))
}

# Identify the volume behind a directory, as "<filesystem> <mount point>".
#
# Both columns, because neither alone is a reliable identity: under MSYS the
# filesystem column is the drive letter (`D:`) and the mount point is a
# synthetic `/d`, while on Linux the filesystem is a device node and the mount
# point is the meaningful half.  Comparing the pair is right on both.
# Prints nothing and returns 1 if df gave no usable row.
volume_id_for() {
    local id
    id="$(df -Pk "$1" 2>/dev/null | awk 'NR==2 {print $1" "$6}')"
    case "$id" in
        ''|' ') return 1 ;;
    esac
    printf '%s\n' "$id"
}

# The directory the toolchain will use for scratch.
#
# rustc/LLVM temporaries, the linker's intermediates and cargo's last-use
# database all land here.  POSIX order: TMPDIR, then TMP, then TEMP, then
# /tmp.  A Windows-shaped value (`C:\Users\...`) is converted with cygpath when
# one is available, because df cannot read a backslash path.
resolve_temp_dir() {
    # ${x:-} on every one of them: this script runs under `set -u`, and all
    # three are genuinely unset on a bare Linux shell, so a bare $TMPDIR here
    # aborts the whole run with "unbound variable" before anything is built.
    local d
    for d in "${TMPDIR:-}" "${TMP:-}" "${TEMP:-}"; do
        [ -n "$d" ] || continue
        case "$d" in
            [A-Za-z]:[\\/]*)
                if command -v cygpath >/dev/null 2>&1; then
                    d="$(cygpath -u "$d" 2>/dev/null || printf '%s' "$d")"
                fi
                ;;
        esac
        [ -d "$d" ] && { printf '%s\n' "$d"; return 0; }
    done
    printf '/tmp\n'
}

# Second volume: the toolchain's scratch space.  Same ok/refuse/unknown
# trichotomy as the tree's floor, folded into the same reporting rather than
# run as a parallel path.
#
# Filed by lane B 2026-08-19 (requests/b-a-free-space-floor-does-not-check-the-
# compiler-s-temp-volume.md) after a run printed
#
#     Free space OK: 47 GiB on the build volume
#     ...
#     rustc-LLVM ERROR: out of memory
#
# with C: -- not the build volume -- at zero bytes free.  That message is the
# reason this is worth a check rather than being left to fail on its own: "out
# of memory" sends you to RAM, to parallelism, to a runaway build, anywhere
# except the disk, which the run has just finished certifying.  The same
# condition reached a host-target crate as "No space left on device", so the
# diagnosis you get depends on which crate happens to hit it first.
#
# No reclaim here, deliberately, and not merely as scope.  reclaim-space.py
# deletes build output under this project's worktrees; the temp volume is the
# operator's system drive, holding VM images and installed software that this
# script has no business forming an opinion about.  Checking is safe; reclaiming
# there is not.
check_temp_free_space() {
    local phase="$1"
    [ "$MIN_FREE_TEMP_GB" = "0" ] && return 0

    local tmpdir tmp_vol root_vol
    tmpdir="$(resolve_temp_dir)"

    if ! tmp_vol="$(volume_id_for "$tmpdir")"; then
        echo "WARNING: could not identify the volume behind the toolchain's temp" \
             "directory ($tmpdir); the ${MIN_FREE_TEMP_GB} GiB scratch floor is" \
             "NOT being enforced for this run." >&2
        return 0
    fi

    # The single-volume machine is the common case and must not print two lines
    # that look like two checks -- the point of naming the volume at all is that
    # the reader can tell which one a number refers to.
    if root_vol="$(volume_id_for "$PROJECT_ROOT")" && [ "$tmp_vol" = "$root_vol" ]; then
        if [ "$MIN_FREE_GB" = "0" ]; then
            echo "WARNING: the toolchain's temp directory ($tmpdir) is on the build" \
                 "volume ${tmp_vol%% *}, whose floor is disabled; the" \
                 "${MIN_FREE_TEMP_GB} GiB scratch floor is NOT being enforced" \
                 "for this run." >&2
        else
            echo "Toolchain temp ($tmpdir) is on the build volume ${tmp_vol%% *}; the build-volume floor covers it."
        fi
        return 0
    fi

    local tmp_gb
    if ! tmp_gb="$(measure_free_gb "$tmpdir")"; then
        echo "WARNING: could not measure free space on $tmpdir (df gave no usable" \
             "number); the ${MIN_FREE_TEMP_GB} GiB scratch floor is NOT being" \
             "enforced for this run." >&2
        return 0
    fi

    if [ "$tmp_gb" -lt "$MIN_FREE_TEMP_GB" ]; then
        echo "" >&2
        echo "ERROR: only ${tmp_gb} GiB free on ${tmp_vol%% *}, which is where the toolchain" >&2
        echo "writes its scratch files ($tmpdir); the floor for that volume is" >&2
        echo "${MIN_FREE_TEMP_GB} GiB (${phase})." >&2
        echo "" >&2
        echo "This is a different volume from the tree, which was checked separately and is" >&2
        echo "fine.  Refusing anyway: rustc reports this condition as" >&2
        echo "  rustc-LLVM ERROR: out of memory" >&2
        echo "which costs far more to diagnose than it costs to refuse here." >&2
        echo "" >&2
        echo "reclaim-space.py is NOT the remedy for this one -- it only knows about build" >&2
        echo "output under this project's worktrees, and nothing it can delete lives on" >&2
        echo "${tmp_vol%% *}.  Free space there by hand, or point the toolchain elsewhere:" >&2
        echo "    TMPDIR=/d/tmp ./scripts/boot-test.sh" >&2
        echo "" >&2
        echo "To override for one run:  --min-free-temp-gb=N   (or" >&2
        echo "BOOT_TEST_MIN_FREE_TEMP_GB=N, 0 disables just this check)." >&2
        exit 1
    fi
    echo "Toolchain temp OK: ${tmp_gb} GiB on ${tmp_vol%% *} ($tmpdir, floor ${MIN_FREE_TEMP_GB} GiB, ${phase})."
}

# The lowest free-space reading this run has seen, and the phase that produced
# it, for record_boot_outcome.  Empty until the first successful measurement.
#
# The minimum rather than the last reading: the question a reader asks of this
# file months later is "was the host short of disk when this cluster of boots
# went red", and the worst moment of a run is not usually its final one -- a
# build consumes tens of GiB and then the linker gives most of it back.
BT_FREE_GB_MIN=""
BT_FREE_GB_PHASE=""

# Record a free-space reading if it is the lowest so far.
#
# Every measurement goes through here rather than only the successful ones, so
# that a run which *refused to start* still records why: that row's whole value
# is the number that caused the refusal.
note_free_gb() {
    local gb="$1" phase="$2"
    # Guard against a non-numeric reading rather than trusting the caller.
    # `measure_free_gb` returns non-zero when df gives nothing usable, and
    # every caller checks -- but an arithmetic comparison on a stray word is a
    # `set -e` abort under `[ ]`, which would turn a diagnostic into a failed
    # boot test.
    case "$gb" in
        ''|*[!0-9]*) return 0 ;;
    esac
    if [ -z "$BT_FREE_GB_MIN" ] || [ "$gb" -lt "$BT_FREE_GB_MIN" ]; then
        BT_FREE_GB_MIN="$gb"
        BT_FREE_GB_PHASE="$phase"
    fi
}

check_tree_free_space() {
    local phase="$1"
    [ "$MIN_FREE_GB" = "0" ] && return 0

    # Name the volume rather than saying "the build volume".  Lane B's report
    # turned on a run that printed "47 GiB on the build volume" while a
    # *different* volume was at zero, so an unnamed number is exactly the part
    # that misleads.  Falls back to the old wording if df cannot name it.
    local vol_label="the build volume"
    local root_vol
    if root_vol="$(volume_id_for "$PROJECT_ROOT")"; then
        vol_label="${root_vol%% *}"
    fi

    local avail_gb
    if ! avail_gb="$(measure_free_gb)"; then
        echo "WARNING: could not measure free space on $PROJECT_ROOT " \
             "(df gave no usable number); the ${MIN_FREE_GB} GiB " \
             "floor is NOT being enforced for this run." >&2
        return 0
    fi

    note_free_gb "$avail_gb" "$phase"

    if [ "$avail_gb" -lt "$MIN_FREE_GB" ]; then
        # --reclaim-space: try the remedy before refusing.
        #
        # reclaim-space.py is safe to invoke unattended because it does not
        # guess whether a directory is in use: it *renames* each candidate
        # first, and Windows refuses to rename a directory that has any file
        # open inside it, so a successful rename is proof rather than a
        # timestamp heuristic.  At its defaults it can only cost the
        # integration checkout and this worktree.
        if [ "$RECLAIM_SPACE" = "1" ]; then
            echo "Only ${avail_gb} GiB free (floor ${MIN_FREE_GB} GiB, ${phase});" \
                 "--reclaim-space given, running scripts/reclaim-space.py." >&2
            local py=""
            if command -v python &>/dev/null; then py=python
            elif command -v python3 &>/dev/null; then py=python3
            fi
            if [ -z "$py" ]; then
                echo "WARNING: --reclaim-space given but no python interpreter" \
                     "was found; cannot run the remedy." >&2
            else
                # Ask for headroom above the floor rather than the floor
                # exactly.  The build this is clearing the way for is itself
                # what consumes the margin, so stopping at the floor would
                # simply trip the second (pre-staging) check minutes later.
                #
                # The exit status is deliberately not tested: a run that could
                # not reach floor+10 may still have freed enough to clear the
                # floor, and the re-measurement below is the authority on that.
                "$py" "$SCRIPT_DIR/reclaim-space.py" \
                    --need "$((MIN_FREE_GB + 10))" --yes || true
                if avail_gb="$(measure_free_gb)" && [ "$avail_gb" -ge "$MIN_FREE_GB" ]; then
                    echo "Free space OK after reclaim: ${avail_gb} GiB" \
                         "(floor ${MIN_FREE_GB} GiB, ${phase})."
                    # Not noted: the pre-reclaim reading above is already the
                    # minimum, and it is the honest one. Recording the
                    # post-reclaim figure as this run's low-water mark would
                    # hide the very pressure that forced a reclaim.
                    return 0
                fi
                echo "Reclaim ran but free space is still below the floor." >&2
            fi
        fi
        echo "" >&2
        echo "ERROR: only ${avail_gb} GiB free on ${vol_label}, which holds the tree; the floor is ${MIN_FREE_GB} GiB (${phase})." >&2
        echo "" >&2
        echo "Refusing to continue rather than risk a disk-full build.  On 2026-08-15 this" >&2
        echo "volume hit zero bytes free and a half-written edit truncated a kernel source" >&2
        echo "file to zero bytes; a part-way link can also leave a stale kernel staged in the" >&2
        echo "ESP, which a later --no-build run boots as if it were current." >&2
        echo "" >&2
        echo "To free space, use the tool built for it:" >&2
        echo "    python scripts/reclaim-space.py --need $((MIN_FREE_GB + 10)) --yes" >&2
        echo "or re-run this script with --reclaim-space to do that and retry automatically." >&2
        echo "" >&2
        echo "Prefer it over a bare 'cargo clean' in another worktree.  It renames each" >&2
        echo "directory before deleting it, and Windows refuses to rename a directory with" >&2
        echo "any file open inside, so it can tell 'idle' from 'in use' as a fact rather" >&2
        echo "than by a timestamp -- a target/ that has been untouched for minutes may" >&2
        echo "still belong to a lane sitting in a QEMU boot phase.  Run without --yes first" >&2
        echo "to see what it would remove.  target/ is entirely regenerable, so the worst" >&2
        echo "case is a rebuild and never lost source." >&2
        echo "" >&2
        echo "To override for one run:  --min-free-gb=N   (or BOOT_TEST_MIN_FREE_GB=N, 0 disables)" >&2
        exit 1
    fi
    echo "Free space OK: ${avail_gb} GiB on ${vol_label} (floor ${MIN_FREE_GB} GiB, ${phase})."
}

# Both volumes a build can run out of room on.
#
# A wrapper rather than a call at the tail of check_tree_free_space, because
# that body returns early on three separate paths -- floor disabled, df
# unreadable, floor cleared by a reclaim -- and a check that quietly does not
# run on two of them is worse than no check at all: it reads as coverage.
check_free_space() {
    local phase="$1"
    check_tree_free_space "$phase"
    check_temp_free_space "$phase"
}

# --- Commit charge: the other resource a boot runs out of ---------------------
#
# The disk checks above guard the wrong resource for the failure that actually
# keeps happening.  Windows' *commit limit* is RAM plus pagefile, and when it is
# reached the kernel refuses to back new private pages -- so `fork()` fails and
# nothing new can start, while free *physical* memory still reads healthy.  With
# three lanes compiling at once this machine sits at 96-97% of a ~262 GB limit,
# and 20 GB of free RAM alongside it is the normal shape, not a contradiction.
# Watching free RAM does not predict it; only the commit number does.
#
# It has cost two boot runs.  On 2026-09-02 one died at 395s with
# `dofork: child -1 ... 0xC000012D` (STATUS_COMMIT_LIMIT) -- after the build,
# deep into the self-tests, with the whole run discarded.  See known-issues.md
# "Three lanes building at once exhausts the Windows commit limit".
#
# WAITING RATHER THAN REFUSING.  Unlike a full disk, this condition clears by
# itself: the builds causing it finish.  Refusing on sight would stop an
# autonomous lane for something that resolves in minutes, so this mirrors the
# boot lock -- wait for a bounded budget, then give up with a status that says
# nothing was booted.
#
# AN ABSOLUTE FLOOR, NOT A PERCENTAGE.  What a run needs is a fixed quantity --
# a 3 GiB guest, QEMU's own mappings, and room for the dozens of short-lived
# helpers the harness forks while polling -- not a share of the machine.  A
# percentage would demand more on a big host and less on a small one, which is
# backwards.
#
# NEVER BLOCKS ON A FAILED PROBE.  No PowerShell, or an unparseable answer,
# means "unknown", and unknown proceeds.  A gate that fails closed on its own
# measurement error would stop every boot on a non-Windows host, and a boot not
# run is a regression not caught.
# WHERE 12 GiB COMES FROM, AND HOW FIRM IT IS.  Not from adding up what a boot
# needs -- that sum says ~4 GiB (a 3 GiB guest plus QEMU's own mappings) and it
# is demonstrably too low.  It comes from the failure: readings taken minutes
# either side of the 2026-09-02 fork failure showed 8.0 and 9.7 GiB of commit
# still free, so a run died at a headroom that a naive floor would have called
# ample.  Cygwin's fork reserves the parent's whole address space, and another
# lane's rustc can take several GiB in a spike, so the margin has to cover a
# transient neither process reports.
#
# So this is an empirical floor anchored to one observation, deliberately set
# above the highest reading associated with a failure rather than at it.  That
# makes it a lower bound on what is safe, not a measurement of it.  If a boot
# ever dies of STATUS_COMMIT_LIMIT while this gate passed it, the number is
# still too low and the evidence for raising it is that run -- record the
# reading in known-issues.md rather than adjusting by feel.
MIN_COMMIT_FREE_MB="${BOOT_TEST_MIN_COMMIT_FREE_MB:-12288}"
COMMIT_WAIT="${BOOT_TEST_COMMIT_WAIT:-900}"

# Free commit charge in MiB on stdout, or a non-zero status if it cannot be had.
#
# `TotalVirtualMemorySize` and `FreeVirtualMemory` are Win32_OperatingSystem's
# names for the commit limit and the unused part of it, both in KiB.  They are
# the same pair Task Manager shows as "Committed"; there is no `df`-like
# equivalent, which is why this reaches for PowerShell at all.
measure_commit_free_mb() {
    command -v powershell &>/dev/null || return 1
    local out
    out="$(powershell -NoProfile -NonInteractive -Command \
        '[int]((Get-CimInstance Win32_OperatingSystem).FreeVirtualMemory/1KB)' \
        2>/dev/null | tr -cd '0-9')" || return 1
    [ -n "$out" ] || return 1
    echo "$out"
}

# Wait until the host can afford this run, or give up with exit 5.
#
# `phase` is echoed so a run that waits twice says which wait it is in; the
# second one is the interesting one, because it means our own build is what
# consumed the margin.
check_commit_headroom() {
    local phase="$1"
    [ "$MIN_COMMIT_FREE_MB" -gt 0 ] 2>/dev/null || return 0

    local free_mb
    if ! free_mb="$(measure_commit_free_mb)"; then
        # Said once per run and not per poll: on a host without PowerShell this
        # is the normal case, and a warning repeated every 20 seconds trains the
        # reader to skip the block it is printed in.
        if [ "${_COMMIT_PROBE_WARNED:-0}" = 0 ]; then
            _COMMIT_PROBE_WARNED=1
            echo "NOTE: cannot read the host's commit charge; the" \
                 "${MIN_COMMIT_FREE_MB} MiB floor is NOT being enforced."
        fi
        return 0
    fi

    if [ "$free_mb" -ge "$MIN_COMMIT_FREE_MB" ]; then
        echo "Commit headroom OK: ${free_mb} MiB free (floor ${MIN_COMMIT_FREE_MB} MiB, ${phase})."
        return 0
    fi

    echo "=== Waiting for commit headroom (${free_mb} MiB free, need ${MIN_COMMIT_FREE_MB} MiB, ${phase}) ==="
    echo "    Another lane is probably building.  This clears on its own; nothing is wrong with the tree."
    local waited=0
    local nap
    while [ "$waited" -lt "$COMMIT_WAIT" ]; do
        # Never sleep past the budget.  A fixed 20s tick would make
        # BOOT_TEST_COMMIT_WAIT=5 wait twenty seconds, so the knob would not
        # mean what it says -- and it is the knob a test, or an operator in a
        # hurry, would reach for first.
        nap=$((COMMIT_WAIT - waited))
        [ "$nap" -gt 20 ] && nap=20
        sleep "$nap"
        waited=$((waited + nap))
        if ! free_mb="$(measure_commit_free_mb)"; then
            # The probe worked a moment ago and does not now.  Proceeding is the
            # right default for the same reason it is above -- we decline to
            # block on our own inability to measure -- but it is said out loud,
            # because a silent transition from "gated" to "not gated" is the
            # shape of a check that has quietly stopped checking.
            echo "NOTE: the commit-charge probe stopped answering after ${waited}s; proceeding ungated."
            return 0
        fi
        if [ "$free_mb" -ge "$MIN_COMMIT_FREE_MB" ]; then
            echo "Commit headroom OK after ${waited}s: ${free_mb} MiB free (floor ${MIN_COMMIT_FREE_MB} MiB)."
            return 0
        fi
        echo "    still ${free_mb} MiB free after ${waited}s of ${COMMIT_WAIT}s..."
    done

    echo "" >&2
    echo "ERROR: gave up after ${COMMIT_WAIT}s waiting for commit headroom (${free_mb} MiB free, floor ${MIN_COMMIT_FREE_MB} MiB, ${phase})." >&2
    echo "" >&2
    echo "NOTHING WAS BUILT AND NOTHING WAS BOOTED — this says nothing about the code under test." >&2
    echo "" >&2
    echo "Windows' commit limit is RAM plus pagefile.  At the limit, no process can start:" >&2
    echo "fork() returns STATUS_COMMIT_LIMIT (0xC000012D) and the run dies wherever it happens" >&2
    echo "to be, which on 2026-09-02 was 395 seconds into a boot whose build had already cost" >&2
    echo "twenty minutes.  Refusing now costs seconds instead." >&2
    echo "" >&2
    echo "The usual cause is another lane's cargo build.  Do NOT kill it — it is another" >&2
    echo "agent's in-flight work.  Wait for it, or do work that does not need to boot." >&2
    echo "" >&2
    echo "To override for one run:  BOOT_TEST_MIN_COMMIT_FREE_MB=0  (0 disables the floor)" >&2
    echo "To wait longer:           BOOT_TEST_COMMIT_WAIT=<seconds>" >&2
    # Same reasoning as exit 4's cleanup: take the previous run's artefacts with
    # us, so "nothing was booted" is self-evident to a caller that greps the
    # serial log without having heard of this status.
    rm -f "${SERIAL_FILE:-}" "${SERIAL_FILE:+${SERIAL_FILE%.txt}-regs.txt}" 2>/dev/null || true
    exit 5
}

# Validated here rather than passed through, so a typo ("--host-load=quiet")
# fails the run outright instead of being silently recorded as an unknown value
# by bench-history.py.  A mislabelled control is worse than an unlabelled one.
case "$HOST_LOAD" in
    idle|loaded|unknown) ;;
    *)
        echo "ERROR: --host-load must be idle, loaded or unknown (got '$HOST_LOAD')" >&2
        exit 1
        ;;
esac

# --bench waits for a marker that is emitted long after BOOT_OK, so it needs a
# correspondingly longer budget.  Applied only if the caller did not pick a
# timeout themselves — an explicit --timeout= always wins, in either direction.
if [ "$BENCH" = "1" ] && [ "$TIMEOUT_EXPLICIT" = "0" ]; then
    TIMEOUT="$BENCH_TIMEOUT"
fi

# --bench DEFAULTS to --release; every other run defaults to debug.  Either can
# be overridden with --profile=<debug|release>.
#
# WHY: a benchmark that does not measure the shipped build is not a benchmark.
# Until 2026-08-14 this script ran a bare `cargo build` for every mode, so all
# 63 benchmarks were measured at `opt-level = 0` (there is no
# `[profile.dev.package.kernel]` override) and then scored against
# `baselines.toml` targets taken from *optimised* Linux/Fuchsia/L4/jemalloc
# implementations — a comparison with no meaning.  Meanwhile
# `[profile.release.package.kernel]` had been sitting in Cargo.toml the whole
# time with `opt-level = 3, codegen-units = 1, strip = "none"`, tuned for
# exactly this and never used.  See known-issues.md
# B-BENCH-ENTIRE-SUITE-MEASURES-AN-UNOPTIMISED-KERNEL.
#
# It also cuts the largest *noise* source in the suite.  Under TCG a hot loop
# that straddles a 4 KiB guest page costs ~1.7x, deterministically per build,
# and that penalty is invisible to both the canary and the mean/min check
# because it does not vary within a run.  Straddle probability scales with the
# loop's byte length: ~500 bytes at opt-level 0 versus a few dozen optimised.
# See B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x.
#
# The default boot test deliberately stays on debug — faster rebuilds and
# readable panics matter most when a boot *fails*, and --bench already roughly
# doubles the cycle.  Whether that split is right is Q46 in open-questions.md;
# if it is resolved toward "release everywhere", collapse these two branches.
#
# --profile=<debug|release> overrides the choice below.  --bench selects the
# *default* profile; it no longer dictates it.  The combination that override
# unlocks -- `--bench --profile=debug`, i.e. wait for BENCH_OK on a debug build
# -- is the only way to exercise the debug branch of the per-profile budgets in
# bench.rs, which are otherwise dead code no test can reach.
if [ -n "$PROFILE_REQ" ]; then
    BENCH_PROFILE="$PROFILE_REQ"
elif [ "$BENCH" = "1" ]; then
    BENCH_PROFILE="release"
else
    BENCH_PROFILE="debug"
fi

if [ "$BENCH_PROFILE" = "release" ]; then
    CARGO_PROFILE_ARGS=("--release")
    KERNEL_BIN="$PROJECT_ROOT/target/x86_64-unknown-none/release/kernel"
else
    CARGO_PROFILE_ARGS=()
    KERNEL_BIN="$PROJECT_ROOT/target/x86_64-unknown-none/debug/kernel"
fi

# Optional hard-lockup NMI watchdog device (see --hard-lockup-watchdog above and
# Q20).  Empty unless opted in, so the default QEMU command line is unchanged.
#
#  * -device i6300esb        — Intel 6300ESB PCI watchdog, emulated by QEMU under
#                              TCG (unlike the PMU, which TCG does not model), so
#                              it is the one NMI source that works in our harness.
#  * -action watchdog=inject-nmi
#                            — on watchdog expiry, inject an NMI into the guest
#                              (fires even with IF=0) instead of resetting it.
#
# The action is overridable via the WATCHDOG_ACTION env var (default
# inject-nmi) purely as a *diagnostic* affordance: setting it to `reset`
# (combined with the always-present -no-reboot) turns a stage-2 expiry into a
# clean QEMU exit, which discriminates "the i6300esb counter fired but the NMI
# was not delivered/handled" (VM exits during a wedge) from "the counter never
# fired at all" (VM hangs the full timeout).  Normal runs never set it, so the
# harness default is byte-for-byte unchanged.
WATCHDOG_ACTION="${WATCHDOG_ACTION:-inject-nmi}"
WATCHDOG_ARGS=()
# Diagnostic HMP monitor for capturing the wedged guest RIP on timeout.  ON BY
# DEFAULT; --no-monitor opts out.  On timeout we query `info registers`/`info
# cpus` over this socket BEFORE killing QEMU, which captures the frozen CPU's
# RIP directly from the emulator — bypassing in-guest NMI delivery entirely (the
# silent BSP-dead wedge never takes the injected NMI, so the in-guest handler
# dump is blind; the emulator's own view is not).
#
# It used to be attached only alongside --hard-lockup-watchdog, which made both
# RIP-capture paths (the stall capture and the timeout capture) dead code on
# every ordinary run.  That cost a real diagnosis on 2026-08-25: a
# B-FORKEXEC-BOOT-HANG recurrence consumed the full 900 s timeout and produced
# no RIP, and the immediately-following re-run *with* the flag booted green —
# which is the normal outcome for a hang that appears once in a few dozen boots.
# An opt-in detector for an intermittent fault is a detector that is off when
# the fault happens.
#
# Coupling the two was a category error rather than a policy: §61 kept the
# hard-lockup watchdog opt-in so the *guest* is unperturbed, because
# `-device i6300esb` changes the guest's PCI topology.  `-monitor tcp:` adds no
# device and changes nothing the guest can observe — it is a host-side control
# socket — so §61's rationale never applied to it.  Verified empirically across
# the 2026-08-25 pair: the run with the monitor attached and the run without
# produced no difference in harness stdout (no `(qemu)` banner either way,
# since -serial already goes to a file rather than stdio).
MONITOR_ARGS=()

# Pick a TCP port for the HMP monitor that QEMU can actually bind.  On Windows,
# whole ranges are *reserved* (Hyper-V/WSL "excludedportrange") and a bind into
# one fails with "Failed to bind socket: Input/output error" — QEMU then exits
# instantly and every armed boot fails ~2 s in (this silently wasted a full
# wedge-soak run: the old hardcoded 55123 sits inside the reserved 55053-55152
# range).  Choose the first candidate at/above a base port that is neither in
# any excluded range nor currently LISTENing.  Falls back to the base port when
# the query tools are unavailable (non-Windows), so Linux/CI behaviour is
# unchanged.  An explicit MONITOR_PORT env override always wins.
pick_monitor_port() {
    local base="$1" p="" excl="" listen=""
    # Excluded ranges as "start end" pairs (Windows only; empty elsewhere).
    if command -v netsh &>/dev/null; then
        excl="$(netsh interface ipv4 show excludedportrange protocol=tcp 2>/dev/null \
                | grep -E '^[[:space:]]*[0-9]+[[:space:]]+[0-9]+' \
                | awk '{print $1" "$2}')"
    fi
    # Currently-listening local TCP ports (best-effort).
    if command -v netstat &>/dev/null; then
        listen="$(netstat -ano 2>/dev/null | grep -iE 'LISTEN' \
                  | grep -oE ':[0-9]+' | tr -d ':' | sort -u)"
    fi
    for p in $(seq "$base" $((base + 200))); do
        local bad=0 s e
        while read -r s e; do
            [ -z "$s" ] && continue
            if [ "$p" -ge "$s" ] && [ "$p" -le "$e" ]; then bad=1; break; fi
        done <<< "$excl"
        [ "$bad" -eq 1 ] && continue
        if [ -n "$listen" ] && printf '%s\n' "$listen" | grep -qx "$p"; then continue; fi
        echo "$p"; return 0
    done
    echo "$base"  # nothing free found; let QEMU try the base and report
}

# Port selection is inside the enable test on purpose: pick_monitor_port shells
# out to netsh and netstat, which cost a second or two on Windows, and a run
# that will not attach the monitor has no use for the answer.
if [ "$MONITOR_ENABLED" -eq 1 ]; then
    if [ -n "${MONITOR_PORT:-}" ]; then
        MONITOR_PORT_SRC="env override"
    else
        MONITOR_PORT="$(pick_monitor_port 57000)"
        MONITOR_PORT_SRC="auto-selected (excluded-range aware)"
    fi
    MONITOR_ARGS=(-monitor "tcp:127.0.0.1:$MONITOR_PORT,server,nowait")
    echo "=== Diagnostic HMP monitor ENABLED (tcp:127.0.0.1:$MONITOR_PORT, $MONITOR_PORT_SRC) ==="
fi
if [ "$HARD_LOCKUP_WATCHDOG" -eq 1 ]; then
    # SC2054 (use spaces, not commas, between array elements): the comma is
    # QEMU's own property separator inside a single `-device` argument, not a
    # separator between two of ours.  Splitting on it would pass `id=hwdog0` as
    # a standalone argument and QEMU would reject the command line.
    # shellcheck disable=SC2054
    WATCHDOG_ARGS=(-device i6300esb,id=hwdog0 -action "watchdog=$WATCHDOG_ACTION")
    echo "=== Hard-lockup watchdog ENABLED (i6300esb -> $WATCHDOG_ACTION) ==="
    if [ "$MONITOR_ENABLED" -ne 1 ]; then
        # The watchdog's whole value is the RIP the monitor reads back, so
        # --no-monitor alongside it is almost certainly a mistake.  Say so
        # rather than silently arming a detector whose output is discarded.
        echo "=== WARNING: --no-monitor disables the RIP capture the watchdog exists to feed ===" >&2
    fi
fi

# Capture the guest CPU state over the HMP monitor socket, then resolve RIP to a
# kernel symbol.  Best-effort: prints a warning and returns non-zero if the
# monitor is unreachable or the shell lacks /dev/tcp support.
#
# $3 is the label to print the RIP under, and it is a required argument rather
# than a default because the meaning of the sample depends entirely on which
# caller took it.  From the stall detector the guest has demonstrably stopped
# and "Wedged RIP" is a finding; from the plain timeout path the guest may well
# have been running, and the same words would be an unfounded diagnosis.
#
# Args: $1 = monitor TCP port, $2 = output file for the raw register dump,
#       $3 = label to print the RIP under (see above; required).
capture_guest_state() {
    local port="$1" out="$2" rip_label="$3"
    # HMP over a bash /dev/tcp socket.  Fire the read-only queries and let the
    # `timeout` bound the read — we deliberately do NOT send `quit`: quitting
    # provokes a QEMU shutdown that can hang mid-teardown (holding the monitor
    # port and surviving the harness's later `kill`), which then blocks the
    # NEXT boot from binding the port.  A single connection is opened (no
    # pre-check probe, which would consume the single-client monitor slot).
    if ! { exec 9<>"/dev/tcp/127.0.0.1/$port"; } 2>/dev/null; then
        echo "  (monitor unreachable on port $port; cannot capture RIP)"
        return 1
    fi
    printf 'info registers\ninfo cpus\ninfo registers -a\n' >&9
    timeout 5 cat <&9 > "$out" 2>/dev/null || true
    exec 9>&- 2>/dev/null || true
    if [ ! -s "$out" ]; then
        echo "  (monitor produced no output; cannot capture RIP)"
        return 1
    fi
    echo "=== Guest register dump captured to: $out ==="
    # Extract RIP from the HMP `info registers` output (line contains "RIP=...").
    local rip
    rip="$(grep -oiE 'RIP=[0-9a-f]+' "$out" | head -n1 | cut -d= -f2 || true)"
    if [ -n "$rip" ]; then
        echo "  $rip_label = 0x$rip"
        resolve_kernel_symbol "$rip"
    else
        echo "  (no RIP= line in monitor output; see $out)"
    fi
    return 0
}

# Resolve a hex address to the kernel symbol that *contains* it.
#
# This delegates to `scripts/symbolize.py`.  It used to be a second, independent
# symbolizer written in awk right here, and that is worth recording because it
# was wrong in a way that mattered more here than anywhere else in the tree.
#
# The awk version picked "the last defined symbol with addr <= RIP" and its own
# comment asserted "that is the function the RIP lies within".  It is not.  A
# symbol's extent is `st_size`, and nearest-preceding says nothing about it: an
# address in the alignment padding after a 16-byte array resolved to that array
# plus a 15 KiB offset -- confidently, with nothing to indicate a miss.  It also
# ignored symbol *kind*, so a data object could out-rank a function.
#
# The single caller is the hang-capture path above: a boot that wedged, one RIP
# from the QEMU monitor, and nothing else to go on.  That is the worst place in
# the tree to print a plausible wrong function name, because there is no second
# signal to contradict it -- the reader goes and reads the wrong subsystem.
#
# Two symbolizers was also the same shape as the capability bug fixed in
# c58efa00d: two implementations of one operation, an invariant that they agree,
# and nothing checking it.  `symbolize.py` has since been fixed (sized extents,
# kind-aware search, `--self-test`); keeping a duplicate in awk would have left
# that fix unapplied at exactly the site that needs it most.  So there is now
# one symbolizer.
#
# If Python is missing we print the raw address and the command to run by hand.
# A fallback that is known to be wrong is worse than no fallback: it looks like
# an answer.
resolve_kernel_symbol() {
    local rip="$1"
    if [ ! -f "$KERNEL_BIN" ]; then
        echo "  (kernel ELF missing; resolve 0x$rip manually)"
        return 1
    fi
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "  (no python; resolve 0x$rip by hand:)"
        echo "     python scripts/symbolize.py --elf \"$KERNEL_BIN\" 0x$rip"
        return 1
    fi
    # `--elf`, not `--profile`: $KERNEL_BIN already tracks --release, and the
    # whole point is to symbolize against the binary that actually booted.
    "$py" "$SCRIPT_DIR/symbolize.py" --elf "$KERNEL_BIN" "0x$rip" 2>/dev/null |
        sed 's/^/  Symbol: /'

    # Kept from the awk version this replaced: a RIP outside the higher half was
    # never going to be in the kernel ELF, and saying so turns a bare `??` into
    # a direction to look.  `symbolize.py` cannot say it -- it does not know
    # this project's address-space split -- so the hint stays here.
    case "$(printf '%s' "$rip" | tr 'A-F' 'a-f')" in
        ffff*) ;;
        *) echo "  (0x$rip is not a higher-half address -- likely a ring-3 RIP," \
                "so the kernel ELF is the wrong binary to resolve it against)" ;;
    esac
    return 0
}

# Print the micro-benchmark result lines from the serial log.  The kernel emits
# them as "[bench] <name>: <number>" plus PASS / "OVER HARDWARE TARGET" lines
# from a background task that runs AFTER BOOT_OK.
#
# An over-target verdict is NOT a failure here and is not reported as one.
# Under QEMU's TCG interpreter every guest memory access carries a softmmu
# lookup costing a few hundred host cycles, where real hardware takes an L1 hit
# at 1-4 cycles, so the bare-metal targets in bench/baselines.toml are
# unreachable by construction and most of the suite sits 10-400x over them
# while being perfectly correct.  Five boots were once spent chasing exactly
# that illusion (known-issues.md,
# TD-BENCH-OWNER-AB-BUDGET-WAS-AN-ABSOLUTE-CYCLE-COUNT).
#
# The comparison that does carry signal is run-over-run on this same host,
# which cancels the emulation constant.  scripts/bench-history.py records each
# run to bench/history.jsonl and diffs against the previous one.  This used to
# say "compare against prior runs" without anything storing them, which made
# the advice unfollowable.

# Non-zero if the benchmark recorder failed on this run.  Read by finish_pass,
# which is the only place allowed to print the PASSED banner.
BENCH_RECORDER_STATUS=0

print_bench_results() {
    local file="$1"
    [ -f "$file" ] || return 0
    echo "=== Benchmark results ==="
    # The machine-readable SCORE and CANARY lines are for bench-history.py,
    # not the reader; the kernel prints a prose verdict for each alongside
    # them, and that is what stays here. Note this only filters the *display*
    # -- bench-history.py re-reads the raw file, so nothing is lost.
    grep -E '^\[bench\]' "$file" \
        | grep -v -E '^\[bench\] (SCORE|CANARY) ' \
        || echo "(no [bench] lines found)"

    # Record and diff.
    #
    # --profile stamps the record with the build profile it was measured on.
    # Numbers from different profiles are not comparable — opt-level 0 vs 3 on
    # this code is a multiple, not a percentage — so the comparator must never
    # diff across the boundary.  The 5 records written before 2026-08-14 carry
    # no profile field and are read as "debug".
    #
    # The exit status is CAPTURED, not discarded.  It used to end in `|| true`
    # on the reasoning that "a missing python or a write failure must not turn a
    # healthy boot into a failed one" — which is true, and which those two cases
    # already satisfy without it: python's absence is handled by the `command
    # -v` branch below, and a write failure is reported by the tool without a
    # non-zero exit.  What `|| true` actually suppressed was the third case
    # nobody had in mind: the recorder *crashing*.  A refactor left a `NameError`
    # on the recording path, and for four commits every `--bench` boot printed a
    # traceback, wrote no history record, and was immediately overprinted with
    # "=== Boot test PASSED ===" — the run silently produced no data at all.
    #
    # Note this invocation passes no --fail-on-regression, so the tool has no
    # legitimate non-zero exit here: any non-zero status is a fault in the tool
    # itself.  See the docstring on bench-history.py's print_canary_summary.
    #
    # --wall-seconds and --host-load feed the run-level verdict, which is the
    # worst of the canary, dispersion and wall-clock axes rather than the canary
    # alone.  The wall figure is omitted entirely (not passed as 0) when the
    # QEMU window was never timed, because bench-history.py's whole discipline
    # is that an absent measurement is unknown, not clean.
    #
    # --commit/--dirty carry the state captured before the build rather than
    # whatever HEAD is now; see the BT_HEAD block near the top of this file for
    # why re-deriving it at exit is wrong, and wrong in the hiding direction.
    local bench_args=(--serial "$file" --profile "$BENCH_PROFILE"
                      --host-load "$HOST_LOAD" --commit "${BT_HEAD:-unknown}")
    # Omitted entirely rather than passed empty when it could not be computed:
    # an absent field is unknown, and unknown must not group.
    if [ -n "${BT_SRC_DIGEST:-}" ]; then
        bench_args+=(--src-digest "$BT_SRC_DIGEST")
    fi
    # The ELF that was actually measured, so the record carries the addresses of
    # the placement-sensitive functions alongside their timings.
    if [ -f "${KERNEL_BIN:-}" ]; then
        bench_args+=(--kernel-elf "$KERNEL_BIN")
    fi
    if [ "${BT_DIRTY:-0}" = 1 ]; then
        bench_args+=(--dirty)
    fi
    # A probe run is recorded but never becomes a baseline. BENCH_EXPERIMENT
    # states the reason; QEMU_EXTRA implies one even when the caller forgot,
    # because a run under non-default emulator flags is no more reproducible
    # from a checkout than one under a hand-patched kernel -- and it was exactly
    # such a run (a tb-size probe) that landed unlabelled in the history and
    # motivated all of this.
    if [ -n "${BENCH_EXPERIMENT:-}" ]; then
        bench_args+=(--experiment "$BENCH_EXPERIMENT")
    elif [ -n "${QEMU_EXTRA:-}" ]; then
        bench_args+=(--experiment "QEMU_EXTRA=$QEMU_EXTRA (non-default emulator flags)")
    fi
    if [ -n "${QEMU_START_EPOCH:-}" ]; then
        local wall=$(( ${QEMU_END_EPOCH:-$(date +%s)} - QEMU_START_EPOCH ))
        # Spelt as a full `if` rather than `[ ... ] && ...`: under `set -e` a
        # bare AND-list whose test fails takes the script's exit status with it,
        # so a clock that stepped backwards mid-run would abort the harness
        # instead of merely declining to record a nonsense duration.
        if [ "$wall" -ge 0 ]; then
            bench_args+=(--wall-seconds "$wall")
        fi
    fi

    local rc=0
    if command -v python &>/dev/null; then
        python "$SCRIPT_DIR/bench-history.py" "${bench_args[@]}" || rc=$?
    elif command -v python3 &>/dev/null; then
        python3 "$SCRIPT_DIR/bench-history.py" "${bench_args[@]}" || rc=$?
    else
        echo "(python not found; skipping benchmark history diff)"
        return 0
    fi

    if [ "$rc" -ne 0 ]; then
        echo "=== BENCHMARK RECORDER FAILED (exit $rc) ==="
        echo "    The kernel's numbers are in $file, but they were NOT recorded"
        echo "    to bench/history.jsonl, so this run cannot be compared against"
        echo "    later ones.  This is a bug in scripts/bench-history.py, not in"
        echo "    the kernel: the boot itself is unaffected."
        BENCH_RECORDER_STATUS=$rc
    fi
}

# Record this run's outcome to bench/boot-history.jsonl.
#
# Called from ONE place -- the EXIT trap -- and deliberately not from the ~12
# sites that call `exit`.  A recorder wired into each exit site is wrong the
# first time someone adds a thirteenth, and wrong in the direction that hurts:
# the site nobody remembered to wire up is a *failure* site, so the omission
# reads downstream as a clean streak.  boot-history.py therefore derives the
# verdict from the pair (exit code, serial log) rather than being told it.
#
# Why record at all: four open kernel issues (W1, B-FORKEXEC-BOOT-HANG,
# B-PTHREAD-TEARDOWN-PF, W-KERNEL-COW-WRITE) have closure conditions that are
# counts of consecutive clean boots, and nothing counted them -- W1's status
# line has read "clean streak 7" since 2026-06-14 while many dozens of boots
# have passed.  This is the boot-outcome twin of bench-history.py, which stores
# numbers this file does not see (it only gains a record on a --bench run that
# reached its marker, so it is structurally blind to hangs).
#
# It must never change our exit status: a broken recorder turning a green boot
# red is strictly worse than no recorder.  Hence `|| true` and no use of $?
# after the call.
record_boot_outcome() {
    local rc="$1"
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        return 0
    fi

    # --commit/--branch/--dirty carry the state captured before the build, not
    # whatever HEAD happens to be now; see the BT_HEAD block near the top.
    local args=(--serial "$SERIAL_FILE" --exit-code "$rc"
                --marker "$WAIT_MARKER" --profile "${BENCH_PROFILE:-debug}"
                --commit "${BT_HEAD:-unknown}" --branch "${BT_BRANCH:-unknown}")
    # Passed only when the file exists, so a --no-boot run (or one that died
    # before QEMU launched) hands the recorder nothing rather than a leftover
    # from the previous boot -- the same rule as --gated-markers above, and for
    # the same reason: a stale host-failure signature would excuse a boot that
    # the host had nothing to do with, which is the one direction this feature
    # must not fail in.
    if [ -f "$QEMU_STDERR" ]; then
        args+=(--qemu-stderr "$QEMU_STDERR")
    fi
    if [ -n "${BT_SRC_DIGEST:-}" ]; then
        args+=(--src-digest "$BT_SRC_DIGEST")
    fi
    # Passed only when *this* run's pre-build gate produced the file.  A run
    # with no python, or one that skipped the gates, would otherwise hand the
    # recorder a leftover from some earlier run and describe a main.rs that is
    # not the one that booted.
    #
    # The test is a flag set by the gate, not a timestamp comparison, because
    # there is no timestamp ordering that distinguishes the two cases: the
    # markers are emitted before the build and the serial log is written after
    # it, so a fresh markers file is *older* than the serial log -- exactly like
    # a stale one.  Whether the gate ran is a fact this script knows directly;
    # inferring it from mtimes would be guessing at something already known.
    if [ "${GATED_MARKERS_FRESH:-0}" = 1 ]; then
        args+=(--gated-markers "$GATED_MARKERS")
    elif [ -f "$GATED_MARKERS" ]; then
        echo "[boot-history] this run did not regenerate gated-markers.json --"
        echo "               omitting gated_ran rather than recording an older"
        echo "               main.rs's markers against this boot."
    fi
    if [ "${BT_DIRTY:-0}" = 1 ]; then
        args+=(--dirty)
    fi
    if [ -n "${BOOT_LABEL:-}" ]; then
        args+=(--label "$BOOT_LABEL")
    fi
    # Mirrors the benchmark recorder's experiment rule, and must keep mirroring
    # it. A run that no checkout reproduces is not evidence about the tree, and
    # that is as true of its boot outcome as of its timings -- but only the
    # benchmark half used to say so. The gap let a deliberate `-cpu host` probe,
    # which died in OVMF before our kernel was even loaded, land as a plain
    # TIMEOUT and reset the consecutive-clean streak that four open kernel
    # issues use as their closure bar.
    #
    # Every reason is collected into one string rather than the first one
    # winning, because a probe is often several at once (foreign accelerator
    # *and* foreign CPU model), and the reader of a row a month from now needs
    # all of them to know what was actually run.
    local why=""
    if [ -n "${BENCH_EXPERIMENT:-}" ]; then
        why="$BENCH_EXPERIMENT"
    fi
    if [ -n "${QEMU_EXTRA:-}" ]; then
        why="${why:+$why; }QEMU_EXTRA=$QEMU_EXTRA (non-default emulator flags)"
    fi
    if [ "${QEMU_CPU_OVERRIDDEN:-0}" = 1 ]; then
        why="${why:+$why; }QEMU_CPU=$QEMU_CPU (non-default guest CPU)"
    fi
    # A non-default GPU also changes the display backend (egl-headless instead
    # of none), so such a run is not comparable on wall time with a tracking
    # run even when it passes -- which is precisely what the experiment tag is
    # for.
    if [ "${GPU_OVERRIDDEN:-0}" = 1 ]; then
        why="${why:+$why; }SLATE_GPU=$GPU_DEVICE (non-default display device)"
    fi
    # A --no-rootfs run tests strictly less than a tracking run and therefore
    # reads greener: the Path-Z rungs that read /mnt no-op instead of failing.
    # Tagging it keeps a deliberately narrower boot out of the consecutive-clean
    # streak, which four open kernel issues use as their closure bar -- a streak
    # extended by removing the failing tests would certify nothing.
    if [ "${NO_ROOTFS:-0}" = 1 ]; then
        why="${why:+$why; }--no-rootfs (rootfs.ext4 not attached; /mnt rungs no-op)"
    fi
    if [ -n "$why" ]; then
        args+=(--experiment "$why")
    fi
    # Absent, not zero, when the run did not build: --no-build and --no-stage
    # skip Step 1 entirely, and recording 0 there would drag every median down
    # while looking like an implausibly fast build rather than like a run that
    # never built.  A missing field is a question the reader can answer; a wrong
    # one is not.
    if [ -n "${BUILD_SECONDS:-}" ]; then
        args+=(--build-seconds "$BUILD_SECONDS")
    fi
    # Absent when nothing was measured -- --min-free-gb=0 disables the check
    # entirely, and an unreadable df returns early -- because a run that did not
    # look is not a run that saw zero GiB free.
    if [ -n "${BT_FREE_GB_MIN:-}" ]; then
        args+=(--free-gb-min "$BT_FREE_GB_MIN")
        if [ -n "${BT_FREE_GB_PHASE:-}" ]; then
            args+=(--free-gb-phase "$BT_FREE_GB_PHASE")
        fi
    fi
    if [ -n "${QEMU_START_EPOCH:-}" ]; then
        local wall=$(( ${QEMU_END_EPOCH:-$(date +%s)} - QEMU_START_EPOCH ))
        if [ "$wall" -ge 0 ]; then
            args+=(--wall-seconds "$wall")
        fi
    fi

    "$py" "$SCRIPT_DIR/boot-history.py" "${args[@]}" || true
}

# Everything that must happen on the way out, once, however we leave.
#
# `reason` distinguishes a normal exit from a signal.  Ctrl-C is not a boot
# outcome -- recording it would enter an operator's interruption into a series
# that exists to measure the kernel, and would reset every hang streak every
# time someone stopped a run early.  The once-guard matters because bash runs
# the INT/TERM handler *and then* the EXIT handler, so without it every
# interrupted run would clean up twice.
_BOOT_EXIT_DONE=0
on_boot_exit() {
    local rc="$1"
    local reason="$2"
    if [ "$_BOOT_EXIT_DONE" -eq 1 ]; then
        return 0
    fi
    _BOOT_EXIT_DONE=1
    kill_qemu "$QEMU_PID"
    echo_qemu_stderr
    if [ "$reason" = "exit" ]; then
        record_boot_outcome "$rc"
    fi
    release_boot_lock
}

# The single place that decides a boot passed, and the only place that prints
# the PASSED banner.
#
# It is a function because there are two ways to reach a successful boot -- the
# poll loop notices the marker, or the post-loop check finds it after QEMU has
# already exited -- and both used to carry their own verbatim copy of this
# sequence.  Two copies of "what counts as a pass" is one copy too many: any
# condition added to one silently does not apply to the other, and the failure
# mode is a boot that reports PASSED down whichever path the copy was not
# applied to.
#
# Exit codes: 0 pass; 3 the kernel booted but the run did not produce the
# artefact it was asked for.  3 is deliberately distinct from 1 (kernel/self-test
# failure) and 2 (hang/wedge) -- conflating "the kernel is broken" with "our
# tooling is broken" sends the reader to the wrong tree.
finish_pass() {
    local file="$1"
    if [ "$BENCH" -eq 1 ]; then
        print_bench_results "$file"
    else
        report_bench_absence "$file"
    fi
    # Both branches, and after print_bench_results so a --bench run checks the
    # map against the row it has just written rather than the previous one.
    # The map is what decides whether a changed file gets called covered, so a
    # rule that has quietly stopped being true is the same false assurance the
    # map was added to remove -- it must be checked on every green boot, not
    # only on the ones that skip the suite.
    report_bench_coverage_rot
    report_pathz_skips "$file"

    # A green run whose rootfs could not be verified is still green -- the
    # kernel booted -- but its Path-Z coverage claim is unbacked.  Re-print the
    # reason here rather than only at attach time: that warning is several
    # hundred lines of serial log above the PASSED banner, and the banner is
    # what gets read.
    if [ -n "$ROOTFS_UNVERIFIED" ]; then
        echo "=== ROOTFS UNVERIFIED ($ROOTFS_UNVERIFIED) ==="
        echo "  Path-Z rungs in this run may have exercised stale binaries."
        echo "  Their PASS is not evidence about the current tree."
    fi

    if [ "$BENCH_RECORDER_STATUS" -ne 0 ]; then
        echo "=== Boot test INCOMPLETE ($WAIT_MARKER reached, but --bench recorded nothing) ==="
        exit 3
    fi

    prune_build_cache_if_low

    echo "=== Boot test PASSED ==="
    exit 0
}

# Drop build-cache units no build has needed for a fortnight, if the volume is
# getting full.  Rationale for the placement and the threshold: see
# PRUNE_CACHE_BELOW_GB near the top of this file.
#
# Deliberately cannot change the verdict.  This runs after every gate has
# already passed, its status is ignored, and the PASSED banner is printed
# afterwards regardless.  A green build that housekeeping then failed to tidy
# is still a green build, and a run that reported FAILED because a disk
# cleanup hit a locked file would be actively misleading -- the failure would
# read as the kernel's.
prune_build_cache_if_low() {
    [ "$PRUNE_CACHE_BELOW_GB" = "0" ] && return 0
    [ -f "$SCRIPT_DIR/prune-build-cache.py" ] || return 0

    local avail_gb
    avail_gb="$(measure_free_gb)" || return 0
    [ "$avail_gb" -ge "$PRUNE_CACHE_BELOW_GB" ] && return 0

    local py=""
    if command -v python &>/dev/null; then py=python
    elif command -v python3 &>/dev/null; then py=python3
    else return 0
    fi

    echo "=== ${avail_gb} GiB free (below ${PRUNE_CACHE_BELOW_GB}); pruning this tree's cold build cache ==="
    echo "    Only units cargo has not invoked in 14 days, and only under $PROJECT_ROOT/target."
    echo "    Nothing a build has asked for is touched; --no-prune-cache skips this."
    # --target-dir is passed explicitly rather than left to the script's
    # default.  The default is "the target/ beside the script", and this script
    # is shared by four worktrees -- so in a checkout whose scripts/ came from
    # somewhere else the default would name the wrong tree.  A run must only
    # ever prune the tree it just built.
    "$py" "$SCRIPT_DIR/prune-build-cache.py" \
        --target-dir "$PROJECT_ROOT/target" --yes || true
    if avail_gb="$(measure_free_gb)"; then
        echo "=== ${avail_gb} GiB free after the prune ==="
    fi
}

# Find QEMU
QEMU=""
for candidate in \
    "qemu-system-x86_64" \
    "/c/Program Files/qemu/qemu-system-x86_64.exe" \
    "C:/Program Files/qemu/qemu-system-x86_64.exe"; do
    if command -v "$candidate" &>/dev/null || [ -f "$candidate" ]; then
        QEMU="$candidate"
        break
    fi
done

if [ -z "$QEMU" ]; then
    echo "ERROR: qemu-system-x86_64 not found" >&2
    exit 1
fi

# Find OVMF firmware
OVMF=""
for candidate in \
    "/c/Program Files/qemu/share/edk2-x86_64-code.fd" \
    "C:/Program Files/qemu/share/edk2-x86_64-code.fd" \
    "/usr/share/OVMF/OVMF_CODE.fd" \
    "/usr/share/edk2/ovmf/OVMF_CODE.fd"; do
    if [ -f "$candidate" ]; then
        OVMF="$candidate"
        break
    fi
done

if [ -z "$OVMF" ]; then
    echo "ERROR: OVMF/EDK2 firmware not found" >&2
    exit 1
fi

# Git-ignored prerequisites (known-issues.md
# A-A-FRESH-CHECKOUT-CANNOT-BOOT-TEST-AND-NEITHER-FAILURE-NAMES-THE-MISSING-STEP).
#
# Two classes of artifact are required and absent from any fresh checkout: the
# six ring-3 service binaries the kernel pulls in with `include_bytes!`, and the
# `limine/` bootloader tree staged into the ESP.  Both used to be discovered the
# hard way — the first as fifteen `couldn't read .../release/netstack` errors
# from a compiler that is not what is missing, and the second as a bare
# `cp: cannot stat 'limine/BOOTX64.EFI'` raised *after* a full workspace build
# had already been spent.  Neither message named the step to run, so the
# knowledge lived only in the shell history of whoever set the tree up first.
#
# Checked here, before Step 1, because that is the last point at which the
# limine failure is free.  The list is not restated: bootstrap-worktree.sh
# derives it from the kernel's own `include_bytes!` paths, so a service added
# to the kernel is covered here without anyone remembering to add it.
check_prerequisites() {
    local boot="$SCRIPT_DIR/bootstrap-worktree.sh"
    if [ ! -x "$boot" ] && [ ! -f "$boot" ]; then
        echo "WARNING: $boot not found; build/boot prerequisites are NOT being" \
             "checked for this run." >&2
        return 0
    fi

    # Ask only about what this particular run needs.  A --no-build --no-stage
    # soak boots the image already in the ESP: it compiles nothing and copies
    # nothing, so neither the embedded service binaries nor limine/ can affect
    # it, and refusing such a run for their absence would be refusing a run that
    # would have worked.  rootfs.ext4 is in scope for every boot that attaches
    # it — its absence silently shrinks the suite rather than blocking it — but
    # not for a --no-rootfs run, which has already decided not to attach it.
    # Asking about it there would report a missing prerequisite for a run that
    # does not have one, and print "this run tests LESS than a normal one" as
    # though the tree were at fault rather than the flag.
    local need=""
    [ "$NO_ROOTFS" -eq 0 ] && need="rootfs"
    [ "$NO_BUILD" -eq 0 ] && need="services${need:+,$need}"
    [ "$NO_STAGE" -eq 0 ] && need="limine${need:+,$need}"
    if [ -z "$need" ]; then
        echo "Prerequisites: nothing this run depends on (--no-build --no-stage --no-rootfs)."
        return 0
    fi

    local report status
    report="$(bash "$boot" --check --need="$need" 2>&1)" && status=0 || status=$?

    case "$status" in
        0) echo "Prerequisites OK ($need)." ;;
        3)
            # Degrading, not blocking: the boot test runs and passes, having
            # quietly skipped every REAL-glibc rung.  Refusing would be wrong —
            # the run is still useful — but saying nothing would let a green
            # result stand for more than it measured.
            echo "" >&2
            echo "$report" >&2
            echo "" >&2
            echo "WARNING: continuing, but this run tests LESS than a normal one." >&2
            echo "" >&2
            ;;
        1)
            if [ "$BOOTSTRAP" = "1" ]; then
                echo "Prerequisites missing; --bootstrap given, provisioning:" >&2
                echo "$report" >&2
                # Not silenced and not backgrounded: this builds several crates
                # and may clone a bootloader, and a caller who asked for it
                # should see it happen.
                #
                # Its exit status is deliberately not tested, for the same
                # reason check_tree_free_space ignores reclaim-space.py's: a run that
                # could not provision *everything* may well have provisioned
                # everything that blocks a build.  bootstrap-worktree.sh exits
                # non-zero when only `rootfs.ext4` is missing, and that alone
                # must not turn a now-buildable tree back into a refusal.  The
                # re-check below is the authority.
                bash "$boot" || true
                report="$(bash "$boot" --check --need="$need" 2>&1)" && status=0 || status=$?
                case "$status" in
                    0)
                        echo "Prerequisites provisioned."
                        return 0
                        ;;
                    3)
                        echo "Prerequisites provisioned, except rootfs.ext4:" >&2
                        echo "$report" >&2
                        echo "" >&2
                        echo "WARNING: continuing, but this run tests LESS than a normal one." >&2
                        echo "" >&2
                        return 0
                        ;;
                esac
                echo "ERROR: --bootstrap ran but prerequisites are still missing." >&2
            fi
            echo "" >&2
            echo "$report" >&2
            echo "" >&2
            echo "ERROR: this checkout cannot build or stage a boot image yet." >&2
            echo "" >&2
            echo "Refusing here rather than at the point of failure.  A missing service" >&2
            echo "binary surfaces as fifteen include_bytes! errors blaming the kernel, and" >&2
            echo "a missing limine/ surfaces as a 'cp: cannot stat' AFTER a full workspace" >&2
            echo "build has been spent — neither of which names the step above." >&2
            echo "" >&2
            echo "Run the command above, or re-run this script with --bootstrap to do" >&2
            echo "that and continue in one go." >&2
            exit 1
            ;;
        *)
            # Includes 2 (the embed list could not be derived).  Do not continue
            # on an unclassified answer: the one thing this check must never do
            # is let "I could not tell" pass for "nothing is missing".
            echo "" >&2
            echo "$report" >&2
            echo "" >&2
            echo "ERROR: prerequisite check exited $status (unexpected); refusing." >&2
            exit 1
            ;;
    esac
}

check_prerequisites

# A landed request is stamped, not deleted (roadmap.md rule 2, §315).
#
# This runs FIRST, ahead of every other gate, because it is the only one here
# whose subject is information that is already gone rather than code that is
# merely wrong.  Every other failure below is a file you can read and fix; this
# one is a file you cannot read, and the window in which restoring it is free
# closes when the deletion reaches `main`.
#
# It is a gate rather than a sixth reminder because the convention was enforced
# by attention and attention lost four times, in every lane, over two weeks --
# `d30e2a5ca`, `57d21b4ee`, `cd23f2f97`, `dd4e34fd9`, all after rule 2 changed
# in `236dc2206`.  The last of those is the one that settles the argument: its
# own commit message asserted the *opposite* rule, so the author was not
# ignoring the convention but misremembering it while explaining it, and no
# restatement fixes that.  The symptom arrived three minutes later, when the
# next commit had to repoint two live citations at something that still existed.
#
# `scripts/open-requests.py` structurally cannot cover it.  That script answers
# "which *surviving* files are unresolved?", and a deleted file survives
# nothing -- so a deletion removes a request from the one report that exists to
# find it, silently, and in the direction that reads as "nothing is open".
# Only a diff against history can see a deletion at all.
#
# Costs one `git diff` against the merge base with origin/main, so it sees only
# what this lane removed since diverging and can never fail one lane's build
# with another lane's history.  A rename passes (rename detection is on), an
# uncommitted `rm` counts, and `requests/.deletions-allowed` waives a basename
# with a stated reason for when a deletion really is right.
check_requests_not_deleted() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== request-deletion check: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking that no request file was deleted ==="
    if run_checker check-requests-not-deleted "$py" "$PROJECT_ROOT/scripts/check-requests-not-deleted.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  A request that has landed gets a" >&2
    echo "**Status:** line and stays where it is -- it is not a ticket, it is" >&2
    echo "the argument, and code and documents across the tree cite it by path." >&2
    echo "" >&2
    echo "The restore command is printed above.  Use an open/blocked/partial" >&2
    echo "wording if only part of it landed: open-requests.py ranks those above" >&2
    echo "'landed', so an honest header is what keeps the unfinished half" >&2
    echo "visible instead of hiding it behind a tick." >&2
    exit 1
}

check_requests_not_deleted

# A file declared `text eol=lf` that holds CRLF on disk, which no git command
# you would think to run will tell you about.
#
# On 2026-09-03 this sweep went red at `check_shellcheck`, ~45 minutes in, on
# thirteen files a tool in this tree had rewritten in text mode.  The whole
# working tree said clean the entire time: `git status`, `git diff`, `git diff
# --quiet` and `git add` all compare *through* the clean filter, which converts
# CRLF to LF before any comparison happens, so a wholly-CRLF file is identical
# to the index as far as every command anyone actually runs is concerned.
# Staging the repair of all thirteen produced a zero-byte diff.  Only
# `git diff-files` and `git ls-files --eol` see the raw bytes, and nobody runs
# those.  `text eol=lf` is a promise kept at checkout, not an invariant checked
# afterwards -- so nothing was checking it.
#
# It runs HERE, second, for the reason the cost was 45 minutes rather than 30
# seconds.  It cannot claim the first slot: the gate above it is about
# information that is already destroyed, and this one is only about wasting a
# cycle.  Everything else in the sweep can wait behind it.
#
# Scope is every declared file, not just `*.sh`, deliberately.  `check_shellcheck`
# is what caught this, and it covers `.sh` -- so it saw one of the thirteen and
# was silent about the other twelve, which is exactly the shape that makes a
# whole-tree corruption read like a one-file typo.  The thing worth learning
# from a finding here is that some tool wrote text in the wrong mode; a
# `.sh`-only gate hides the size of that.  Costs ~32 seconds reading 37 MB
# across 1438 files, pooled 16 ways -- the cost is per-file antivirus
# interception, not bandwidth (see open-questions.md A-Q7).
check_eol() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== eol check: skipped (no python) ===" >&2
        return 0
    fi

    # Graded against the real tree first.  Every way this checker can break --
    # a `git check-attr` stream walked out of alignment, an attribute filter
    # that matches nothing, a CR test that only counts CRLF -- makes the
    # declared set or the finding set go *empty*, and an empty finding set is
    # reported in the same words as a clean tree.  Its self-test drives the
    # whole pipeline end to end for that reason: a fixture aimed at the parts
    # cannot see a gate that finds a defect, prints it, and returns 0 anyway.
    echo "=== Checking the eol gate against the tree it grades ==="
    if ! run_checker check-eol-selftest "$py" "$PROJECT_ROOT/scripts/check-eol.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The eol gate no longer agrees with the" >&2
        echo "tree it grades, so its verdict means nothing -- every failure mode" >&2
        echo "it has empties its own finding set, which it reports exactly the" >&2
        echo "way it reports a clean tree." >&2
        exit 1
    fi

    echo "=== Checking that no file declared eol=lf holds a carriage return ==="
    if run_checker check-eol "$py" "$PROJECT_ROOT/scripts/check-eol.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  Each file above is declared \`text eol=lf\`" >&2
    echo "in .gitattributes and has CRLF line endings on disk." >&2
    echo "" >&2
    echo "Do not trust git about this.  \`git status\` and \`git diff\` will call" >&2
    echo "the tree clean, because they compare through the clean filter, which" >&2
    echo "normalises the CRLF away before the comparison.  Staging the repair" >&2
    echo "produces an empty diff.  Use \`git ls-files --eol\` to see it." >&2
    echo "" >&2
    echo "Repair by rewriting the bytes -- read the file, replace CRLF with LF," >&2
    echo "write it back in binary mode.  Then find what wrote it that way: this" >&2
    echo "is almost always a script opening a file in text mode on Windows, and" >&2
    echo "fixing the file without fixing the writer just schedules the next one." >&2
    exit 1
}

check_eol

# `check_libc_shape` began, from the day it was wired until 2026-09-04, with
# `py="$(find_python)" || return 0`.  `find_python` is defined nowhere -- not
# here, not in run-checker.sh, not on PATH -- so the substitution exited 127,
# the `|| return 0` turned that into a pass, and the gate together with its
# 24-case self-test had never executed once, on any host, in its life.
#
# Nothing in this script could have told us.  `set -e` does not fire, because
# `||` is an explicit handler and that is what it is for.  `bash -n` accepts the
# line, because the syntax is valid and the failure is at run time.  shellcheck
# is looking for unset *variables*, and this is a command.  And
# `check-gates-are-wired.py` counted the gate as wired, correctly by its own
# rule, because the `run_checker` call site is right there in the text -- which
# is the lesson, and the next term past design-decisions.md §907: a gate is
# what `run_checker` runs, not what it is named -- and *a call site that exists
# is not a call site that executes*.
#
# So this gate asks the question no other one does: does every literal command
# substitution in the tree's shell call something that exists?  Across the 104
# graded files it found exactly one defect, the one above.  That measured blast
# radius of one is the whole argument for its narrow shape -- it fires on real
# bugs and not on working code, so it will still be on next month.  Costs ~14
# seconds.  See known-issues.md ->
# A-A-THE-LIBC-SHAPE-GATE-WAS-BORN-DEAD-AND-THE-WIRING-GATE-CALLS-IT-WIRED.
check_shell_callables() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== shell callee check: skipped (no python) ===" >&2
        return 0
    fi

    # Graded against the real tree first, for the same reason check_eol is.
    # Every way this checker breaks -- a runaway heredoc that swallows a file,
    # an over-eager escape rule, a `command -v` batch whose framing drifted --
    # makes its *candidate* set go empty, and an empty candidate set is
    # reported in precisely the words of a clean tree.  A `<<<` here-string
    # read as a `<<` opener did exactly that during development: boot-test.sh
    # silently dropped from 114 command substitutions to 46, with no other
    # symptom.  The self-test counts what the real boot-test.sh yields, so that
    # failure cannot pass.
    echo "=== Checking the shell-callee gate against the tree it grades ==="
    if ! run_checker check-shell-callables-selftest "$py" \
            "$PROJECT_ROOT/scripts/check-shell-callables.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The shell-callee gate no longer agrees" >&2
        echo "with the tree it grades, so its verdict means nothing -- every" >&2
        echo "failure mode it has empties its own candidate set, which it" >&2
        echo "reports exactly the way it reports a clean tree." >&2
        exit 1
    fi

    echo "=== Checking that every shell command substitution calls something real ==="
    if run_checker check-shell-callables "$py" \
            "$PROJECT_ROOT/scripts/check-shell-callables.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  A command substitution above calls a name" >&2
    echo "that nothing in reach defines.  At run time it exits 127 and yields" >&2
    echo "the empty string; if it is guarded by \`||\`, wrapped in \`if\`, or" >&2
    echo "assigned with \`local\`, that status is swallowed and the caller" >&2
    echo "proceeds with an empty value believing it succeeded." >&2
    echo "" >&2
    echo "That is how this script shipped a gate that never ran once.  The" >&2
    echo "checker's own output above says which name and which line." >&2
    exit 1
}

check_shell_callables

# Every gate in this script trusts that a checker which finds a problem will
# *say so* by exiting non-zero.  `check-doc-links.py` did not.  A bare run of it
# fell through to `ap.print_help(); return 0` while every refusal sat behind
# `--check`, so it scanned the whole tree for 412 seconds, found dead links,
# printed them where `pre-boot.py` discarded them, and reported success.  It was
# found by accident, because a log ended in a usage message.
#
# Note what could not have found it.  Every self-test in `scripts/` aims at a
# checker's *detector* -- feed it a planted defect, assert the finding appears.
# All of them would have passed on that file, because its detector was fine.  A
# gate has two halves, detect and *refuse*, and a suite pointed at the first
# cannot see a hole in the second.
#
# So this gate checks the second half, mechanically: for each `check-*.py`, is
# any non-zero exit reachable from a bare invocation?  It is a static AST walk,
# not an execution -- running all 31 costs ~38 minutes, and running them could
# not distinguish "passes on a clean tree" from "passes on everything" without
# planting a defect in each.  It errs toward silence: an unrecognised guard or a
# computed return counts as capable of refusing.
#
# It runs HERE, before the build, because it costs well under a second, and
# because a suite that cannot fail is the one thing there is no point
# discovering after ten minutes of compiling.
check_gates_can_refuse() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== gate-refusal check: skipped (no python) ===" >&2
        return 0
    fi

    # Its own cases first, for the reason this gate exists: an analyser that has
    # stopped analysing reports an empty finding list, which is spelled exactly
    # like a clean tree.  Its first version did precisely that -- green, and
    # blind to the real defect, because it modelled `if args.flag:` but not
    # `if args.flag is not None:`.
    if ! run_checker check-gates-can-refuse-selftest "$py" \
            "$PROJECT_ROOT/scripts/check-gates-can-refuse.py" --selftest; then
        echo "" >&2
        echo "ERROR: refusing to build.  The gate-refusal analyser fails its" >&2
        echo "own cases, so its verdict on every other gate is worthless --" >&2
        echo "and its failure mode is an empty report, which reads as a pass." >&2
        return 1
    fi

    echo "=== Checking that every gate can still refuse ==="
    if run_checker check-gates-can-refuse "$py" \
            "$PROJECT_ROOT/scripts/check-gates-can-refuse.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  Each checker named above cannot reach a" >&2
    echo "non-zero exit when run with no arguments -- which is how this script" >&2
    echo "and scripts/pre-boot.py run it.  Whatever it enforces, it does not." >&2
    echo "" >&2
    echo "The usual cause is that the refusal sits behind a flag: a bare run" >&2
    echo "prints help, or reports findings and returns 0.  The fix is to make" >&2
    echo "the bare run the checking run and keep the flag as an accepted no-op," >&2
    echo "as scripts/check-doc-links.py now does." >&2
    exit 1
}

check_gates_can_refuse

# The other half of the same question.  `check_gates_can_refuse` above asks
# whether a gate *can* say no; this asks whether anything ever *asks* it.
# A gate fails to enforce its rule if either answer is no, and the two are
# indistinguishable from a green log, because in both cases the evidence is an
# absence.
#
# This script does not glob `scripts/check-*.py` -- it names each checker in a
# `run_checker` call, so a gate nobody adds a call for runs only in
# `scripts/pre-boot.py`, a ~40-minute local pre-flight nobody is obliged to run.
# Measured 2026-09-02: nine of thirty-one gates were in that state, eight of
# them absent from the push hook as well.
#
# It is a RATCHET, not a gate: the eight known-unwired checkers are pinned in
# the script with a reason each, and it fails only when that set changes --
# a new unwired gate, a pinned entry that is now wired, or a pinned entry whose
# file is gone.  Six of the eight are lane C's, and failing outright would block
# all three lanes on work none of them scheduled.
check_gates_are_wired() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== gate-wiring check: skipped (no python) ===" >&2
        return 0
    fi

    # Its own cases first.  Reading shell well enough to answer this is fussier
    # than it looks -- three obvious methods all gave confidently wrong answers,
    # and one of them was broken by the very commit that motivated the check
    # (an `echo` naming a gate it does not run).  A parser that has quietly
    # stopped resolving calls reports every gate as unwired, or none.
    if ! run_checker check-gates-are-wired-selftest "$py" \
            "$PROJECT_ROOT/scripts/check-gates-are-wired.py" --selftest; then
        echo "" >&2
        echo "ERROR: refusing to build.  The gate-wiring analyser fails its" >&2
        echo "own cases, so it is no longer reading this script correctly and" >&2
        echo "its verdict cannot be trusted in either direction." >&2
        return 1
    fi

    echo "=== Checking that every gate is actually run by something ==="
    if run_checker check-gates-are-wired "$py" \
            "$PROJECT_ROOT/scripts/check-gates-are-wired.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  The set of gates nothing runs has" >&2
    echo "changed.  Each line above is one of:" >&2
    echo "" >&2
    echo "  * a checker under scripts/ that no run_checker call names, so" >&2
    echo "    whatever rule it enforces is not enforced by this build;" >&2
    echo "  * a pinned exemption that is now wired, or whose file is gone --" >&2
    echo "    prune it, because an exemption list nobody prunes stops" >&2
    echo "    describing the tree it exempts;" >&2
    echo "  * a run_checker call whose script argument could not be resolved," >&2
    echo "    which is reported rather than skipped on purpose." >&2
    echo "" >&2
    echo "Add a run_checker call for the gate, or pin it in PINNED in" >&2
    echo "scripts/check-gates-are-wired.py with the reason it stays unwired." >&2
    exit 1
}

check_gates_are_wired

# The third arm of the same family, one level in.  The two above ask whether a
# gate *can* refuse and whether anything *asks* it to; this asks whether the
# thing that answers the harder question -- is each individual guard inside a
# gate load-bearing, or decoration? -- is still able to answer it at all.
#
# That question is not academic.  Twice now the answer has been "decoration":
# check-libc-shape.py pinned MIN_MEMBERS/MIN_SYMBOLS that main() never read,
# and check-doc-links.py grew a five-part coverage floor whose fixtures were
# derived from the constants under test, so gutting three of them shrank the
# fixtures to match and all 74 cases stayed green.  Nothing but a mutation
# sweep finds that shape: a green suite is the symptom, not the diagnosis.
#
# The sweep itself -- scripts/mutate-gate.py -- is NOT wired here, and that is
# a measured decision rather than an oversight.  On 2026-09-03, on a host busy
# with a boot test, sweeping the three tables took 9m38s against 24s for this
# gate, because the sweep is one subprocess per needle and each runs a gate's
# whole suite.  That figure grows with every row anyone adds, so the cost of
# catching a survivor rises in proportion to how much anyone tests.  Nine
# minutes per boot on a run that already spends forty on gates buys a gate
# people are tempted to skip, which is worse than no gate.
#
# What runs every time is the half that is static and is the half that rots.
# A needle is a *quotation* of the gate's source; reword the line it quotes and
# the needle matches nothing.  It does not fail -- it is skipped, and it leaves
# a row that reads as coverage over a guard that is once again untested.  The
# only thing that would say so is the sweep nobody has run since.
check_mutation_needles() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== mutation-needle check: skipped (no python) ===" >&2
        return 0
    fi

    # Its own cases first, for this family's usual reason and one of its own:
    # a scan that has stopped finding tables reports an empty finding list,
    # which is spelled exactly like a directory of live needles.  The gate
    # answers that with an absolute floor, and the floor is only worth what its
    # own cases are -- so they run before its verdict is believed.
    if ! run_checker check-mutation-needles-selftest "$py" \
            "$PROJECT_ROOT/scripts/check-mutation-needles.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The mutation-needle gate fails its" >&2
        echo "own cases, so its verdict on every mutation table is worthless" >&2
        echo "-- and its failure mode is an empty report, which reads as a" >&2
        echo "directory in which every needle is live." >&2
        return 1
    fi

    echo "=== Checking that every mutation needle still quotes something ==="
    if run_checker check-mutation-needles "$py" \
            "$PROJECT_ROOT/scripts/check-mutation-needles.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  A SELFTEST_MUTANTS row named above no" >&2
    echo "longer asks anything.  Each is one of:" >&2
    echo "" >&2
    echo "  * a needle quoting a line that has been reworded or deleted, so" >&2
    echo "    the sweep skips the row and the guard it covered is untested" >&2
    echo "    while the table still reads as coverage;" >&2
    echo "  * a needle now matching two sites, which cannot say which one it" >&2
    echo "    meant and which the sweep refuses to guess at;" >&2
    echo "  * a replacement identical to its needle, which mutates nothing" >&2
    echo "    and is indistinguishable from a mutant every case killed;" >&2
    echo "  * a table on a gate with no self-test for a sweep to run, which" >&2
    echo "    is decoration of exactly the kind the table detects." >&2
    echo "" >&2
    echo "Re-quote the row against the line as it now reads, or delete it." >&2
    echo "Then run scripts/mutate-gate.py on that gate to confirm the row" >&2
    echo "still kills -- this gate checks that the question is asked, not" >&2
    echo "that the answer is right." >&2
    exit 1
}

check_mutation_needles

# `check_unwired_gate_selftests` used to stand here. It ran the *fixtures* of
# three lane-C gates whose real checks nothing ran, on the reasoning that
# "unwired" and "rotting" are different problems: a checker waiting to be
# switched on drifts meanwhile, and the first real run of a drifted checker
# reports nothing, which reads exactly like a pass.
#
# Lane C wired all three for real on 2026-09-03 (`check_lane_c_gui_gates`,
# below), and ran each gate's `--self-test` immediately before the check it
# guards -- the same protection, sited better. Keeping both would have run
# three fixtures twice per boot under duplicate `run_checker` labels, which is
# itself a gate failure: `test-pre-push-run-checker.py` requires the labels in
# this file to be distinct, because a label is how a run is identified in the
# skiplog and in `bench/boot-history.jsonl`.
#
# The reasoning is preserved here rather than in a deleted commit because it is
# the argument for running a fixture at all, and it will be needed again the
# next time a gate is written before its tree is ready for it.

# A self-test that nothing calls is not a test.  It compiles, it reads as
# coverage, it gets cited in a commit message as "tested" -- and it has never
# executed.  `evdev::self_test` sat uncalled for exactly one commit, and the
# first boot that ran it failed hard on a real ordering bug
# (B-A-EVDEV-SYN-DROPPED-ARRIVES-ONE-RECORD-LATE); the audit that followed found
# forty more in the same state.  This gate keeps the count at zero.
#
# It runs HERE -- before the build -- because it costs milliseconds and the
# build costs ten minutes, and a wiring mistake is exactly the kind of thing
# there is no reason to discover after a build.
#
# Skipped only when Python is absent.  A script that exits 2 because it could
# not run counts as a failure, because a check that cannot fire must not be
# indistinguishable from a check that passed.
check_self_tests_wired() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Self-test wiring: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking that every self-test is reachable ==="
    # --emit-markers writes the serial line each conditionally-called self-test
    # declares it prints (`// RAN-IF: "..."` in main.rs).  boot-history.py reads
    # the file at the end of this run and records which ones actually appeared,
    # turning "is it wired" -- a question about source -- into "did it run",
    # which only the log can answer.
    #
    # Generated here rather than at recording time so it describes the tree that
    # was *built*: this runs before the build, and HEAD moves during a run
    # (committing while a boot test runs is normal here), so re-deriving the
    # markers twenty minutes later could describe a main.rs that never booted.
    mkdir -p "$PROJECT_ROOT/build"
    if run_checker check-self-tests-wired "$py" "$PROJECT_ROOT/scripts/check-self-tests-wired.py" \
            --emit-markers "$GATED_MARKERS"; then
        GATED_MARKERS_FRESH=1
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  Wire the self-test(s) above into" >&2
    echo "kernel_main, or allowlist them with a reason.  Fixing this now" >&2
    echo "costs a minute; the ten-minute build that follows would prove" >&2
    echo "nothing about code that never runs." >&2
    exit 1
}

check_self_tests_wired

# Refuse to build when a lock guard is held across a call that re-takes the same
# lock.
#
# This is the cross-function form of the bug that froze a boot in
# `fs::encrypt::encrypt`: a task asking for a lock it already holds, on a
# non-reentrant spinlock, which spins forever.  The single-statement form
# (`X.lock().n = X.lock().n + 1`) is findable with a grep.  The cross-function
# form is not -- no single line mentions the lock twice -- and it produces a
# boot that hangs with no output at all, which is the most expensive failure
# mode this project has.  When the checker was first written it found two, one
# of them reachable by typing `boot` at the kernel shell.
#
# Gated here, before the build, for the same reason as the self-test check: it
# costs milliseconds and the build costs ten minutes.  Exit 2 (could not run at
# all) is treated as a failure, because a check that cannot fire must never be
# indistinguishable from a check that passed.
check_recursive_locks() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Recursive-lock check: skipped (no python) ===" >&2
        return 0
    fi

    # The four source gates share one Rust scanner, and its failure mode is
    # silence: a scanner that loses brace nesting sees no functions, so it
    # reports no findings, which reads exactly like a clean tree.  It really did
    # hide a lock-order inversion for as long as `kshell.rs` contained a `'"'`.
    # Run its own cases first so a parser regression is reported as one.
    echo "=== Checking the shared Rust source scanner ==="
    if ! run_checker check-recursive-locks-selftest "$py" "$PROJECT_ROOT/scripts/check-recursive-locks.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The scanner that backs the lock, VFS" >&2
        echo "and self-test gates mis-parses a Rust literal form.  Until it is" >&2
        echo "fixed those gates are not checking anything, and their silence" >&2
        echo "must not be read as a pass." >&2
        return 1
    fi

    echo "=== Checking for guards held across a re-acquiring call ==="
    if run_checker check-recursive-locks "$py" "$PROJECT_ROOT/scripts/check-recursive-locks.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  Each report above is a task that would" >&2
    echo "ask for a lock it already holds -- an unbounded spin with no serial" >&2
    echo "output, not a crash.  Drop the guard before the call, or split the" >&2
    echo "callee so the locked part is separate.  The analysis is within-file" >&2
    echo "and hand-checkable: read the two functions it names before changing" >&2
    echo "anything." >&2
    exit 1
}

check_recursive_locks

# Keep module-global locks out from under calls into the VFS.
#
# Same family as the check above, and the same placement rationale: it costs
# milliseconds against a ten-minute build, and it must run before the build so
# a check that cannot run never looks like a check that passed.
#
# The failure is an AB/BA deadlock, not a recursive one.  `Vfs::readdir` takes
# the *filesystem's* lock and, for a generated filesystem like procfs, calls
# the subsystem that produces the content -- which takes that subsystem's
# module-global lock.  So the live order is filesystem lock -> module state.
# A function that holds module state and then calls into the VFS runs that
# order backwards, and two CPUs, one in each path, wedge each other with no
# output at all.  This is not hypothetical: the case that prompted the checker
# sat on the path of every file read and write.
#
# The sweep that brought the count from 31 to 0 is in the git history; every
# fix has the same shape, which is the shape to follow for a new report.
# Snapshot what the VFS call needs while the lock is held, drop the guard, make
# the call, then retake the lock and re-look-up by key or name to write the
# result back.  Never carry a reference -- or a bare index -- into a container
# across the call, because the container can move underneath it.  Where the
# work genuinely needs `&mut` for its whole duration (`net/ssh.rs`), check the
# object out of its slot with `mem::replace` and leave a placeholder behind.
check_vfs_lock_order() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== VFS lock-order check: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking for module locks held across a call into the VFS ==="
    if run_checker check-vfs-under-lock "$py" "$PROJECT_ROOT/scripts/check-vfs-under-lock.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  Each report above holds a module-global" >&2
    echo "lock across a VFS call, which inverts the kernel's filesystem-lock" >&2
    echo "-> module-state order and can wedge two CPUs against each other with" >&2
    echo "no serial output.  Snapshot what the call needs, drop the guard, then" >&2
    echo "retake it and re-look-up by key to write back.  The analysis is" >&2
    echo "within-file and hand-checkable: read the function it names first." >&2
    echo "" >&2
    echo "Note it reports only the FIRST VFS call under a given guard, so a" >&2
    echo "function with several must have all of them moved, not just the one" >&2
    echo "named -- otherwise the next one simply takes its place." >&2
    exit 1
}

check_vfs_lock_order

# Keep kernel writes to user memory inside the validated primitives.
#
# Same placement rationale as the two checks above: it costs milliseconds, the
# build costs ten minutes, and a check that cannot run must not look like a
# check that passed.
#
# The failure it guards is the quietest one in the tree.  Writing to a user page
# through the HHDM alias of its physical frame is a write to a *kernel* address
# in a writable mapping, so neither SMAP nor the page's own write-protect bit
# applies -- a copy-on-write page gets modified in place and the process sharing
# it silently diverges, with no fault and no log line.  Unlike
# `W-KERNEL-COW-WRITE`, which at least announces itself as a ring-0 #PF, nothing
# reports this at all, which is why it is worth a pre-build gate rather than a
# boot-time self-test.
check_user_access_sites() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== User-access-site check: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking kernel writes to user memory ==="
    if run_checker check-user-access-sites "$py" "$PROJECT_ROOT/scripts/check-user-access-sites.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  Each report above either opens a SMAP" >&2
    echo "window outside mm/user.rs, or writes through the HHDM alias of a page" >&2
    echo "it resolved with page_table::translate -- which reports a frame" >&2
    echo "without regard to whether the mapping is writable.  Route it through" >&2
    echo "mm::user::copy_to_user_as, which checks WRITABLE and breaks a CoW" >&2
    echo "before writing.  If the frame genuinely belongs to no address space" >&2
    echo "yet, add the file to ALLOWED_HHDM_WRITERS with a note saying why." >&2
    exit 1
}

check_user_access_sites

# Keep every path-taking VFS entry point behind the one permission gate.
#
# This guards the failure mode that no runtime test can see, because both
# checks the gate runs fail *open*: a VFS method that forgets to call
# `check_path_access` reads and writes files exactly as it always did, and a
# test that exercises permitted access passes identically against a gate that
# is not there.  Only the source-level invariant notices.
#
# It had already failed in both directions at once.  `file_tags::check_access`
# was hand-written into sixteen call sites in fs/vfs.rs plus a seventeenth
# copied into fs/handle.rs -- and missing from roughly twenty more entry points
# nobody had remembered.  `acl::check_access`, the entire POSIX 1003.1e
# evaluation algorithm, was called from nowhere at all: `setfacl` validated and
# stored ACLs, `getfacl` read them back, procfs counted them, and no file
# operation ever consulted one.
check_vfs_permission_gate() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== VFS permission-gate check: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking the VFS permission gate ==="
    if run_checker check-vfs-permission-gate "$py" "$PROJECT_ROOT/scripts/check-vfs-permission-gate.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  Each report above is a way for a file" >&2
    echo "operation to bypass access control without anything noticing, since" >&2
    echo "both the capability-tag and POSIX-ACL checks fail open." >&2
    echo "" >&2
    echo "A Vfs method that resolves a path must call check_path_access with" >&2
    echo "the PathAccess it intends (Read / Write / Execute / Metadata).  If it" >&2
    echo "genuinely acts on the filesystem rather than on the path -- statvfs," >&2
    echo "trim, the mount table -- add it to UNGATED in the script with a" >&2
    echo "reason, because that list is the audit trail." >&2
    echo "" >&2
    echo "acl::check_access and file_tags::check_access each have exactly one" >&2
    echo "legal caller, inside the gate.  Route new checks through the gate" >&2
    echo "rather than calling them directly: a hook you must remember at every" >&2
    echo "entry point is a hook the next entry point will not have." >&2
    exit 1
}

check_vfs_permission_gate

# Keep the shell's exit statuses honest: a usage message is a failure report.
#
# When a kshell command prints `Usage: ...` because it could not do what it was
# asked, it must also set a non-zero exit status.  Otherwise the diagnostic goes
# to the screen and a success goes to the caller -- and a script reads the
# caller's copy, so `cmd && next` runs `next` after a typo and `set -e` does not
# stop.
#
# This is checked rather than reviewed because it has now been got wrong twice.
# A sweep in August fixed 710 sites by searching for the shape they had (a usage
# print followed by a bare `return;`) and reported itself complete, having
# missed 87 that leave by falling off the end of a `match` arm instead.  A sweep
# keyed on a syntactic pattern defines its own blind spot and cannot report it;
# the checker is keyed on the property instead, so it does not care what shape
# the next one is written in.  See design-decisions.md §296.
check_usage_status() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== usage-status check: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking that usage messages report failure ==="
    if run_checker check-usage-status "$py" "$PROJECT_ROOT/scripts/check-usage-status.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  Each site above prints a usage message and" >&2
    echo "then tells its caller the command succeeded." >&2
    echo "" >&2
    echo "Add set_exit(1) after the diagnostic.  If the site is NOT an error --" >&2
    echo "a bare subcommand printing its current setting is a query, and it" >&2
    echo "succeeded -- add it to ALLOWED in the script with a reason instead." >&2
    echo "Get that distinction backwards and you produce the mirror-image bug:" >&2
    echo "'elog echo' and 'fc algo' answered correctly and reported failure for" >&2
    echo "a month because the previous sweep patched their query paths." >&2
    exit 1
}

check_usage_status

# ... and the mirror of it: answering a question is not reporting a failure.
#
# The rule above, applied to a site it does not fit, produces the opposite bug.
# `elog echo` and `fc algo` print the current setting when given no value and
# append a usage line as a hint for changing it; the August sweep read the hint
# as a complaint and gave both a `set_exit(1)`, so for a month they printed the
# right answer and told the caller they had failed.  `$(fc algo)` under `set -e`
# killed the script *after* producing the value it asked for.
#
# Deliberately a separate script rather than a second rule inside the first: the
# two point in opposite directions, and a single classifier holding both would
# resolve a disagreement between them silently.  Apart, each states a property,
# and a site that trips both is a site whose author has to say which it is.
check_query_status() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== query-status check: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking that answering a query reports success ==="
    if run_checker check-query-status "$py" "$PROJECT_ROOT/scripts/check-query-status.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  Each block above can only be reached by the" >&2
    echo "user asking -- no argument was given -- and it answers with the current" >&2
    echo "state and then reports failure." >&2
    echo "" >&2
    echo "Drop the set_exit.  If the block really is an error -- a required" >&2
    echo "operand is missing, so nothing was answered -- add it to ALLOWED in the" >&2
    echo "script with a reason instead." >&2
    exit 1
}

check_query_status

# Keep every option word either understood or refused (design-decisions.md §600).
#
# The two gates above are about what a command *says*.  This one is about what
# it does with a word it could not read, and the answer must never be "carry on
# as though it were not there".  A dropped word runs a different command --
# successfully -- and the difference is nearly always in the direction of doing
# more, because the word that fell out was a filter or a restriction.
#
# It is here rather than in code review because the rule was broken in nine
# commands at once and nobody saw it: `batch delete --dry-runn a b` deleted `a`
# and `b` for real, the typo matching no flag and then being filtered out of the
# file list, so it was neither a dry run nor an error.  Each half of that was
# individually reasonable; only the pair was a bug, and the pair is invisible
# from inside either function.
check_option_refusal() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== option-refusal check: skipped (no python) ===" >&2
        return 0
    fi

    # Its own cases first.  This gate shipped a `--self-test` that nothing ran
    # -- found 2026-09-02 by check-gates-are-wired.py, which is the only reason
    # anyone looked.  A self-test only stays honest while something executes
    # it; unrun, it is a file that describes a scanner rather than tests one.
    # This scanner reads Rust source, so its failure mode is the usual silent
    # one: lose the parse and it reports no findings, which is spelled exactly
    # like a clean kshell.rs.
    if ! run_checker check-option-refusal-selftest "$py" \
            "$PROJECT_ROOT/scripts/check-option-refusal.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The option-refusal scanner fails its" >&2
        echo "own fixtures, so its verdict on kshell.rs is worthless -- and it" >&2
        echo "fails by reporting nothing, which reads as a clean tree." >&2
        return 1
    fi

    echo "=== Checking that an option word is refused, not discarded ==="
    if run_checker check-option-refusal "$py" "$PROJECT_ROOT/scripts/check-option-refusal.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  A command-line word is either understood" >&2
    echo "or refused; it is never discarded, and a value is never invented for" >&2
    echo "one that could not be read." >&2
    echo "" >&2
    echo "For a parser with no way to say no: give the catch-all arm a" >&2
    echo "shell_println! naming the word, set_exit(1), and a return -- and add" >&2
    echo "a '--' end-of-options marker, since refusing dash-leading words is" >&2
    echo "what makes a file named '-x' otherwise unnameable." >&2
    echo "" >&2
    echo "For an invented value: an absent argument may take a default; a" >&2
    echo "present but unparseable one may not.  Split the two cases." >&2
    echo "" >&2
    echo "If a site is genuinely right, add it to ALLOWED in the script with a" >&2
    echo "written reason.  Do NOT add a function to option-refusal-ledger.txt" >&2
    echo "to silence new code -- that ledger only ever shrinks." >&2
    exit 1
}

check_option_refusal

# A self-test may not compare two readings of a counter that never stops.
#
# Here for the same reason as the check above it: the rule was broken in four
# places in one day, each locally reasonable, and the fourth panicked the
# kernel on a green tree over a single timer interrupt --
#
#     [5/6] tick cross-check (39171 >= 39171): OK
#     !!! KERNEL PANIC !!!
#     panicked at kernel/src/fs/irqstat.rs:354:5:
#     assertion `left == right` failed
#       left: 39171
#      right: 39170
#
# What makes it gate material rather than review material is that the rung
# three lines above the panic reasons this exact race out correctly, at
# length, and then the same function walks into it anyway. Knowing a counter
# moves is not the same as noticing every place you assumed it did not.
check_live_counter_reads() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== live-counter check: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking that no self-test compares two readings of one counter ==="
    if run_checker check-live-counter-reads "$py" "$PROJECT_ROOT/scripts/check-live-counter-reads.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  A live counter -- the timer, an interrupt," >&2
    echo "the clock, a CPU coming online -- moves between two reads of it, so an" >&2
    echo "equality assertion across two readings holds only when nothing" >&2
    echo "happened in between.  That is not a test; it is a race that usually" >&2
    echo "wins." >&2
    echo "" >&2
    echo "The fix is structural, not a wider tolerance: take ONE snapshot and" >&2
    echo "derive both sides from it, so the comparison cannot be handed two." >&2
    echo "See kernel/src/fs/irqstat.rs (totals_from / lines_from).  Where the" >&2
    echo "counter is monotone and you cannot snapshot it, bracket it instead --" >&2
    echo "read before and after and assert membership in that range, which is" >&2
    echo "exact rather than merely tolerant.  See kernel/src/fs/sysfs.rs." >&2
    echo "" >&2
    echo "If two readings are genuinely meant to be independent, add a line to" >&2
    echo "scripts/live-counter-ledger.txt with the reason." >&2
    exit 1
}

check_live_counter_reads

# Keep self-test skips honest: looked up, and reported.
#
# A self-test may legitimately skip -- there is no second CPU to offline on a
# uniprocessor, no PCID on a CPU without it.  Two things make a skip a lie
# instead of a fact, and this gate refuses both.
#
# The first is the reported half.  If the skip never reaches the summary, the
# last line still reads `Self-test PASSED` and a half-run is
# byte-indistinguishable from a full one.  That is not hypothetical: 26 tcc
# rungs no-op'd unnoticed for weeks behind a green line
# (known-issues.md -> B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT).
#
# The second is the larger one: how the skip was *decided*.  Writing
# `if mkdir(d).is_ok() { ..test.. } else { skip }` reads as "skip when this
# filesystem has no directories" but means "skip on **any** failure" --
# including the regression the section exists to catch.  Under that shape the
# worse the code under test gets, the more sections switch themselves off, and
# the suite goes green precisely when it should be loudest.  The precondition
# must be a fact the test looked up (the mount table, CPUID, a feature query),
# never an error it inferred.
#
# Placed before the build for the same reason as its siblings: it costs
# milliseconds against a ten-minute build, and exit 2 (could not run) counts as
# failure so a check that cannot fire never looks like one that passed.
check_selftest_skips() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Self-test skip check: skipped (no python) ===" >&2
        return 0
    fi

    # This gate decides what counts as "the suite" by a call-graph closure, and
    # that closure fails silently in both directions: too wide and it grades the
    # whole file, too narrow and it grades nothing.  Its fixture runs first so
    # either collapse is reported as a gate fault rather than as a clean tree.
    echo "=== Checking the self-test skip gate against its fixture ==="
    if ! run_checker check-selftest-skips-selftest "$py" "$PROJECT_ROOT/scripts/check-selftest-skips.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The self-test skip gate no longer" >&2
        echo "agrees with its own fixture, so its verdict on the tree means" >&2
        echo "nothing -- a gate that grades the wrong set of functions reports" >&2
        echo "zero findings just like a clean tree does." >&2
        return 1
    fi

    echo "=== Checking that self-test skips are looked up and reported ==="
    if run_checker check-selftest-skips "$py" "$PROJECT_ROOT/scripts/check-selftest-skips.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  A self-test may skip, but it must skip" >&2
    echo "for a reason it looked up, and it must say so in the line a reader" >&2
    echo "believes." >&2
    echo "" >&2
    echo "For a skip decided from .is_ok()/.is_err() on the code under test:" >&2
    echo "ask the environment instead -- crate::fs::selftest::is_mounted(p) /" >&2
    echo "is_mounted_rw(p), a CPUID/feature query, a capability lookup.  When" >&2
    echo "an error genuinely must be classified, use selftest::classify: only" >&2
    echo "NotSupported / ReadOnlyFilesystem / NoSuchDevice mean 'this system" >&2
    echo "cannot'.  Anything else means the system was asked and refused, and" >&2
    echo "that is a defect the test must fail on." >&2
    echo "" >&2
    echo "For an unconditional success line printed after a section skipped:" >&2
    echo "thread a crate::fs::selftest::Skips through the test, record(section," >&2
    echo "why) at each skip, and close with skips.report(tag) plus" >&2
    echo "skips.suffix() appended to the PASSED line." >&2
    exit 1
}

check_selftest_skips

# A shell self-test asserts on wording, and wording is the thing this tree is
# busiest changing.
#
# `kshell::self_test` runs only inside QEMU -- the kernel binary carries
# `test = false`, because a bare-metal crate supplies its own panic lang item
# and cannot link the host test harness -- so an assertion that names text no
# command can print any more is invisible to `cargo check`, `cargo clippy` and
# every other gate here, and shows up as a panicked kernel eleven minutes into
# a boot.
#
# It has happened.  On adddc7459 a table of nine commands asserted each one's
# arity complaint contained `Usage:`; `vd remove` was then converted to
# `required_id` and began saying `vd: remove: missing desktop id`, which is
# strictly better, and the rung took the kernel down *for the improvement*.
# The defect class is not "someone typed Usage:" -- it is an assertion whose
# expected text no longer belongs to the command under test, in either
# direction: a `contains` that fails a correct kernel, or a `lacks` that can no
# longer fire and so guards nothing.  This gate found one of each still live in
# the tree the day it was written.
#
# Before the build, like its siblings: seconds against ten minutes, and exit 2
# counts as failure so a gate that cannot run never reads as one that passed.
check_selftest_wording() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Self-test wording check: skipped (no python) ===" >&2
        return 0
    fi

    # Grade the gate before letting it grade the tree.  Every way this checker
    # can break -- a call-graph edge it stops following, a `match` arm it stops
    # narrowing to, a `self_test` span cut short by a brace inside a comment --
    # makes findings *disappear*, and it reports that in the same words as a
    # clean tree.  Its fixture is the `adddc7459` bug in miniature, so a gate
    # that has lost the ability to catch the one bug it was written for says so
    # here, instead of nodding a broken tree through to an eleven-minute boot.
    echo "=== Checking the self-test wording gate against its fixture ==="
    if ! run_checker check-selftest-wording-selftest "$py" "$PROJECT_ROOT/scripts/check-selftest-wording.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The self-test wording gate no longer" >&2
        echo "agrees with its own fixture, so its verdict on the tree means" >&2
        echo "nothing -- a gate whose analysis has collapsed reports zero" >&2
        echo "findings just like a clean tree does." >&2
        exit 1
    fi

    echo "=== Checking that self-test assertions name text their command prints ==="
    if run_checker check-selftest-wording "$py" "$PROJECT_ROOT/scripts/check-selftest-wording.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  Each report above is an assertion whose" >&2
    echo "expected text the command under test cannot produce." >&2
    echo "" >&2
    echo "For an assert_output_contains: the rung will panic the kernel on a" >&2
    echo "correct boot.  Re-read what the command says now and assert that --" >&2
    echo "usually the operand helper's wording, 'cmd: sub: missing <noun>' or" >&2
    echo "\`word' is not a <noun>." >&2
    echo "" >&2
    echo "For an assert_output_lacks: the assertion can never fire, so the" >&2
    echo "regression it was written to catch is no longer guarded.  Point it at" >&2
    echo "the sentence a *wrong* run would print today." >&2
    echo "" >&2
    echo "If the expected text is genuinely right and merely underivable -- a" >&2
    echo "value the command rearranged out of the rung's own data -- add it to" >&2
    echo "ALLOWED in the script with the reason.  Do not widen the analysis to" >&2
    echo "make a real finding disappear." >&2
    exit 1
}

check_selftest_wording

# The same defect class, everywhere the sibling gate cannot see.
#
# `check-selftest-wording.py` follows a kshell *command* to the text it prints,
# so it only ever looks at rungs keyed on a command word.  A module self-test
# that formats a string and asserts on it -- `assert!(metrics.contains("..."))`
# in `net::dashboard`, `procfs`, `smtp`, `klog` -- has no command word to
# resolve and no `capture_command` to key on, so the sibling never looked at it,
# and 300 assertions of that shape were guarded by nothing.
#
# "Guarded by nothing" is not hyperbole: the kernel crate carries `test = false`
# (a bare-metal binary supplies its own `panic_impl`), so no assertion in it can
# run on the host.  The only thing that executes one is an eleven-minute QEMU
# boot, and `cargo build` / `cargo clippy` are green on a tree whose self-test
# panics on the first line it reaches.  Worse is the vacuous variant, which does
# not even panic: `dashboard` asserted `contains("os_net_rx_bytes_total ")` with
# a trailing space, which the `# HELP` / `# TYPE` lines satisfy on their own, so
# once the metric gained a label the assertion went on nodding at a build
# emitting no samples at all.
check_selftest_format_wording() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Self-test format wording check: skipped (no python) ===" >&2
        return 0
    fi

    # Graded against its own fixtures first, for the reason the sibling is:
    # every way this checker can break -- a raw string it stops parsing, a
    # consuming position it stops recognising, a `self_test` span cut short --
    # makes findings *disappear*, and it reports that in the same words as a
    # clean tree.
    echo "=== Checking the self-test format wording gate against its fixtures ==="
    if ! run_checker check-selftest-format-wording-selftest "$py" "$PROJECT_ROOT/scripts/check-selftest-format-wording.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The self-test format wording gate no" >&2
        echo "longer agrees with its own fixtures, so its verdict on the tree" >&2
        echo "means nothing -- a gate whose analysis has collapsed reports zero" >&2
        echo "findings just like a clean tree does." >&2
        exit 1
    fi

    echo "=== Checking that self-test assertions name text some literal produces ==="
    if run_checker check-selftest-format-wording "$py" "$PROJECT_ROOT/scripts/check-selftest-format-wording.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  Each report above is a self-test assertion" >&2
    echo "demanding text that no string literal in the kernel can produce." >&2
    echo "" >&2
    echo "Usually the producer's wording moved and the assertion did not: a" >&2
    echo "field gained a label, a header lost a colon, a unit changed.  Re-read" >&2
    echo "what the code emits now and assert that -- as written, the assertion" >&2
    echo "will panic the kernel on a correct boot." >&2
    echo "" >&2
    echo "If the text is genuinely right and merely underivable -- a value the" >&2
    echo "code computes, or a header pasted together from byte slices -- add it" >&2
    echo "to ALLOWED in the script with the reason.  Do not widen the analysis" >&2
    echo "to make a real finding disappear: the pool over-approximates already," >&2
    echo "and every widening costs a defect it would otherwise catch." >&2
    exit 1
}

check_selftest_format_wording

# The numbering that the whole self-test log is read by.
#
# A boot report says "rung 67 failed" and known-issues.md says "rung 79 cleared
# the 21 `matches!` sites" -- both are indexing a 103-rung log by number, so the
# numbers have to be a real index.  Two rungs sharing one is the failure that
# actually happens: near-simultaneous batches each take the next free number
# against the same tip, both compile, both run, both pass, and the reference
# that was supposed to locate a failure is what makes it ambiguous.  Only a
# reading of all 103 banners finds it, which is what this does.
check_selftest_rung_numbers() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Self-test rung numbering check: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking the rung-numbering gate against its fixture ==="
    if ! run_checker check-selftest-rung-numbers-selftest "$py" "$PROJECT_ROOT/scripts/check-selftest-rung-numbers.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The rung-numbering gate no longer agrees" >&2
        echo "with its own fixture, so its verdict means nothing -- it reports a" >&2
        echo "clean tree and a collapsed analysis in the same words." >&2
        exit 1
    fi

    echo "=== Checking that self-test rungs are numbered uniquely and in order ==="
    if run_checker check-selftest-rung-numbers "$py" "$PROJECT_ROOT/scripts/check-selftest-rung-numbers.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  The self-test rung numbering is not a usable" >&2
    echo "index into the log." >&2
    echo "" >&2
    echo "For a duplicate: two batches took the same next-free number against the" >&2
    echo "same tip.  Renumber the later one to the end -- do not renumber the" >&2
    echo "earlier, other documents already cite it." >&2
    echo "" >&2
    echo "For a gap: a rung was deleted or renumbered.  Close the hole, so that" >&2
    echo "'the log reached rung N' keeps meaning 'all N rungs ran' and a boot" >&2
    echo "truncated mid-self-test stays obvious." >&2
    exit 1
}

check_selftest_rung_numbers

# The four bash oracles: where kshell's quoting rules actually come from.
#
# Every other shell gate here reads kshell's source and checks it against a rule
# written down in this repository.  These four check the *rule* -- they hand the
# same bytes to real bash through WSL and compare.  A disagreement means our
# model of the shell is wrong, which no amount of internal consistency would
# ever reveal: the rest of the gates would go on agreeing with each other about
# the wrong answer.
#
# They were pinned as unwired from the day they were written, for a good reason
# that has now been dealt with: the boot test must run on a host carrying only
# the Rust toolchain and QEMU, and on a host without WSL these gates cannot ask
# bash anything at all.  Aborting the build there would make a quoting checker
# the reason all three lanes could not build.  Two things had to land first, and
# both have (2026-09-03): bashprobe now *declines* (exit 2) rather than
# reporting a finding (exit 1) when WSL is absent, and `run_checker` grew the
# per-call-site `--may-skip` channel that turns that decline into a loud skip.
#
# So the gates are `--may-skip` and the self-tests are NOT, and that asymmetry
# is the whole design rather than an oversight.  A self-test needs no WSL by
# construction -- it reads only fixtures the checker carries in its own source
# (lane A's rule, requests/a-b-yes-to-the-self-test-rule-and-one-half-it-does-
# not-cover.md §4) -- so on a WSL-less host these four still check their own
# tables, their floors, the port of shellquote.rs, and the transcription of the
# rung literals.  That is most of what can go wrong with them, and it is checked
# on every host.  What skips is only the half that genuinely requires bash.
check_bash_oracles() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== bash oracles: skipped (no python) ===" >&2
        return 0
    fi

    # A self-test failure is never skippable: it means the checker disagrees
    # with fixtures it carries itself, so its verdict on anything else is
    # worthless -- and unlike the gate, it cannot have been prevented from
    # looking by a missing tool.
    echo "=== Checking the four bash oracles against their own fixtures ==="
    if ! run_checker check-shellquote-vs-bash-selftest "$py" "$PROJECT_ROOT/scripts/check-shellquote-vs-bash.py" --self-test; then
        _bash_oracle_selftest_died check-shellquote-vs-bash.py
    fi
    if ! run_checker check-ansic-quoting-vs-bash-selftest "$py" "$PROJECT_ROOT/scripts/check-ansic-quoting-vs-bash.py" --self-test; then
        _bash_oracle_selftest_died check-ansic-quoting-vs-bash.py
    fi
    if ! run_checker check-kshell-pipeline-vs-bash-selftest "$py" "$PROJECT_ROOT/scripts/check-kshell-pipeline-vs-bash.py" --self-test; then
        _bash_oracle_selftest_died check-kshell-pipeline-vs-bash.py
    fi
    if ! run_checker check-kshell-rungs-vs-bash-selftest "$py" "$PROJECT_ROOT/scripts/check-kshell-rungs-vs-bash.py" --self-test; then
        _bash_oracle_selftest_died check-kshell-rungs-vs-bash.py
    fi

    local ran=0 skipped=0
    echo "=== Asking real bash whether these are bash's rules ==="
    if run_checker --may-skip check-shellquote-vs-bash "$py" "$PROJECT_ROOT/scripts/check-shellquote-vs-bash.py"; then
        if [ -n "$RUN_CHECKER_SKIPPED" ]; then skipped=$((skipped + 1)); else ran=$((ran + 1)); fi
    else
        _bash_oracle_disagreed check-shellquote-vs-bash.py "kernel/src/shellquote.rs"
    fi
    if run_checker --may-skip check-ansic-quoting-vs-bash "$py" "$PROJECT_ROOT/scripts/check-ansic-quoting-vs-bash.py"; then
        if [ -n "$RUN_CHECKER_SKIPPED" ]; then skipped=$((skipped + 1)); else ran=$((ran + 1)); fi
    else
        _bash_oracle_disagreed check-ansic-quoting-vs-bash.py "the \$'...' rules recorded in its own docstring"
    fi
    if run_checker --may-skip check-kshell-pipeline-vs-bash "$py" "$PROJECT_ROOT/scripts/check-kshell-pipeline-vs-bash.py"; then
        if [ -n "$RUN_CHECKER_SKIPPED" ]; then skipped=$((skipped + 1)); else ran=$((ran + 1)); fi
    else
        _bash_oracle_disagreed check-kshell-pipeline-vs-bash.py "kshell's expansion pipeline (sites 4/5/6/7)"
    fi
    if run_checker --may-skip check-kshell-rungs-vs-bash "$py" "$PROJECT_ROOT/scripts/check-kshell-rungs-vs-bash.py"; then
        if [ -n "$RUN_CHECKER_SKIPPED" ]; then skipped=$((skipped + 1)); else ran=$((ran + 1)); fi
    else
        _bash_oracle_disagreed check-kshell-rungs-vs-bash.py "the assertions in kshell self-test rungs 115 and 117"
    fi

    if [ "$skipped" -gt 0 ]; then
        echo "=== bash oracles: $ran asked bash, $skipped skipped (no WSL on this host) ==="
    else
        echo "=== bash oracles: all $ran agree with real bash ==="
    fi
}

_bash_oracle_selftest_died() {
    echo "" >&2
    echo "ERROR: refusing to build.  $1 no longer agrees with the" >&2
    echo "fixtures it carries in its own source." >&2
    echo "" >&2
    echo "This is not a WSL problem and skipping it would be wrong: a self-test" >&2
    echo "needs no bash, so it did look, and what it found was that the checker" >&2
    echo "is broken.  Until it is fixed, that gate's verdict about kshell -- on" >&2
    echo "every host, including the ones that do have WSL -- means nothing." >&2
    exit 1
}

_bash_oracle_disagreed() {
    echo "" >&2
    echo "ERROR: refusing to build.  Real bash disagrees with $1." >&2
    echo "" >&2
    echo "Read the direction of this carefully before changing anything: bash is" >&2
    echo "the oracle here, not the subject.  A failure means $2" >&2
    echo "is wrong, and the expectation written in the gate was our belief about" >&2
    echo "the shell rather than a fact about it.  Fix the Rust, or -- if the" >&2
    echo "divergence is deliberate -- move the case into that file's DIVERGENCES" >&2
    echo "list, where BOTH answers are pinned and it starts failing again if" >&2
    echo "either side changes.  Do not adjust the expectation to match our code:" >&2
    echo "that converts the one gate measuring reality into another one agreeing" >&2
    echo "with us." >&2
    echo "" >&2
    echo "If instead the gate says the transport is not faithful, nothing about" >&2
    echo "kshell has been tested at all -- the bytes reaching bash were not the" >&2
    echo "bytes written down.  See scripts/bashprobe.py." >&2
    exit 1
}

check_bash_oracles

# Is `libc.a` carved finely enough that a program can bring its own `getopt`?
#
# This is the GNU make failure of design-decisions.md S339: a member that
# defines `getopt` *and* something every program needs is a member no program
# can decline, so a program supplying its own `getopt` gets a duplicate symbol
# and does not link.  The gate reads the archive's symbol index directly and
# asks three questions of it -- each strict family owns its member outright, no
# member mixes a replaceable name with an unavoidable one, and no replaceable
# name shares a member with an ordinary one.
#
# It was pinned in check-gates-are-wired.py from the day it was written, with
# the reason "needs an opt-in skip channel in run-checker.sh first".  That
# channel now exists (`--may-skip`), so the pin is retired here.
#
# `--ignore-age` is load-bearing, not a convenience.  Bare, the gate declines
# whenever `posix/` is newer than the sysroot -- which is nearly always, since
# the sysroot is rebuilt by hand and posix/ is edited every day.  A gate that
# declines on every run is the failure mode already logged as OPEN in
# known-issues.md, and it would be self-inflicted here: we would have wired a
# gate that never answers.  With the flag it always answers a sound but weaker
# question -- is the archive *on disk* shaped correctly? -- and the staleness
# it stops reporting is not lost, because the fixture-freshness block above
# already prints a loud WARNING naming the newer posix/ sources.  Better a true
# answer about a slightly old archive than no answer about a current one.
#
# So what is `--may-skip` still for?  The archive being absent entirely, on a
# fresh checkout whose sysroot has never been built, and a symbol index that
# yielded so little the gate refuses to grade it.  Both are declines -- "could
# not look", exit 2 -- and neither is a pass.
#
# The self-test is NOT skippable, for the same reason the bash oracles' are
# not: it builds its own `ar` archives in memory and needs no sysroot at all,
# so on a machine with no `libc.a` it is the only thing still checking that
# this gate can tell a bad archive from a good one.
check_libc_shape() {
    # This block was `py="$(find_python)" || return 0` from the day the gate was
    # wired (e3e72d4bf) until 2026-09-04.  `find_python` was never written --
    # not here, not in run-checker.sh, not on PATH -- so the substitution failed
    # 127, the `|| return 0` turned that into "passed", and neither this gate
    # nor its self-test had ever executed on any host.  It announced itself once
    # per run as a single stderr line reading `line 4232: find_python: command
    # not found`, between two banners in a 60k-line log, carrying no ERROR and
    # not touching the exit status.  See known-issues.md ->
    # A-A-THE-LIBC-SHAPE-GATE-WAS-BORN-DEAD-AND-THE-WIRING-GATE-CALLS-IT-WIRED.
    #
    # Two things are deliberate in the replacement.  It is the same inline
    # `command -v` block the other ~20 gates use, rather than a helper: there is
    # no helper to call, and inventing one here would put the only caller of a
    # new abstraction in the file that just demonstrated why an uncalled name is
    # dangerous.  And the no-python arm *announces* the skip.  The old line
    # returned silently, which is a second defect that would have survived
    # merely defining `find_python` -- a gate that declines without saying so is
    # indistinguishable from one that looked and found nothing.
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== libc.a shape: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking libc.a member granularity ==="
    if ! run_checker check-libc-shape-selftest "$py" "$PROJECT_ROOT/scripts/check-libc-shape.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  check-libc-shape.py no longer agrees" >&2
        echo "with the synthetic archives it builds in its own source." >&2
        echo "" >&2
        echo "This needs no sysroot, so it did look, and what it found is that" >&2
        echo "the checker is broken -- which makes its verdict about the real" >&2
        echo "libc.a meaningless on every host, including this one." >&2
        exit 1
    fi

    if run_checker --may-skip check-libc-shape "$py" "$PROJECT_ROOT/scripts/check-libc-shape.py" --ignore-age; then
        if [ -n "$RUN_CHECKER_SKIPPED" ]; then
            echo "=== libc.a shape: skipped (no archive to grade on this host) ==="
        fi
    else
        echo "" >&2
        echo "ERROR: refusing to build.  libc.a is carved too coarsely." >&2
        echo "" >&2
        echo "A program that defines its own copy of one of the names above" >&2
        echo "cannot decline the member that also defines it, so it gets a" >&2
        echo "duplicate definition and fails to link.  This is exactly how the" >&2
        echo "GNU make port broke -- see design-decisions.md S339." >&2
        echo "" >&2
        echo "The fix is in how posix/ is compiled, not in this list: raising" >&2
        echo "codegen-units will NOT split a member, because rustc partitions" >&2
        echo "by module and a ceiling does not force a floor.  Move the named" >&2
        echo "symbols into a module of their own." >&2
        exit 1
    fi
}

check_libc_shape

# A diagnostic that names the wrong command is caught by nothing else.
#
# The operand helpers are handed their command's name as a bare string literal:
# `required_num::<u32>(&parts, 1, "epollstat", sub, "pid")`.  Copy that block
# into another function -- which is exactly what a whole-file search-and-replace
# during a burn-down pass does -- and the recipient starts announcing itself as
# the donor.  It compiles, it formats, and the option-refusal gate scores the
# site as fixed, because by its own measure it is.  The only evidence is in the
# text of a message no test reads, and its effect is to send whoever reads it to
# another command's source.
#
# The shell's own dispatch table settles what each function may legitimately
# call itself, aliases included, so this is decidable rather than a heuristic.
check_shell_message_names() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Shell message-name check: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking the message-name gate against its fixture ==="
    if ! run_checker check-shell-message-names-selftest "$py" "$PROJECT_ROOT/scripts/check-shell-message-names.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The message-name gate no longer agrees" >&2
        echo "with its own fixture, so its verdict means nothing -- it reports a" >&2
        echo "clean tree and a collapsed analysis in the same words." >&2
        exit 1
    fi

    echo "=== Checking that each shell diagnostic names its own command ==="
    if run_checker check-shell-message-names "$py" "$PROJECT_ROOT/scripts/check-shell-message-names.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  A shell diagnostic names a command it did" >&2
    echo "not come from." >&2
    echo "" >&2
    echo "Almost always this is a block copied between two cmd_ functions with" >&2
    echo "the donor's name still in it.  Fix the literal, not the dispatch arm:" >&2
    echo "the arm is what the user actually types, and is the authority here." >&2
    echo "" >&2
    echo "If a function legitimately has no dispatch arm, it is not a command" >&2
    echo "and should not be printing a command name -- pass the name down from" >&2
    echo "the caller that does have one." >&2
    exit 1
}

check_shell_message_names

# The article in a refusal is chosen at runtime by `article_for`, so it is not
# in any format string and `check-selftest-wording.py` cannot see it.  The one
# place it becomes visible is a self-test rung asserting the message -- i.e. at
# the end of an eleven-minute QEMU boot.  This moves it to the front.
check_shell_noun_article() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Shell noun-article check: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking the noun-article gate against its fixture ==="
    if ! run_checker check-shell-noun-article-selftest "$py" "$PROJECT_ROOT/scripts/check-shell-noun-article.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The noun-article gate no longer agrees" >&2
        echo "with its own fixture, so its verdict means nothing -- it reports a" >&2
        echo "clean tree and a collapsed parser in the same words." >&2
        exit 1
    fi

    echo "=== Checking that an operand noun states an article spelling cannot pick ==="
    if run_checker check-shell-noun-article "$py" "$PROJECT_ROOT/scripts/check-shell-noun-article.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  A refusal would print the wrong article." >&2
    echo "" >&2
    echo "\`article_for\` picks by spelling; English picks by sound. A noun that" >&2
    echo "starts with the \"yoo\" onset (user, uid, unit, URL) is spelled with a" >&2
    echo "vowel and sounds like a consonant, so the rule yields \"an user id\"." >&2
    echo "" >&2
    echo "Write the article into the noun -- \"a user id\" -- which \`article_for\`" >&2
    echo "prints verbatim. That is the escape hatch its own doc comment names." >&2
    exit 1
}

check_shell_noun_article

# A hand-written "every variant" list cannot go stale loudly.
#
# `const ALL: [Foo; N] = [...]` claims to name every variant of `Foo`, and the
# language has no way to check it: adding a variant leaves the array a valid
# array of N `Foo`s, just no longer a complete one.  Where such a list drives a
# test loop, the variant nobody added to it is the one case nothing asks about
# -- a branch that looks covered, which is worse than one that looks bare.
#
# An exhaustive `match` elsewhere does break the build, but it breaks it
# somewhere else; the author fixes `label()`, the compiler goes quiet, and the
# list is still the old length.
#
# The in-language fix is `assert!(ALL.len() == core::mem::variant_count::<Foo>())`
# and it is E0658 on stable.  If that ever stabilises, delete this gate and the
# script and write the assertion beside each list -- an error at the list beats
# a report about it.
#
# Requested by lane C in requests/c-a-wire-the-variant-list-gate-into-boot-test.md,
# which shipped the script scoped to gui/apps/net*/pkg and left widening to us.
# It is now tree-wide, which is also what exposed the one bug in it: matching a
# bare element type by name across crates paired `gui/keylayout`'s *struct*
# `Level` with `kernel/src/klog.rs`'s unrelated 5-variant enum of the same name.
# Resolution is scoped nearest-first now, and anything it cannot place is
# reported as a skip rather than dropped.
check_variant_lists() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Variant list check: skipped (no python) ===" >&2
        return 0
    fi

    # The counter is a heuristic over Rust source, and a miscounting heuristic
    # reports a clean tree exactly the way a clean tree does.  Its fixture runs
    # first so that collapse is a gate fault and not a pass.  This is not
    # hypothetical: the first version of the script reported `CursorShape` as
    # having 12 of its 13 variants, because the pass that collapses struct and
    # tuple variants ate `[default]` and left a bare `#` glued to `Arrow`.
    echo "=== Checking the variant list gate against its fixture ==="
    if ! run_checker check-variant-lists-selftest "$py" "$PROJECT_ROOT/scripts/check-variant-lists.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The variant list gate no longer" >&2
        echo "agrees with its own fixture, so its verdict on the tree means" >&2
        echo "nothing -- a counter that miscounts reports zero findings just" >&2
        echo "like a tree with none." >&2
        return 1
    fi

    echo "=== Checking that every ALL list still names every variant ==="
    if run_checker check-variant-lists "$py" "$PROJECT_ROOT/scripts/check-variant-lists.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  A list named ALL/ALL_*/EVERY_* is a" >&2
    echo "claim that it holds every variant of its enum, and one of them no" >&2
    echo "longer does." >&2
    echo "" >&2
    echo "If the variant belongs in the list: add it, and update the array" >&2
    echo "length beside it.  Check whether the loops that walk the list now" >&2
    echo "need a case for it -- that is the coverage this gate exists to" >&2
    echo "protect." >&2
    echo "" >&2
    echo "If the list is meant to be a subset: rename it to say so (the tree" >&2
    echo "uses ZONELESS, EXPENSE_CATS, FIXED) and put the reason in a doc" >&2
    echo "comment beside it.  A subset named ALL is the same defect wearing" >&2
    echo "the other hat." >&2
    return 1
}

if ! check_variant_lists; then
    exit 1
fi

# An app that keeps time but never receives the clock.
#
# A GUI app's clock is one event, `Event::Tick { elapsed_ms }`.  An app that
# ages anything -- a stopwatch, a metronome, a toast that expires, a WPM figure
# -- must route it to whatever advances that state.  If `handle_event` never
# names the variant, the event lands in the `_ =>` arm and the state is frozen
# for the life of the process, while the window still lays out, still repaints,
# still answers the keyboard, and still shows a number.
#
# `dead_code` cannot see this, because the advancing function *is* called -- by
# its own unit test, which passes the interval in by hand and passes.  Lane C
# found five of these and all five had green tests over frozen code.
#
# The gate is a heuristic over Rust source and its fixture runs first, for the
# usual reason: a gate that has stopped seeing reports zero findings in exactly
# the way a clean tree does.
#
# Requested by lane C in requests/c-a-wire-the-tick-gate-into-boot-test.md.
# Scope is lane C's own tree (gui, apps, net*, pkg), which is where the script
# was falsified; unlike the variant-list gate, this one is not widened, because
# `handle_event` and `Event::Tick` are matched by bare name and a same-named
# `handle_event` over some other lane's own event enum would be a false finding
# in a gate that refuses to build.  Widen it when something outside lane C
# grows a guitk window, and falsify it there first.
check_tick_wiring() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Tick wiring check: skipped (no python) ===" >&2
        return 0
    fi

    # Not a formality.  The first version of this script accepted a file's own
    # regression test as evidence of production wiring -- so every file it ever
    # caused to be fixed would have gone permanently blind to it.  It now blanks
    # comments and `#[cfg(test)]` items before searching, and the fixture pins
    # that behaviour.
    echo "=== Checking the tick wiring gate against its fixture ==="
    if ! run_checker check-tick-wiring-selftest "$py" "$PROJECT_ROOT/scripts/check-tick-wiring.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The tick wiring gate no longer agrees" >&2
        echo "with its own fixture, so its verdict on the tree means nothing --" >&2
        echo "a gate that has stopped seeing reports zero findings just like a" >&2
        echo "clean tree does." >&2
        return 1
    fi

    echo "=== Checking that apps which keep time receive the clock ==="
    if run_checker check-tick-wiring "$py" "$PROJECT_ROOT/scripts/check-tick-wiring.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  Each file above defines handle_event and a" >&2
    echo "function that takes a time interval, but never matches Event::Tick." >&2
    echo "Nothing in the running program advances that state: it is frozen for" >&2
    echo "the life of the process and shows a plausible zero." >&2
    echo "" >&2
    echo "The fix is a match arm in handle_event:" >&2
    echo "" >&2
    echo "    Event::Tick { elapsed_ms } => { self.tick(*elapsed_ms); ... }" >&2
    echo "" >&2
    echo "Note elapsed_ms is an INTERVAL since this window's previous tick, not" >&2
    echo "a timestamp -- see gui/window/src/lib.rs.  Then write the regression" >&2
    echo "test through handle_event, never against the advancing function: a" >&2
    echo "test that calls tick() directly cannot tell a wired app from an" >&2
    echo "unwired one, which is how all five of these shipped green." >&2
    return 1
}

if ! check_tick_wiring; then
    exit 1
fi

# ---------------------------------------------------------------------------
# Five lane-C gates that existed but were asked by nothing.
#
# Requested by lane B in
# requests/b-c-six-gui-gates-are-never-run-by-anything.md: a checker sitting in
# scripts/ looks like an enforced rule, and is only enforced if something calls
# it.  These five ran solely inside scripts/pre-boot.py, a local pre-flight
# nobody is obliged to run, so the rules they hold could be broken, merged and
# pushed with nothing objecting.
#
# All five were verified 0-or-1 before being wired: `run_checker` treats any
# other exit as "no verdict" and aborts the build, so a gate that can answer
# "I could not look" must not be here.  check-frame-needles.py has a `return 2`
# that is reachable only when an app named on its command line does not exist,
# which no call here can do.  check-generated-tables.py had one for a crashed
# generator and it is now `return 1` -- see that file, and lane B's request
# requests/b-c-check-generated-tables-returns-2-which-now-means-no-verdict.md.
#
# The sixth, check-evdev-elf-asm.py, stays unwired on purpose; its PINNED entry
# in scripts/check-gates-are-wired.py carries the reason.
#
# Measured cost, 2026-09-03: about 3.5 minutes for all five on the development
# machine, and it is dominated by *reading files* -- 80 ms per file, the same
# for read_text and read_bytes, and no faster on a second pass, which is a
# property of this filesystem rather than of the scripts.  Against a build that
# takes ten minutes and reads far more, and given a failure here saves that
# build, that is worth paying.  Do not "optimise" the scanners on the strength
# of a cProfile run: one was tried on 2026-09-03 and the profile's per-call
# overhead pointed at the wrong function entirely.  Measure with a clock.
check_lane_c_gui_gates() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Lane C GUI gates: skipped (no python) ===" >&2
        return 0
    fi

    # The shared lexer the wiring gates read Rust with. It had no test at all
    # until 2026-09-03, while four checkers decided what to report from what it
    # returns -- and a wrong answer there does not raise, it just makes those
    # gates see less. Same argument as the tick gate's fixture above.
    echo "=== Checking the Rust source scanner against its own cases ==="
    if ! run_checker rustscan-selftest "$py" "$PROJECT_ROOT/scripts/rustscan.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  scripts/rustscan.py no longer agrees" >&2
        echo "with its own cases.  Four gates read Rust through it, so their" >&2
        echo "verdicts on the tree mean nothing until it does -- a lexer that" >&2
        echo "has stopped seeing a construct reports zero findings exactly as" >&2
        echo "a clean tree does." >&2
        return 1
    fi

    # A gate that has stopped scanning reports zero findings exactly as a
    # clean tree does, so the window-wiring gate is checked against its own
    # fixture before its verdict on the tree is believed.
    if ! run_checker check-window-wiring-selftest "$py" "$PROJECT_ROOT/scripts/check-window-wiring.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The window-wiring gate no longer agrees with" >&2
        echo "its own fixture, so it can no longer be trusted to find" >&2
        echo "a program whose main never opens a window." >&2
        return 1
    fi

    echo "=== Checking that GUI programs open a window ==="
    if ! run_checker check-window-wiring "$py" "$PROJECT_ROOT/scripts/check-window-wiring.py"; then
        echo "" >&2
        echo "ERROR: refusing to build.  Each program above draws -- it builds" >&2
        echo "RenderTree or RenderCommand -- but its main never reaches" >&2
        echo "app::launch, so nothing it draws is ever put on a screen.  Its" >&2
        echo "tests can be green and its window has never opened." >&2
        return 1
    fi

    # A gate that has stopped scanning reports zero findings exactly as a
    # clean tree does, so the key-release gate is checked against its own
    # fixture before its verdict on the tree is believed.
    if ! run_checker check-key-release-wiring-selftest "$py" "$PROJECT_ROOT/scripts/check-key-release-wiring.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The key-release gate no longer agrees with" >&2
        echo "its own fixture, so it can no longer be trusted to find" >&2
        echo "a program that acts on a key going the wrong way." >&2
        return 1
    fi

    echo "=== Checking that a key coming up is not read as a second press ==="
    if ! run_checker check-key-release-wiring "$py" \
        "$PROJECT_ROOT/scripts/check-key-release-wiring.py"; then
        echo "" >&2
        echo "ERROR: refusing to build.  Each program above acts on a key event" >&2
        echo "without asking whether the key was going down or coming up, so" >&2
        echo "every keystroke is handled twice: one press moves the cursor two" >&2
        echo "cells, one Enter submits a form twice." >&2
        return 1
    fi

    echo "=== Checking that whole-frame needles are scoped to their band ==="
    if ! run_checker check-frame-needles "$py" "$PROJECT_ROOT/scripts/check-frame-needles.py"; then
        echo "" >&2
        echo "ERROR: refusing to build.  Each assertion above looks for a needle" >&2
        echo "over the whole frame when the needle is painted in more than one" >&2
        echo "place, so it passes on the wrong one and would keep passing if the" >&2
        echo "band it is about stopped being drawn (known-issues lesson 91)." >&2
        echo "Scope the assertion to the band's Rect." >&2
        return 1
    fi

    echo "=== Checking that generated tables match their generators ==="
    if ! run_checker check-generated-tables "$py" \
        "$PROJECT_ROOT/scripts/check-generated-tables.py"; then
        echo "" >&2
        echo "ERROR: refusing to build.  A checked-in generated table is not" >&2
        echo "what its generator produces, or a generator would not run at all." >&2
        echo "Re-run the generator named above and READ THE DIFF: a table that" >&2
        echo "changes when you did not mean to change one is the finding." >&2
        return 1
    fi

    # A gate that has stopped scanning reports zero findings exactly as a
    # clean tree does, so the diskcleanup-root gate is checked against its own
    # fixture before its verdict on the tree is believed.
    if ! run_checker check-diskcleanup-test-roots-selftest "$py" "$PROJECT_ROOT/scripts/check-diskcleanup-test-roots.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The diskcleanup-root gate no longer agrees with" >&2
        echo "its own fixture, so it can no longer be trusted to find" >&2
        echo "a test aimed at a real host path." >&2
        return 1
    fi

    echo "=== Checking that diskcleanup's tests do not point at the host ==="
    if ! run_checker check-diskcleanup-test-roots "$py" \
        "$PROJECT_ROOT/scripts/check-diskcleanup-test-roots.py"; then
        echo "" >&2
        echo "ERROR: refusing to build.  A test in apps/diskcleanup hands a real" >&2
        echo "host path to something that deletes.  Point it at a scratch" >&2
        echo "directory: this is the one test suite in the tree that can do" >&2
        echo "damage by running." >&2
        return 1
    fi

    # Four crates under apps/ carry an `-app` suffix because a crate of the
    # bare name already exists under userspace/.  `cargo ... -p <directory>`
    # therefore addresses a different crate, in a different lane -- and says
    # nothing about it.  On 2026-09-04 `cargo test -p sysinfo` reported
    # "0 passed; ok" about userspace/sysinfo while apps/sysinfo had 62 tests.
    if ! run_checker check-crate-names-selftest "$py"         "$PROJECT_ROOT/scripts/check-crate-names.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  scripts/check-crate-names.py no longer" >&2
        echo "agrees with its own cases, so its verdict on the tree means" >&2
        echo "nothing." >&2
        return 1
    fi

    if ! run_checker check-crate-names "$py"         "$PROJECT_ROOT/scripts/check-crate-names.py" apps userspace; then
        echo "" >&2
        echo "ERROR: refusing to build.  A crate's package name no longer" >&2
        echo "matches its directory, and is not recorded in that script's" >&2
        echo "KNOWN table.  Either record it with the reason it cannot be" >&2
        echo "renamed, or rename it -- an unrecorded mismatch means" >&2
        echo "  cargo ... -p <directory> silently addresses something else." >&2
        return 1
    fi

    # A SlateOS program target is `target_os = "linux"`, but every syscall in
    # `posix` is gated `#[cfg(target_os = "none")]` for the `libc.a` build.  So
    # a crate that lists `posix` as a *Rust dependency* compiles a second libc
    # into itself with every syscall stubbed to -ENOSYS, sitting beside the real
    # one it already links.  Nothing warns.  On 2026-09-04 that made `ssh` and
    # `sshd` unable to run at all: both drew key material through
    # `posix::random::fill`, which read -ENOSYS as "no kernel", fell through to
    # an RDRAND fallback the guest CPU does not have, and returned EIO while
    # blaming a kernel it had never asked.  See design-decisions.md section 768.
    if ! run_checker check-one-libc-selftest "$py"         "$PROJECT_ROOT/scripts/check-one-libc-per-process.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  scripts/check-one-libc-per-process.py" >&2
        echo "no longer agrees with its own cases -- including the ones that" >&2
        echo "prove it can still refuse -- so its verdict means nothing." >&2
        return 1
    fi

    if ! run_checker check-one-libc "$py"         "$PROJECT_ROOT/scripts/check-one-libc-per-process.py"; then
        echo "" >&2
        echo "ERROR: refusing to build.  Either a crate reached a stateful part" >&2
        echo "of posix as a Rust dependency -- which runs the stubbed second" >&2
        echo "copy, not the libc the program links -- or a module on that" >&2
        echo "script's PURE_MODULES allowlist has stopped being pure." >&2
        return 1
    fi

    return 0
}

if ! check_lane_c_gui_gates; then
    exit 1
fi

# Keep `.unwrap()` / `.expect()` out of kernel production paths.
#
# The count reached zero on 2026-08-22 and the script that measured it was
# documented as gateable ("exit status is 1 when it finds something") -- but it
# returned 0 unconditionally, and no caller existed to notice. A count at zero
# with nothing holding it there is a count on its way back up: the only reason
# it is cheap to keep at zero is that a regression is caught by the commit that
# introduces it, and that only happens if something checks.
check_production_unwrap() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Production unwrap/expect check: skipped (no python) ===" >&2
        return 0
    fi

    # Graded against a real kernel file first, not a fixture string.  Every way
    # this checker can break -- a test-scope detector that starts swallowing
    # production scope, a `?` suffix rule that widens, a comment stripper that
    # eats a line it should keep -- makes findings *disappear*, and it reports
    # zero findings in the same words whether it looked or not.  The self-test
    # picks a real `kernel/src/**.rs` with a production fn and a nested test fn,
    # plants the site in each, and asserts which one is reported: a synthetic
    # fixture would only prove the matcher works on input shaped the way I
    # imagined, not that it is still attached to the code it claims to grade.
    echo "=== Checking the production unwrap gate against a real kernel file ==="
    if ! run_checker scan-unwrap-selftest "$py" "$PROJECT_ROOT/scripts/scan-unwrap.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The production unwrap gate no longer" >&2
        echo "agrees with the kernel source it grades, so its verdict means" >&2
        echo "nothing -- every failure mode it has empties its own finding set," >&2
        echo "which it reports exactly the way it reports a clean tree." >&2
        exit 1
    fi

    echo "=== Checking for unwrap/expect in kernel production paths ==="
    if run_checker scan-unwrap "$py" "$PROJECT_ROOT/scripts/scan-unwrap.py" --summary; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  Each site above is an .unwrap()/.expect()" >&2
    echo "reachable from a kernel production path, where a panic takes the" >&2
    echo "machine down -- a denial of service for any of them an attacker can" >&2
    echo "steer.  Return the error with ?, or restructure so the failure cannot" >&2
    echo "be represented (design-decisions.md 282 lists the three shapes)." >&2
    echo "Test code is exempt and detected automatically; if a genuine test" >&2
    echo "site is being reported, its enclosing fn needs 'test' in its name." >&2
    exit 1
}

check_production_unwrap

# Refuse to build when a library module's entire public surface is named by
# nothing outside it.  Requested by lane C in
# requests/c-a-please-add-the-orphan-module-ratchet-to-the-pre-build-gate.md.
#
# WHY IT NEEDS A GATE.  `cargo build` cannot warn about an unused `pub` item,
# and a module's own unit tests keep the suite green, so an unreachable module
# is invisible to everything else in the build.  That is the same question
# check_self_tests_wired asks -- does anything actually reach this code? --
# asked one level up, at module rather than function scale.
#
# IT IS A RATCHET, NOT A CLEAN-TREE TEST.  47 modules are pinned in
# scripts/orphan-modules-baseline.txt and the gate is silent about every one of
# them: the existing pile is blocked on an operator decision (open-questions.md
# -> C-Q6, which decides whether the shell's settings pages survive at all), so
# it cannot be paid down today.  What --check refuses is a *newly* unreachable
# module, which keeps the pile from growing while that question sits.
#
# IT CANNOT FAIL ON LANE A'S OWN TREE.  Candidate modules are drawn from lane
# C's roots only (gui/**, apps/**, pkg/**, net*/**); kernel/**, posix/**,
# userspace/** and services/** are never reported.  It reads them, because a
# lane-C type used by lane B is used -- but it has no opinion about them.
#
# COST: ~39 s measured here, against a boot whose QEMU window alone is 400-900 s.
check_orphan_modules() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Orphan module check: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking that no NEW library module is unreachable ==="
    # Exit 2 means the check could not run -- a missing baseline, or a working
    # directory with no modules in it.  Treated as failure along with 1, and
    # deliberately so: the script spells "cannot fire" differently from
    # "passed" precisely so that a caller can refuse both.
    if run_checker scan-orphan-modules "$py" "$PROJECT_ROOT/scripts/scan-orphan-modules.py" --check; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  Each module above defines public items" >&2
    echo "that no other file in the repository names, so nothing reaches it:" >&2
    echo "cargo cannot warn about an unused pub item and its own tests still" >&2
    echo "pass, which is why this shape ships green." >&2
    echo "" >&2
    echo "Wire it up, delete it, or -- if it is an island on purpose, e.g. a" >&2
    echo "crate consumed outside this tree -- add it to" >&2
    echo "scripts/orphan-modules-baseline.txt in the same commit, with the" >&2
    echo "reason in the commit message." >&2
    exit 1
}

check_orphan_modules

# Refuse to build when any script in scripts/ has a shellcheck finding at
# severity `warning` or worse.
#
# WHY IT NEEDS A GATE.  `scripts/shellcheck-all.sh` was added by lane B on
# 2026-08-27 and, until this line, was referenced by nothing: no gate, no
# harness, no other script.  It ran only when someone remembered it.  It was
# also *unrunnable* under lane A's account -- shellcheck was never installed
# there, so the script exited 2 -- which means the tree it polices went
# unchecked in at least one of the three lanes for the whole of its existence.
# A gate nobody runs is not a gate; this makes it one.
#
# The shell is where this project's worst-behaved code lives, precisely because
# nothing type-checks it: `bash -n` catches syntax only, and the failures that
# actually bite are semantic and silent -- an unquoted expansion that
# word-splits a path containing a space, a `$?` read after the wrong command, a
# `cd` whose failure the next line ignores.  All three have happened here.  The
# unquoted-path one escaped the repository entirely and created a stray file
# named `D:\visual` on the operator's disk, which then wedged an unrelated lane.
#
# `warning`, RAISED FROM `error` ON 2026-08-29, AND STILL NOT A RATCHET.  The
# floor has always been the highest severity the tree already meets, so that
# this stays a clean-tree test with no baseline file to drift and can only ever
# fire on something newly introduced.  For two days that was `error`, because at
# `warning` the tree had 44 findings -- 37 of them SC2209 on `DIFF_PROG=<name>`
# in lane B's differential harnesses, where the tool cannot distinguish a bare
# command name from a forgotten `$(...)`.  Lane B cleared all 44 (quoting them
# rather than disabling the code, and fixing `diff-wsl.sh`'s copy-from template
# so the backlog stops regrowing by one per new harness), so the floor moves up.
#
# THIS IS THE SEVERITY THE GATE WAS BUILT FOR.  `error` catches shell that will
# not run; it does *not* catch an unquoted expansion that word-splits on a
# space -- that is SC2086, a `warning`, and it is the exact class that escaped
# the repository and created the stray `D:\visual` file described above.  Until
# today the gate could not have caught its own founding incident.
#
# ONE TRAP FOR WHOEVER EDITS A COMMENT IN scripts/ NEXT, learned by lane B the
# expensive way: a comment whose first word is `shellcheck` is parsed as a
# *directive*, not prose.  One such line in `diff-wsl.sh` -- which all 50
# harnesses source under `-x` -- failed to parse, and each dependant then
# silently lost its `-x` suppressions and reported SC1094.  The count went from
# 44 to 227 across 50 untouched files.  Keep the word off the start of a line.
#
# SKIPS, LOUDLY, WHEN THE TOOL IS ABSENT.  shellcheck is a third-party binary,
# and hard-failing the kernel build in a lane that has not installed one would
# be a worse outcome than the checking it buys.  Exit 2 from shellcheck-all.sh
# means "tool not found" and is distinct from exit 1, "findings"; only the
# latter stops the build.  Same shape as the `no python` escapes above.
#
# COST: ~40 s measured here, against a boot whose QEMU window alone is 400-900 s.
check_shellcheck() {
    echo "=== Checking scripts/ for shellcheck findings (floor: warning) ==="
    # `local out rc` is deliberately a separate statement from the assignment:
    # `local out="$(...)"` makes `local` the command whose status `$?` reports,
    # which is always 0, silently discarding the exit code this gate is built
    # around.  (That exact trap is one of the things shellcheck flags, SC2155.)
    local out rc
    # And the status must be captured with `&& rc=0 || rc=$?` rather than a bare
    # `rc=$?` on the next line, because this file runs under `set -e`: a plain
    # failing assignment would abort the entire boot test before `rc` was ever
    # read -- taking the *skip* path (exit 2, shellcheck not installed) down
    # with it, which is the opposite of what the skip is for.
    out="$(bash "$PROJECT_ROOT/scripts/shellcheck-all.sh" warning 2>&1)" && rc=0 || rc=$?
    if [ "$rc" -eq 0 ]; then
        printf '%s\n' "$out" | tail -1
        return 0
    fi
    if [ "$rc" -eq 2 ]; then
        # DD 630 decision 2, reversed 2026-08-29 now that its precondition holds.
        #
        # This arm used to `return 0`, because the tool was not yet installed
        # in every lane when the gate was written, and a hard failure would
        # have blocked lanes that had done nothing wrong.  (That sentence
        # cannot open with the tool's name: a comment whose first word after
        # `# ` is "shellcheck" is parsed as a *directive*, and an unparseable
        # one is itself an error -- SC1072/SC1073, which is what the first
        # draft of this comment tripped.)  That tolerance is
        # exactly the blindness the gate was written to end: a missing tool made
        # it report success, so the *entire* bug class -- including the unquoted
        # expansion that created a stray 'D:\visual' file -- was invisible in
        # precisely the situation where nobody was looking.  It is the first of
        # the five gates since found blind, and the only one whose blindness was
        # designed in on purpose.
        #
        # Verified before flipping it, rather than assumed: shellcheck is present
        # in all three lane worktrees, and `shellcheck-all.sh warning` exits 0
        # with zero findings over 79/80/79 scripts in each.  So no lane is
        # blocked by this, and a future exit 2 means the tool was *removed* --
        # which must stop the build, not be waved through.
        printf '%s\n' "$out" >&2
        echo "" >&2
        echo "ERROR: refusing to build.  shellcheck is not installed, so this" >&2
        echo "gate cannot see anything -- and a gate that cannot see reports a" >&2
        echo "clean tree in the same words as a clean tree." >&2
        echo "" >&2
        echo "Install it -- it is one static binary and needs no root:" >&2
        echo "      Windows: shellcheck-stable.zip from the koalaman/shellcheck" >&2
        echo "               releases; put shellcheck.exe in ~/bin (MSYS resolves" >&2
        echo "               the .exe suffix, so no rename is needed)." >&2
        echo "      Linux:   shellcheck-stable.linux.x86_64.tar.xz, same place." >&2
        exit 1
    fi

    printf '%s\n' "$out" >&2
    echo "" >&2
    echo "ERROR: refusing to build.  A script above has a shellcheck finding" >&2
    echo "at severity *warning* or worse." >&2
    echo "" >&2
    # Do NOT tell the reader the finding is theirs.  This message used to end
    # "the tree was at zero of these, so this one is newly introduced by the
    # change in hand", and on 2026-08-31 that sentence was simply false: the
    # two findings then failing lane A's boot test arrived from *another lane*
    # through a merge of `main`, and `main` itself was already red.  The reader
    # spent the run reading their own diff, which was clean, because the gate
    # had told them with total confidence where to look.
    #
    # A gate may not assert authorship it has not checked.  It sees a finding;
    # it does not see who wrote it.  So say what is known, and point at the one
    # command that settles it -- the merge-base comparison below distinguishes
    # "you introduced this" from "you inherited a red trunk", which are
    # different problems with different owners and different fixes.
    echo "This does NOT necessarily mean your change introduced it: a finding" >&2
    echo "can arrive from another lane through a merge of 'main', in which case" >&2
    echo "'main' is red and every lane's boot test is blocked, not just yours." >&2
    echo "Check which case you are in before reading your own diff:" >&2
    echo "    git fetch origin && git merge-base --is-ancestor origin/main HEAD" >&2
    echo "    git stash list  # then compare against origin/main:<script>" >&2
    echo "If the finding reproduces on origin/main, it is inherited -- repair it" >&2
    echo "(or file requests/<you>-<owner>-<slug>.md) and say so in the commit." >&2
    echo "" >&2
    echo "'warning' is not a style level: it is where the unquoted expansion" >&2
    echo "that word-splits on a space lives (SC2086), which is the class that" >&2
    echo "once created a stray file named 'D:\\visual' on the operator's disk." >&2
    echo "" >&2
    echo "Reproduce and read the findings with:" >&2
    echo "    bash scripts/shellcheck-all.sh warning --full" >&2
    echo "" >&2
    echo "Do not silence it with a blanket 'shellcheck disable' at the top of" >&2
    echo "the file.  If a finding is genuinely wrong, disable that one code on" >&2
    echo "that one line, with a comment saying why it is a false positive." >&2
    exit 1
}

check_shellcheck

# Run the tooling's own test suites.
#
# WHY THIS GATE EXISTS.  `scripts/` holds a `test-*.py` suite for most of the
# tooling -- the boot harness itself, the two history loaders, the canary
# loader, the sysroot fixtures, the space reclaimer, the source digest, the
# open-request report -- and until 2026-08-29 **nothing ran any of them**.  (No
# count here on purpose: the glob below is the list, and a number in a comment
# is a second list that goes stale in silence.  This one had, reading
# "fourteen" while sixteen sat on disk.)  They ran when an agent
# happened to remember, which for most of them was the day they were written.
# That is the same failure this file's other gates exist to prevent, one level
# up: a suite that is not run is not a test, it is a comment that takes an hour
# to write.  The specific hazard is worse here than for kernel code, because
# these scripts *are* the harness -- a regression in `boot-history.py` corrupts
# the record of every boot, including the ones that would have shown it.
#
# WHY IT IS AFFORDABLE.  Measured, not assumed: on 2026-08-29 the fourteen
# suites that existed then took ~95 s together (the slowest,
# `test-canary-load.py`, is 31 s), against a boot test that runs 900-1200 s.
# The figure is dated because it is a measurement; it is the ratio that is the
# argument, and a suite would have to be minutes long to change it.  It is also the cheapest possible place to spend it -- these fail
# in seconds and before the build, so a broken harness stops the run instead of
# corrupting its output an hour later.
#
# WHY IT DISCOVERS RATHER THAN LISTS.  A hand-written list is a second place a
# new suite must be registered, and forgetting is silent: the suite passes by
# not running, which is indistinguishable from passing.  So the glob is the
# list, and a floor below guards the failure mode discovery has instead -- a
# glob that matches nothing also reports no failures.
check_python_suites() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Tooling test suites: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Running the tooling's own test suites (scripts/test-*.py) ==="
    local suites=()
    local f
    for f in "$PROJECT_ROOT"/scripts/test-*.py; do
        # An unmatched glob expands to itself in bash, so the literal pattern
        # would be "run" as a filename and fail confusingly.  Test for the file.
        [ -f "$f" ] && suites+=("$f")
    done

    # The floor.  Fourteen suites existed when this gate was written; the check
    # is deliberately `-lt` against a number below that rather than `-eq`, so
    # adding a suite does not fail the build, while losing most of them -- a
    # broken glob, a renamed directory, a checkout that dropped `scripts/` --
    # does.  Without it this gate's own failure mode is a silent pass.
    if [ "${#suites[@]}" -lt 10 ]; then
        echo "" >&2
        echo "ERROR: refusing to build.  Only ${#suites[@]} tooling test suite(s)" >&2
        echo "were discovered under scripts/test-*.py; there are at least 10." >&2
        echo "Discovery is broken, not the code -- and a gate that discovers" >&2
        echo "nothing reports no failures, which reads exactly like a pass." >&2
        exit 1
    fi

    local failed=()
    local skipping=()
    local skipped_groups=0
    local out rc skips nskips skipline
    for f in "${suites[@]}"; do
        # `&& rc=0 || rc=$?` rather than a bare `rc=$?`: this file runs under
        # `set -e`, where a failing command in an assignment would abort the
        # whole boot test before the status could be reported as a suite
        # failure.  Same reasoning as check_shellcheck above.
        # PYTHONIOENCODING=:replace -- the empty field before the colon sets the
        # error handler and leaves the encoding alone.  Capturing with `$(...)`
        # hands the suite a pipe, and on Windows a pipe gets the *locale*
        # encoding (cp1252 here), not UTF-8.  A suite that prints a tick or an
        # em dash -- and several quote the markers lanes really write -- then
        # dies of UnicodeEncodeError partway through its report.  That is the
        # worst way for a gate to fail: exit 1 with a charmap traceback is
        # indistinguishable from a real test failure, and it destroys the
        # diagnosis at the one moment the diagnosis is what you came for.
        #
        # `:replace` rather than `utf-8` on purpose.  Forcing UTF-8 would send
        # UTF-8 bytes to whatever console the operator has, which for cp1252 is
        # mojibake; `:replace` keeps the console's own encoding and degrades the
        # tick to `?`, which is readable everywhere.  Set here rather than in
        # each suite because this line is what creates the pipe, and it covers
        # suites added later that have not thought about it.
        #
        # THE ONE CHECKER INVOCATION IN THIS FILE THAT DOES NOT GO THROUGH
        # `run_checker`, deliberately.  Both halves of the defect run_checker
        # exists for are already answered here: the *whole* output is printed on
        # failure (not a tail), so no evidence is lost, and the advice a failure
        # gives -- "reproduce with `python scripts/<suite>`" -- is the right next
        # step for a crashed suite as much as a failing one.  What run_checker
        # would cost is this loop's readable shape: it echoes everything a
        # checker prints, and forty-odd suites' full output in place of the
        # one-line-per-suite table below is a worse log, not a better one.
        out="$(PYTHONIOENCODING=:replace "$py" -u "$f" 2>&1)" && rc=0 || rc=$?
        if [ "$rc" -eq 0 ]; then
            printf '    %-32s %s\n' "$(basename "$f")" "$(printf '%s\n' "$out" | tail -1)"
            # A passing suite is reported by its LAST LINE ONLY, so a suite that
            # drops a group and still ends with "all N passed" reports a skip
            # that nothing above this line can see.  That is not hypothetical:
            # `test-bench-history.py` has seven skips that fire when the runs
            # they name age out of `bench/history.jsonl`, and
            # `test-rustemit.py`'s most valuable group vanishes if capstone is
            # not installed.  Each is correct to skip; none of them was visible.
            #
            # The per-suite convention -- fold the skip into the summary line --
            # works, and `test-rustemit.py` does exactly that.  It is not enough
            # on its own: it is a rule every future suite has to remember, and
            # the two suites above are the proof that it is not remembered.  So
            # the harness that created the last-line-only display is also the
            # thing that repairs it, and a new suite gets the behaviour without
            # having to know the rule exists.
            #
            # Matched on the FIRST token, not by searching for "skip" anywhere.
            # This project's tooling is largely *about* skips, so a substring
            # match flags `PASS  a boot that skipped nothing yields an empty
            # tuple` and every line the tools-under-test print about skipping a
            # malformed record -- surveyed 2026-09-03, and those false hits are
            # all of today's matches and none of them is a suite skip.  An
            # annotation that is usually noise gets skimmed, which would leave
            # the real skip exactly as hidden as it is now.
            skips="$(printf '%s\n' "$out" | grep -E '^[[:space:]]*SKIP(PED)?\b' || true)"
            if [ -n "$skips" ]; then
                nskips=$(printf '%s\n' "$skips" | wc -l)
                skipped_groups=$((skipped_groups + nskips))
                skipping+=("$(basename "$f")")
                # Indented under the suite's own line, so it reads as part of
                # that suite's report rather than as a separate finding.  Line
                # by line rather than one `printf '  ^ %s\n' "$skips"`: that
                # passes every skip as a *single* argument containing newlines,
                # so only the first would be marked and the rest would come out
                # flush left, reading as output from the harness itself.
                while IFS= read -r skipline; do
                    printf '        ^ %s\n' "$skipline"
                done <<< "$skips"
            fi
            continue
        fi
        failed+=("$(basename "$f")")
        echo "" >&2
        echo "--- $(basename "$f") FAILED (exit $rc) ---" >&2
        # The whole output, not a tail: these suites print one line per
        # assertion and the failing one is rarely last.
        printf '%s\n' "$out" >&2
    done

    if [ "${#failed[@]}" -eq 0 ]; then
        # The skip count rides on the section's closing line for the same reason
        # it rides on each suite's: this line is what a reader scanning a
        # 30,000-line boot log actually sees, and "all passed" with a silently
        # reduced N is the exact claim this gate exists to refuse.  It is a
        # count, not a failure -- every skip observed so far is legitimate (a
        # tool genuinely absent, a run genuinely aged out), and failing on one
        # would only teach the next author to phrase the skip differently.
        if [ "$skipped_groups" -gt 0 ]; then
            echo "=== Tooling test suites: ${#suites[@]} suites, all passed" \
                 "(${skipped_groups} group(s) SKIPPED in ${#skipping[@]}: ${skipping[*]}) ==="
            return 0
        fi
        echo "=== Tooling test suites: ${#suites[@]} suites, all passed, none skipped ==="
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  ${#failed[@]} tooling test suite(s) failed:" >&2
    printf '    %s\n' "${failed[@]}" >&2
    echo "" >&2
    echo "These test the harness itself, so a failure here means the numbers" >&2
    echo "this run would produce cannot be trusted -- including the boot" >&2
    echo "verdict and the history it gets recorded in.  Reproduce with:" >&2
    echo "    python scripts/${failed[0]}" >&2
    exit 1
}

check_python_suites

# Keep `design-decisions.md`'s per-lane numbering bands intact.
#
# WHY THIS IS A BUILD GATE AND NOT A LINT.  `design-decisions.md` is written by
# three lanes at once, and the bands are what let them do that: each lane
# inserts inside its own numeric range, so each lane's insertion point is a
# different line offset and git never has to compare two lanes' prose.  When
# that slips, git does not say so.  On 2026-08-27 lanes A and B both wrote a
# section 626; git reported a 350-line `CONFLICT (content)` naming neither the
# duplication nor the number, and it was caught only because lane A happened to
# grep afterwards.  Writing the checker found nine *more* live duplicates
# (268-276) that nobody had caught at all.  A convention that is enforced by
# remembering to grep is not enforced.
#
# WHY IT RUNS HERE, BEFORE THE BUILD.  It costs ~0.3 s and needs no toolchain,
# and the thing it protects is a document -- so failing before an hour of build
# and boot is the whole benefit.  It cannot fail on anything that was already
# in the tree: existing sections are grandfathered by
# `scripts/design-decisions-baseline.json`, so it fires only on something added
# after it landed.
#
# It exits 1 for "the document is wrong" and 2 for "the checker could not run"
# (missing baseline, unreadable file); both stop the build, but the message
# distinguishes them, because sending a reader to the document when the checker
# is what broke wastes the trip.
check_design_decisions_bands() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== design-decisions.md band check: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking design-decisions.md numbering bands ==="
    local rc
    run_checker check-design-decisions-bands "$py" -u \
        "$PROJECT_ROOT/scripts/check-design-decisions-bands.py" && rc=0 || rc=$?
    if [ "$rc" -eq 0 ]; then
        return 0
    fi

    # No `rc -eq 2` arm any more: this gate used to carry its own "a broken
    # checker, not a broken document" branch, which was the right instinct and
    # covered one exit code out of all the ways a checker can fail to reach a
    # verdict -- an uncaught exception exits *1*, so it landed in the accusing
    # branch below.  run_checker now makes that distinction for every gate in
    # the file, and never returns anything but 0 or 1, so the arm would be dead
    # code claiming a guarantee it no longer provides.
    echo "" >&2
    echo "ERROR: refusing to build.  design-decisions.md violates the" >&2
    echo "per-lane numbering bands.  Two lanes sharing a section number is" >&2
    echo "invisible to git, which is why this is a gate.  The rule is in" >&2
    echo "the file's own 'Numbering and file order' header; run" >&2
    echo "    python scripts/check-design-decisions-bands.py" >&2
    echo "for your lane's next number and the line to insert after." >&2
    exit 1
}

check_design_decisions_bands

# `open-questions.md` is the operator's decision queue, and its one structural
# rule -- open questions in the body, answered ones below `# Resolved` -- was
# broken eight times before anything checked it.  Three separate lanes filed a
# new question into the archive, because that is simply where the file ends,
# and an open question filed under `# Resolved` is invisible: the operator
# reads the queue from the top and never reaches it.
#
# Warnings rather than failures for two of the rules, which is the whole
# design: a missing `C-Q<n>` identifier and a duplicate number whose copies are
# all *archived* are both reported and neither stops the build.  A gate that
# hard-failed on another lane's heading text would be cross-lane breakage --
# lane A's build would refuse over a sentence only lane C may edit -- and
# rewriting an archived entry to satisfy a checker would falsify the record of
# what those numbers meant when they were answered.  See design-decisions.md
# §903.
check_open_questions() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== open-questions.md check: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking the open-questions gate against its fixtures ==="
    if ! run_checker check-open-questions-selftest "$py" -u \
        "$PROJECT_ROOT/scripts/check-open-questions.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The open-questions gate no longer" >&2
        echo "agrees with its own fixtures, so its verdict means nothing." >&2
        exit 1
    fi

    echo "=== Checking open-questions.md structure ==="
    if run_checker check-open-questions "$py" -u \
        "$PROJECT_ROOT/scripts/check-open-questions.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  open-questions.md is structurally wrong." >&2
    echo "" >&2
    echo "Almost always this is a new question appended to the end of the" >&2
    echo "file, which puts it below \`# Resolved\` among the answered ones." >&2
    echo "The operator reads the queue from the top, so a question filed" >&2
    echo "there is not a question that was asked -- move it up into the body." >&2
    echo "" >&2
    echo "The other two causes: a body entry whose \`Status:\` is no longer" >&2
    echo "OPEN (it has been answered, so it belongs in the archive index)," >&2
    echo "and two entries sharing one identifier while at least one is still" >&2
    echo "open (an answer naming that number could not be acted on)." >&2
    exit 1
}

check_open_questions

# Refuse to build when a self-test skip has fired on every recorded boot.
#
# This reads `bench/boot-history.jsonl`, so it is about the *previous* runs and
# not about the one starting now -- which is why it belongs here, before the
# build, rather than in the EXIT trap next to the recorder. Running it early
# also means the failure arrives in seconds instead of after a fifteen-minute
# build and boot.
#
# A skip that has never once *not* fired is a deleted test with a log line, and
# the suite above it still prints PASSED. Six of those were found by eye on
# 2026-08-31 (design-decisions.md sec 650); eye-finding does not scale to the
# seventh. See scripts/check-boot-skips.py for why the check is dynamic rather
# than static, and for the allowlist.
#
# Exit 2 (unreadable history) is treated as a hard stop for the same reason the
# band checker does: a checker that cannot run is not a clean tree, and the two
# must never produce the same output.
check_boot_skips() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Never-running self-test check: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking for self-tests that skip on every recorded boot ==="
    local rc
    run_checker check-boot-skips "$py" -u \
        "$PROJECT_ROOT/scripts/check-boot-skips.py" && rc=0 || rc=$?
    [ "$rc" -eq 0 ] && return 0

    # The `rc -eq 2` arm this used to carry is gone; run_checker makes that
    # distinction for every gate now and never returns 2.  See the note in
    # check_design_decisions_bands.
    echo "" >&2
    echo "ERROR: refusing to build.  A self-test has announced SKIP on" >&2
    echo "every recorded boot, which means its section has never run while" >&2
    echo "the suite above it reported PASSED.  The lines above name each" >&2
    echo "one and the three ways to resolve it; run" >&2
    echo "    python scripts/check-boot-skips.py --list" >&2
    echo "for the full per-skip standing." >&2
    exit 1
}

check_boot_skips

# Refuse to build when a *conditionally called* self-test has never once been
# observed to run.
#
# The sibling gate above reads `skips`, and a skip is a statement: the suite ran
# far enough to say so. A suite behind `if fat_ok` says nothing at all when the
# condition is false -- no SKIP line, no section name, a log region byte-
# identical to one where the suite was never written. `check-boot-skips.py`
# cannot see that class at all, which is why it gets its own gate rather than
# another branch of that one.
#
# The evidence is the `gated_ran` field, written by boot-history.py from the
# `RAN-IF` markers that check-self-tests-wired.py emits earlier in this script.
# So this is again about *previous* runs, and belongs before the build for the
# same two reasons: it is history, and it fails in seconds.
check_gated_selftests() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Never-ran gated self-test check: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking for gated self-tests that have never announced themselves ==="
    local rc
    run_checker check-gated-selftests "$py" -u \
        "$PROJECT_ROOT/scripts/check-gated-selftests.py" && rc=0 || rc=$?
    [ "$rc" -eq 0 ] && return 0

    # The `rc -eq 2` arm this used to carry is gone; run_checker makes that
    # distinction for every gate now and never returns 2.  See the note in
    # check_design_decisions_bands.
    echo "" >&2
    echo "ERROR: refusing to build.  A self-test that is called from inside" >&2
    echo "a conditional in kernel/src/main.rs has never once printed the" >&2
    echo "line it declares with RAN-IF, so it has never run -- while every" >&2
    echo "summary above it counts it as coverage.  The lines above name" >&2
    echo "each one and the three ways to resolve it; run" >&2
    echo "    python scripts/check-gated-selftests.py --list" >&2
    echo "for the full per-marker standing." >&2
    exit 1
}

check_gated_selftests

# Resolved once, here, because two things now need it: the clippy gate below
# and the build after it.  Hoisted out of the build block rather than
# duplicated -- a gate that resolves `cargo` differently from the build it
# guards could lint one toolchain and ship another.
CARGO="${CARGO:-cargo}"
# Try full path on Windows if cargo not in PATH
if ! command -v "$CARGO" &>/dev/null; then
    CARGO="/c/Users/${USER:-${USERNAME:-$(whoami)}}/.cargo/bin/cargo.exe"
fi

# Keep `cargo clippy -p kernel` exiting 0.
#
# The workspace sets `clippy::all = deny` and `clippy::pedantic = warn`, so the
# exit status is an exact question with no judgement in it: **zero `clippy::all`
# violations**.  The ~18,000 `pedantic` / `indexing_slicing` /
# `arithmetic_side_effects` warnings are the known backlog, are `warn`-level by
# deliberate workspace policy, and do not affect the status.
#
# WHY A GATE AND NOT A HABIT.  Eight `deny`-level errors accumulated in this
# crate unnoticed and were only found on 2026-08-24 by someone running clippy by
# hand for the first time in the project's life.  That is the whole failure mode:
# a crate can *declare* `deny` and still drift, because a declaration is only
# enforced by something that runs.  Fixing the eight without adding this would
# have bought exactly one clean day.
#
# WHY IT IS AFFORDABLE, MEASURED RATHER THAN ASSUMED.  This gate was deferred
# once already on the belief that "a clippy run and a `cargo check` run
# invalidate each other's fingerprints in a shared target/, so adding one
# doubles every boot's build time."  That is false, and it was reasoning rather
# than evidence.  `cargo clippy` sets `RUSTC_WORKSPACE_WRAPPER`, which is hashed
# into the fingerprint of every workspace unit, so clippy's artifacts occupy
# their own entries and leave the build's alone.  Measured on 2026-08-24:
#
#   cargo build (warm baseline)          13.8 s
#   cargo clippy -p kernel (cold)       200   s
#   cargo build immediately after         4.7 s   <-- not invalidated
#   cargo clippy again, no source edit    5   s
#   cargo clippy after touching one file 113   s   <-- what a real run pays
#
# 113 s against a boot test whose QEMU window alone is 400-900 s.  The number is
# not nothing, which is why it is written down here instead of being described
# as free.
#
# WHY `-p kernel` AND NOT THE WORKSPACE.  A workspace-wide clippy would let a
# red crate in lane B's or lane C's tree block lane A's boot test, which is the
# exact coupling the lane split exists to prevent.  Each lane gates its own.
#
# WHY THE SAME PROFILE AS THE BUILD.  `cfg(debug_assertions)` selects real code
# in this kernel, so linting debug while shipping release would leave a hole of
# precisely the size of the difference.  The gate checks what the run builds.
#
# `-p kernel` DOES COVER THE ROOT LEAF CRATES.  Worth stating outright, because
# the log below looks exactly as though it does not: every one of its ~18,000
# lines is a `kernel\...` path, and `crc32`, `deflate`, `sha2`, `netipc`,
# `netring`, `tzrules` and `ziparchive` contribute none.  That is cargo
# declining to *print* warnings for non-primary packages, not clippy declining
# to run on them.  Errors are not suppressed the same way.
#
# Verified 2026-08-26 by planting a deny-level `clippy::needless_return` in
# `ziparchive/src/lib.rs` and running this gate's exact command: it exited 101
# and named `ziparchive\src\lib.rs:112:5`.  Nothing else establishes this --
# a clean log is equally consistent with "linted and clean" and "never linted",
# and those two differ by every shared parser of untrusted input in the tree.
#
# Two corollaries, both of which cost an hour to learn:
#   - Adding `-p ziparchive` here changes nothing.  Tested with a cold cache;
#     the crate is a dependency of `kernel` in the same invocation, so it is
#     built in that role regardless of also being named.
#   - The absence of *warnings* from those crates is real but is not this
#     gate's business: the noisy pedantic lints (`cast_possible_truncation` and
#     friends) are `allow` at workspace scope, and these crates are small and
#     genuinely near-clean.  Their pedantic backlog is simply not visible here.
#     To see it, lint one on its own: `cargo clippy -p deflate`.
#
# Skipped under --no-build: that mode boots an already-built kernel, so there is
# no new source for the gate to have an opinion about, and 113 s buys nothing.
check_kernel_clippy() {
    if [ "$NO_BUILD" -ne 0 ]; then
        echo "=== Kernel clippy gate: skipped (--no-build; nothing new to lint) ==="
        return 0
    fi

    local log="$PROJECT_ROOT/build/clippy-kernel.log"
    mkdir -p "$PROJECT_ROOT/build"

    echo "=== Checking that the kernel is clippy-clean (clippy::all = deny) ==="

    # `attempt` exists because a crash caused by this host running out of commit
    # is a *host* condition that this script already knows how to wait out, and
    # throwing away the run instead would discard however many minutes of
    # dependency compilation clippy had already banked.  It is bounded to one
    # retry: a second crash is evidence of something other than a transient, and
    # a gate that retries indefinitely is a gate that never reports.
    local attempt=1
    local start free_mb warns
    while : ; do
        start="$(date +%s)"
        # Output to a file, never to this log.  18,000 warning lines would bury
        # the boot output that the rest of this script greps, and the full text
        # is worth keeping for whoever is working the pedantic backlog.
        #
        # Not a pipe: `cargo ... | grep` would make `$?` grep's, and grep's
        # status is "did I match", which for an error filter is *inverted* -- a
        # clean crate would report failure and a broken one success.
        if (cd "$PROJECT_ROOT" && "$CARGO" clippy -p kernel \
                ${CARGO_PROFILE_ARGS[@]+"${CARGO_PROFILE_ARGS[@]}"} \
                --message-format=short) > "$log" 2>&1; then
            warns="$(grep -c ' warning: ' "$log" 2>/dev/null || echo 0)"
            echo "Clippy OK ($BENCH_PROFILE profile, $(( $(date +%s) - start ))s, \
0 errors, $warns pedantic-level warnings -> $log)."
            return 0
        fi

        # A non-zero clippy is not automatically a lint finding.  `clippy-driver`
        # can *crash* -- on 2026-09-02 it died with STATUS_STACK_BUFFER_OVERRUN
        # (0xc0000409) while the host was at 3.1 GiB of free commit under another
        # lane's build -- and a crash exits non-zero too.  Reading that as "at
        # least one clippy::all violation" is the same error `boot-history.py`
        # used to make about exit 127: a verdict that rests on the *absence* of
        # findings cannot be asserted when the tool that would have found them
        # never finished.  The message below would have printed "Sites:" and then
        # nothing, which is the shape of a gate accusing the tree of something it
        # did not observe.
        #
        # The discriminator is cargo's own: a genuine lint failure ends with
        # "could not compile ... due to N previous errors" and no "Caused by".  A
        # crashed subprocess produces "process didn't exit successfully" carrying
        # a signal or an NTSTATUS, which cargo only prints when the child died
        # rather than reported.  Matching on that, rather than on the specific
        # status code, keeps this correct for a SIGSEGV on a POSIX host as well.
        if grep -qE "process didn't exit successfully|internal compiler error" "$log"; then
            # Is memory the explanation?  Ask now rather than assume.  Unlike the
            # pre-boot headroom gate -- which reads the host ~38 minutes before
            # QEMU needs it, and so is explicitly *not* a prediction anyone
            # should make at t=0 -- this reading is taken at the moment of the
            # failure it is trying to explain.  It is a measurement of a current
            # need, not a forecast of a later one.
            free_mb=""
            if [ "$attempt" -eq 1 ] && [ "${MIN_COMMIT_FREE_MB:-0}" -gt 0 ] 2>/dev/null \
               && free_mb="$(measure_commit_free_mb)" \
               && [ "$free_mb" -lt "$MIN_COMMIT_FREE_MB" ]; then
                echo "" >&2
                echo "NOTE: clippy-driver crashed with only ${free_mb} MiB of commit free" >&2
                echo "      (floor ${MIN_COMMIT_FREE_MB} MiB), so memory explains it.  Waiting for" >&2
                echo "      headroom and running the gate once more rather than discarding" >&2
                echo "      the dependency build that already succeeded." >&2
                echo "" >&2
                # Exits 5 if the host never recovers.  That is the right status:
                # the run died of host load having produced no verdict about the
                # tree, which is exactly what 5 means, and it names the real
                # cause more precisely than 6 would.
                check_commit_headroom "after clippy-driver crashed, before retrying"
                attempt=2
                continue
            fi

            echo "" >&2
            echo "ERROR: clippy-driver crashed instead of reporting a verdict." >&2
            echo "" >&2
            grep -E "process didn't exit successfully|internal compiler error" "$log" \
                | sed -E 's/^(.{0,200}).*/  \1/' >&2
            echo "" >&2
            echo "THIS SAYS NOTHING ABOUT THE TREE.  The gate ran and produced no" >&2
            echo "judgement; it did not produce a clean one.  Do not read a crashed" >&2
            echo "linter as a clean linter, and do not #[allow] anything on the" >&2
            echo "strength of it." >&2
            echo "" >&2
            # Say which of the two cases this is, because they call for opposite
            # responses: "the host was busy" means wait, whereas "it crashed with
            # memory to spare, twice" means investigate the toolchain.  A single
            # message covering both would send the reader to wait out a condition
            # that is not there.
            if [ "$attempt" -gt 1 ]; then
                echo "It crashed TWICE, the second time after commit headroom had" >&2
                echo "recovered above the ${MIN_COMMIT_FREE_MB} MiB floor.  Host memory does not" >&2
                echo "explain this one; suspect the toolchain or a genuine ICE, and" >&2
                echo "do not simply re-run expecting a different answer." >&2
            elif [ -n "$free_mb" ]; then
                echo "Commit headroom was ${free_mb} MiB at the moment it died, at or above" >&2
                echo "the ${MIN_COMMIT_FREE_MB} MiB floor, so this host's memory does not explain" >&2
                echo "it.  Suspect the toolchain or a genuine ICE." >&2
            else
                echo "Commit headroom could not be read, so whether memory explains" >&2
                echo "this is unknown.  On this host it usually does: at the Windows" >&2
                echo "commit limit a compiler dies wherever it happens to be, and" >&2
                echo "another lane's cargo build is normally what got us there." >&2
                echo "Wait for it and re-run -- see check_commit_headroom and exit 5." >&2
            fi
            echo "" >&2
            echo "Full output: $log" >&2
            exit 6
        fi

        echo "" >&2
        echo "ERROR: refusing to build.  cargo clippy -p kernel exited non-zero," >&2
        echo "which under this workspace's lint policy means at least one" >&2
        echo "clippy::all violation -- those are deny-level.  Sites:" >&2
        echo "" >&2
        grep ' error: ' "$log" >&2 || true
        echo "" >&2
        echo "Full output (including the pedantic-level backlog, which is NOT what" >&2
        echo "failed this gate): $log" >&2
        echo "" >&2
        echo "Fix them rather than #[allow] them.  Every one of the eight found on" >&2
        echo "2026-08-24 was a case clippy was right about, and seven were the" >&2
        echo "one-line rewrite clippy dictated verbatim.  If a lint genuinely does" >&2
        echo "not apply here, the allow goes at the narrowest possible scope with a" >&2
        echo "comment saying why -- per CLAUDE.md, not at workspace scope." >&2
        exit 1
    done
}

# Pin the teardown shape of the `/proc` self-test tables.
#
# Requested by lane B in
# `requests/b-a-check-selftest-reinit-is-never-run-by-anything.md`, and answered
# in `requests/a-b-wiring-check-selftest-reinit-and-a-correction-it-runs-nowhere.md`.
#
# Lane B's request said this gate ran "only inside scripts/pre-boot.py". It ran
# nowhere at all -- nothing in `scripts/` on either branch named it. That is
# worse than an unrun gate, because something was relying on it:
# `design-decisions.md` §612 states the liability in plain terms ("a future
# reader may reasonably think diagnostics should not run during boot and remove
# the `fs::*::self_test()` calls, silently switching 146 /proc tables back off
# -- with no error and no failing test, because a table that refuses writes and
# a table with no writers print the same zeros") and names two mitigations
# against it. The sibling, `check-self-tests-wired.py`, is wired. This one had
# never executed, so the record claimed two mitigations and the tree had one.
#
# Placed here, after every cheap document gate, because it is slow -- ~60-100 s,
# and essentially all of it is I/O: it opens 805 `.rs` files on a host that
# costs ~77 ms per file open regardless of cache state (profiled 2026-09-03:
# 59.2 s of 60.6 s in `read()`, 0.46 s in all the regex work combined). A typo
# in `design-decisions.md` still fails in under a second, ahead of this.
check_selftest_reinit() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== self-test reinit check: skipped (no python) ===" >&2
        return 0
    fi

    echo "=== Checking the selftest-reinit gate against its fixtures ==="
    if ! run_checker check-selftest-reinit-selftest "$py" -u \
        "$PROJECT_ROOT/scripts/check-selftest-reinit.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The selftest-reinit gate no longer" >&2
        echo "agrees with its own fixtures, so its verdict means nothing." >&2
        exit 1
    fi

    echo "=== Checking /proc self-test tables are re-opened after clearing ==="
    if run_checker check-selftest-reinit "$py" -u \
        "$PROJECT_ROOT/scripts/check-selftest-reinit.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  A self_test clears a \`Mutex<Option<_>>\`" >&2
    echo "table and does not call \`init_defaults()\` afterwards, so the table" >&2
    echo "stays switched off for the rest of the boot." >&2
    echo "" >&2
    echo "This does not fail a test and does not print an error: a /proc table" >&2
    echo "that refuses writes and one with no writers both print zeros.  That" >&2
    echo "is why it is a gate and not a test.  See design-decisions.md §612." >&2
    exit 1
}

check_selftest_reinit

check_kernel_clippy

# Compile every `#[cfg(unix)]` arm in the workspace, which nothing else does.
#
# Requested by lane B in
# requests/b-a-a-windows-only-check-never-compiles-your-cfg-unix-arms.md.
#
# WHY THIS IS NOT REDUNDANT WITH EVERY OTHER CHECK IN THIS FILE.  We all
# develop on a Windows host, so `cargo build`, `cargo clippy` and `cargo test`
# all run for a Windows target.  rustc does not *compile* what `#[cfg(unix)]`
# guards on a Windows target -- it discards the tokens -- so that code can
# contain plain name-resolution and syntax errors while every routine check
# comes back green.  SlateOS is a unix (`toolchain/x86_64-slateos.json` sets
# `"target-family": ["unix"]`), so the arm that is never compiled is the arm
# that ships.  Lane B's `userspace/backup` did not compile for a unix target
# for nearly three months on exactly this.
#
# IT IS SELF-INFLICTING, WHICH IS WHY IT BELONGS NEXT TO THE CLIPPY GATE.  The
# commit that broke `backup` was a clippy hygiene sweep: it answered a genuine
# "unused variable" warning on a `ManifestEntry::Symlink { target, path }` by
# rebinding to `target: _`.  On Windows that warning is correct, because the
# only reader of `target` is the `cfg(unix)` arm below it; on unix the rebind
# is a hard error.  Any `-D warnings` sweep over a file containing `cfg(unix)`
# is a fresh chance to do this again -- the warnings such a sweep is chasing
# exist *because* the unix arm is invisible to it.
#
# `x86_64-unknown-linux-gnu`, NOT `x86_64-slateos`: the latter needs
# `-Zbuild-std` and is far slower, and for `cfg(unix)` coverage the two are
# equivalent.  `clippy` rather than `build` means nothing is linked, so no
# cross-linker is needed on a Windows host.
#
# WHY `clippy` AND NOT `check`.  Several crates here declare
# `#![deny(clippy::all)]`, which makes a lint a hard error *on the target that
# ships*.  Nothing outside clippy reads that attribute, so a `cargo check` of a
# `cfg(unix)` arm containing a denied lint exits 0 -- and this gate then prints
# "every cfg(unix) arm compiles", which is true, and which every reader takes
# to mean the arms are checked.  That is the failure this gate exists to
# prevent, one level up: half a gate that reads like a whole one.  Requested
# with a full in-situ measurement in
# requests/b-a-cfg-unix-gate-should-lint-as-well-as-compile.md; taken in
# design-decisions.md §678, which also records the risk accepted.
#
# COST, measured 2026-09-02 in os-lane-a (all three exit 0):
#
#     cargo clippy, cold  793 s   |   cargo check, cold  336 s
#     cargo clippy, warm  156 s   |   in-situ `check` historically 9-416 s
#
# Lane B measured `clippy` in situ in a full boot test at 236 s.  So the
# steady-state price of the verb is roughly +2 min per boot test, plus a
# one-time ~13 min in each lane's worktree the first time it runs.
#
# The two columns do not share artifacts and never will: clippy sets
# `RUSTC_WORKSPACE_WRAPPER`, which is hashed into every workspace unit's
# fingerprint, so a clippy run neither reuses nor invalidates a check run --
# it maintains a parallel set.  That is why the cold column is not a
# first-run artifact that goes away, and why the numbers above were taken
# with clippy running twice *before* check: check still had nothing to reuse.
#
# WHY `--all-targets`, AND WHY `--exclude kernel` COMES WITH IT.  Without
# `--all-targets` cargo builds each crate's lib and bin and nothing else, so
# this gate never compiled a single `#[cfg(test)]` module -- and `#[cfg(unix)]`
# is *concentrated* in test code, because production code here is mostly
# written against `std` while the tests are full of `set_permissions(0o4741)`,
# `symlink`, `nlink`, `chown` and xattr fixtures that exist on unix and nowhere
# else.  In `userspace/coreutils` most `#[cfg(unix)]` items in `src/bin/*.rs`
# are inside `mod tests`.  So the gate compiled the smaller half of the thing it
# was built for and printed OK.
#
# Lane B demonstrated the gap rather than arguing it: the same command with
# `--all-targets` against `-p coreutils` found four hard compile errors in
# `userspace/coreutils/src/bin/cp.rs` that had been in the tree for weeks, all
# four inside `#[cfg(unix)] #[test]` helpers, all four missing imports.  Three
# were found by eye first and believed to be all of them; the fourth was eighty
# lines further down under a different name and only the compiler found it.
#
# `--exclude kernel` is not a scope reduction, it is what makes `--all-targets`
# build at all.  `--all-targets` adds each crate's `test` target; a `test`
# target links the harness, which pulls `std`, which already defines
# `panic_impl` -- so a `#![no_std]` binary supplying its own `#[panic_handler]`
# cannot have a `test` target on a hosted triple:
#
#     error[E0152]: found duplicate lang item `panic_impl`
#         --> kernel/src/main.rs:7963:1
#
# That is structural, not a lint.  `kernel` is the only crate in the workspace
# it hits: seven crates define a `#[panic_handler]`, but the other six are the
# `services/*` binaries, which the workspace's own `exclude` list (root
# Cargo.toml ~183) already keeps out.  And nothing is lost by excluding it --
# the kernel is `no_std`, so it has no `cfg(unix)` arms for this gate to check.
#
# The failure mode of `--exclude` is the good one: if a second bare-metal
# binary is ever added *inside* the workspace, this breaks loudly on the next
# run instead of quietly skipping it.  Naming the hosted crates positively with
# `-p` would under-cover in silence instead.
#
# COST, measured by lane B on 2026-09-03 on this workspace, not extrapolated:
# 508 s of one-time compilation on a cache that already held every crate's lib
# and bin, and **zero** new deny-level findings across all three lanes
# (0 errors, 1,857 warnings, which stay warnings).  Steady state after a
# one-line fix in the crate with the most test code in the tree is 46 s.  The
# full `clippy --workspace --exclude kernel --all-targets` pass measured 1,513 s
# cold, but `check` and `clippy` invalidate each other's fingerprints in a
# shared target/, so every run of this gate already pays a rebuild; what
# `--all-targets` adds on top is the test targets, i.e. the 508 s.
#
# Taken in both places at once rather than staged through pre-boot.py first,
# because the risk a staged rollout would have been protecting against -- new
# denials from three lanes' unseen test code -- was measured at zero, and lane B
# left the workspace green under this exact command.  See design-decisions.md
# §904.
check_cfg_unix() {
    if ! rustup target list --installed 2>/dev/null \
        | grep -qx "x86_64-unknown-linux-gnu"; then
        echo "=== cfg(unix) check: skipped (x86_64-unknown-linux-gnu not installed) ===" >&2
        echo "    rustup target add x86_64-unknown-linux-gnu" >&2
        return 0
    fi

    echo "=== Checking that every #[cfg(unix)] arm compiles and lints ==="
    local log start rc
    log="$PROJECT_ROOT/build/check-cfg-unix.log"
    start="$(date +%s)"
    # Same `&& rc=0 || rc=$?` reasoning as check_shellcheck: this file runs
    # under `set -e`, so a bare `if ! cargo ...` is fine but a plain command
    # whose status we want to read is not.
    # `--all-targets --exclude kernel`, requested with measurements in
    # requests/b-a-the-cfg-unix-gate-skips-every-test-module.md.  See the
    # WHY --all-targets block above the function.
    "$CARGO" clippy --workspace --exclude kernel --all-targets \
        --target x86_64-unknown-linux-gnu \
        --message-format=short > "$log" 2>&1 && rc=0 || rc=$?
    if [ "$rc" -eq 0 ]; then
        echo "cfg(unix) OK ($(( $(date +%s) - start ))s, every cfg(unix) arm compiles and lints)."
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  Code guarded by #[cfg(unix)] does not" >&2
    echo "compile, or does not lint, for a unix target.  This is invisible to" >&2
    echo "every other check here, because they all run for the Windows host --" >&2
    echo "and it is the arm that ships, because SlateOS sets" >&2
    echo "target-family = [\"unix\"]." >&2
    echo "" >&2
    grep -E '^[^ ].*: error' "$log" >&2 || true
    echo "" >&2
    echo "Full output: $log" >&2
    echo "" >&2
    echo "HOW TO READ THAT LOG.  A good run of this gate prints on the order of" >&2
    echo "18,000 pedantic-level warnings and still exits 0.  Warnings are NOT" >&2
    echo "why this failed -- clippy exits non-zero only at deny level, so do not" >&2
    echo "go hunting through them.  Only the lines above, which say \"error\"," >&2
    echo "matter, and they come in two kinds:" >&2
    echo "" >&2
    echo "  error[E0433]: ...   a compile failure.  The cfg(unix) arm is broken" >&2
    echo "                      Rust -- a name that does not resolve, a type that" >&2
    echo "                      does not exist on unix, a syntax error." >&2
    echo "  error: <lint text>  a clippy denial, with no bracketed code.  The" >&2
    echo "                      code compiles; the crate says #![deny(clippy::all)]" >&2
    echo "                      and this lint fired inside the cfg(unix) arm.  It" >&2
    echo "                      is fatal on the shipping target for that reason" >&2
    echo "                      and for no other." >&2
    echo "" >&2
    echo "If you arrived here from a warning-cleanup sweep, suspect the sweep:" >&2
    echo "an \"unused variable\" that is real on Windows is often read only by" >&2
    echo "the cfg(unix) arm the host target discarded." >&2
    exit 1
}

check_cfg_unix

# Step 1: Build
if [ "$NO_BUILD" -eq 0 ]; then
    check_free_space "before building"
    # Before the build, not only before QEMU.  A build started against an
    # exhausted commit limit is the thing most likely to *fail* to fork -- rustc
    # spawns dozens of processes -- and it is also twenty minutes we would spend
    # before discovering the boot cannot run either.
    check_commit_headroom "before building"
    echo "=== Building kernel ==="
    # Timed, and recorded in bench/boot-history.jsonl alongside the QEMU window.
    #
    # WHY THIS MATTERS BEYOND CURIOSITY.  open-questions.md Q46 asks whether the
    # non-bench boot test should build release rather than debug, and its whole
    # tradeoff is "slower build, faster boot".  We have always measured the boot
    # half precisely -- wall_seconds and marker_seconds, hundreds of records --
    # and the build half not at all, so one side of that comparison was an
    # assertion and the other was evidence.  A cost claim nobody measures is a
    # cost claim that cannot be checked, and it had gone unmeasured for the
    # entire life of the question.
    #
    # `date +%s` rather than SECONDS: SECONDS counts since the shell started,
    # which includes the prerequisite and free-space checks, and those are not
    # part of what a profile choice makes slower.
    BUILD_START_EPOCH="$(date +%s)"
    (cd "$PROJECT_ROOT" && "$CARGO" build ${CARGO_PROFILE_ARGS[@]+"${CARGO_PROFILE_ARGS[@]}"})
    BUILD_SECONDS=$(( $(date +%s) - BUILD_START_EPOCH ))
    # Said out loud as well as recorded: a lane deciding Q46 by feel should see
    # the number on the run in front of them, not only in the history file.
    echo "Build OK ($BENCH_PROFILE profile, ${BUILD_SECONDS}s)."
fi

if [ "$NO_STAGE" -eq 0 ] && [ ! -f "$KERNEL_BIN" ]; then
    echo "ERROR: Kernel binary not found at $KERNEL_BIN" >&2
    exit 1
fi

# Step 2: Stage boot files
#
# Checked a second time, and not redundantly: staging copies a ~200 MiB kernel
# image into build/esp, and the failure this whole floor exists to prevent is a
# *partial* write leaving a stale-or-truncated image that a later --no-build run
# boots as if it were current.  The pre-build check cannot cover this, because
# the build itself is what consumes the margin.  This also covers --no-build and
# --no-stage callers, which skip the first check entirely.
check_free_space "before staging"
echo "=== Staging boot files ==="
mkdir -p "$ESP_DIR/EFI/BOOT" "$ESP_DIR/boot"
cp "$PROJECT_ROOT/limine/BOOTX64.EFI" "$ESP_DIR/EFI/BOOT/BOOTX64.EFI"

# Strip DWARF, but keep .symtab — the unstripped debug binary is ~215 MiB.
# The staged image is still large because it carries a big .rodata payload:
# ~47 fastpy self-test ELFs are embedded into the kernel via include_bytes!
# (~3.5 MiB each → ~165 MiB of .rodata that stripping CANNOT remove — it's
# genuine program data).  Limine must load that whole image into high memory,
# so the QEMU RAM below (-m) has to comfortably exceed the staged kernel size
# (see the "-m" note near the QEMU invocation).  We try llvm-strip (ships with
# rustup) first, falling back to a plain copy if no strip tool is found.
#
# --strip-debug, NOT a bare strip.  A bare `llvm-strip` removes .symtab along
# with the .debug_* sections, and .symtab is the only thing `ksyms` reads: it
# parses the kernel file Limine hands it and builds the address→name table
# that symbolises every panic backtrace.  With the symbol table gone, ksyms
# printed "No symbol table found in kernel ELF" on every boot and every
# backtrace in the serial log — the primary diagnostic this project has, since
# a panic is how a boot test reports a bug — degraded to a column of raw
# addresses that can only be resolved offline, against a build that has
# usually already been overwritten by the next one.  It also silently defeats
# lockdep's violation reports, which name the two inverted locks by address on
# the explicit assumption that the address resolves to its owning symbol.
#
# The flag costs ~15 MiB of staged image (43 → 58 MiB) and still removes ~73%
# of the binary.  Against the 5 GiB of guest RAM the image is loaded into,
# that is not a tradeoff.
#
# NOTE (tech debt, tracked in known-issues.md): embedding every fastpy self-test
# ELF into the kernel .rodata makes the image grow ~3.5 MiB per new self-test.
# The proper long-term fix is to load these test binaries from the ESP/disk at
# boot instead of baking them into the kernel image.
LLVM_STRIP=""
for candidate in \
    "$HOME/.rustup/toolchains/stable-x86_64-pc-windows-gnu/lib/rustlib/x86_64-pc-windows-gnu/bin/llvm-strip.exe" \
    "$(rustup which llvm-strip 2>/dev/null)" \
    "llvm-strip" \
    "strip"; do
    if [ -n "$candidate" ] && command -v "$candidate" &>/dev/null || [ -f "$candidate" ]; then
        LLVM_STRIP="$candidate"
        break
    fi
done

# Stage the kernel.  A strip failure (e.g. the staged image is locked by a
# stray QEMU still holding the disk open → "Permission denied") MUST NOT be
# ignored: if it is, the boot test silently re-runs the previously-staged
# (stale) kernel and reports misleading results.  So we check the exit code,
# fall back to a plain copy, and abort the whole run if staging can't update
# the image.
STAGED_KERNEL="$ESP_DIR/boot/kernel"
stage_ok=0
if [ "$NO_STAGE" -eq 1 ]; then
    # Deliberately reuse the existing image (see --no-stage).  The freshness
    # guard below is inverted here: staleness relative to target/ is the *point*,
    # so we only check the image exists at all.
    if [ ! -f "$STAGED_KERNEL" ]; then
        echo "ERROR: --no-stage given but no staged kernel at $STAGED_KERNEL." >&2
        echo "       Run once without --no-stage first to populate the ESP." >&2
        exit 1
    fi
    echo "Reusing staged kernel (--no-stage): $(stat -c %y "$STAGED_KERNEL" 2>/dev/null || echo "$STAGED_KERNEL")"
    stage_ok=1
elif [ -n "$LLVM_STRIP" ]; then
    echo "Stripping kernel binary with $LLVM_STRIP --strip-debug (keeps .symtab for ksyms)..."
    if "$LLVM_STRIP" --strip-debug "$KERNEL_BIN" -o "$STAGED_KERNEL"; then
        stage_ok=1
    else
        echo "WARNING: strip failed; falling back to an unstripped copy." >&2
    fi
fi
if [ "$stage_ok" -eq 0 ] && [ "$NO_STAGE" -eq 0 ]; then
    if cp "$KERNEL_BIN" "$STAGED_KERNEL"; then
        stage_ok=1
    fi
fi
if [ "$stage_ok" -eq 0 ]; then
    echo "ERROR: could not stage kernel to $STAGED_KERNEL." >&2
    echo "       The image is likely locked by a stray qemu-system-x86_64" >&2
    echo "       process holding the disk open.  Kill it and re-run." >&2
    exit 1
fi
# Guard against a staged image that predates this build: it must be newer
# than the freshly-built kernel binary we just compiled.  Skipped under
# --no-stage, where an older image is exactly what was asked for.
if [ "$NO_STAGE" -eq 0 ] && [ "$STAGED_KERNEL" -ot "$KERNEL_BIN" ]; then
    echo "ERROR: staged kernel is older than the build output — staging did" >&2
    echo "       not take effect (stale image).  Aborting to avoid a" >&2
    echo "       misleading boot test." >&2
    exit 1
fi

cp "$PROJECT_ROOT/limine.conf" "$ESP_DIR/limine.conf"

# Kernel cmdline injection (a Limine `cmdline:` line on the single boot entry;
# indented so it associates with that entry).
#
# Always passed: `sched.boot_deadline_ms`, this harness's own boot timeout. The
# kernel's boot-window liveness watchdog derives its wall-clock deadline from it
# (see LIVENESS_BOOT_DEADLINE_NS in kernel/src/sched/mod.rs) instead of using a
# hardcoded constant, so it always dumps the task table shortly *before* we kill
# QEMU, and raising --timeout for a slower host or a bigger self-test battery
# moves both in lockstep. A hardcoded kernel-side constant drifted out of sync
# exactly this way and false-fired on every healthy boot (known-issues
# BUG-LIVENESS-DEADLINE-FALSE-FIRE).
#
# Optionally appended: the contents of SLATE_CMDLINE, for extra boot parameters
# (parsed by fs::kernparam) — e.g. to arm the B-KNULLJUMP corruption hunt under
# the soak harness:
#     SLATE_CMDLINE="mm.corruption_hunt=1" ./scripts/boot-test.sh
KERNEL_CMDLINE="sched.boot_deadline_ms=$((TIMEOUT * 1000))"
if [ -n "${SLATE_CMDLINE:-}" ]; then
    KERNEL_CMDLINE="$KERNEL_CMDLINE $SLATE_CMDLINE"
fi
printf '    cmdline: %s\n' "$KERNEL_CMDLINE" >> "$ESP_DIR/limine.conf"
echo "=== Kernel cmdline: $KERNEL_CMDLINE ==="

# Step 2b: Build the real USB image, if asked for.
#
# Built AFTER staging and unconditionally (even under --no-stage): the image is
# a pure function of $ESP_DIR, so rebuilding it is how the image tracks the
# staged tree.  Skipping it when the tree looks unchanged would reintroduce
# exactly the stale-image failure the staging freshness guard above exists to
# prevent -- and the build is a few seconds on a 40 MiB kernel.
ESP_DRIVE_ARGS=(-drive "format=raw,file=fat:rw:$ESP_DIR_WIN")
# The matching `-device usb-storage` cannot live in ESP_DRIVE_ARGS: QEMU
# realises -device options in command-line order, so one naming bus xhci0.0
# must appear AFTER the qemu-xhci that creates it or the run dies with
# "Bus 'xhci0.0' not found".  It is emitted next to usb-kbd instead.
USB_STICK_ARGS=()
if [ "$USB_IMAGE" -eq 1 ]; then
    echo "=== Building USB image (GPT + FAT32) ==="
    if ! python "$PROJECT_ROOT/scripts/build-usb-image.py" \
            --source "$ESP_DIR" --output "$USB_IMG"; then
        echo "ERROR: could not build $USB_IMG." >&2
        exit 1
    fi
    # `if=none` + an explicit id so the image binds ONLY to the usb-storage
    # device below and is not also auto-attached to the default IDE bus, which
    # would present the same filesystem twice and let firmware boot the copy
    # that is not being tested.
    ESP_DRIVE_ARGS=(-drive "id=usbstick,if=none,format=raw,file=$USB_IMG_WIN")
    USB_STICK_ARGS=(-device "usb-storage,bus=xhci0.0,drive=usbstick")
fi

# Step 3: Create a small swap disk image (16 MiB) for disk-backed swap testing.
SWAP_IMG="$PROJECT_ROOT/build/swap.img"
SWAP_IMG_WIN="$(to_win_path "$SWAP_IMG")"
if [ ! -f "$SWAP_IMG" ]; then
    echo "=== Creating 16 MiB swap disk image ==="
    dd if=/dev/zero of="$SWAP_IMG" bs=1M count=16 status=none 2>/dev/null
fi

# Step 3a: Backing store for the NVMe controller (see the device notes above
# the QEMU invocation).  `-device nvme` requires a drive — a controller with no
# namespace enumerates zero namespaces, and the driver's identify/namespace path
# (the part most likely to be wrong) would go untested, which is precisely the
# "present but inert" trap that `intel-hda` without `hda-duplex` sets.
NVME_IMG="$PROJECT_ROOT/build/nvme.img"
NVME_IMG_WIN="$(to_win_path "$NVME_IMG")"
if [ ! -f "$NVME_IMG" ]; then
    echo "=== Creating 32 MiB NVMe disk image ==="
    dd if=/dev/zero of="$NVME_IMG" bs=1M count=32 status=none 2>/dev/null
fi

# Step 3b: Attach the Path-Z glibc rootfs (rootfs.ext4) as a second virtio-blk
# disk when present.  It is enumerated AFTER swap-disk, so it becomes vdb: the
# kernel's swap loop skips it (ext4 superblock detected) and the /mnt ext4 probe
# mounts it, enabling the real-glibc dynamic-execution self-test.  Built on the
# dev box via `wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh`; git-ignored,
# so the boot test simply omits it (and the self-test no-ops) when it is absent.
ROOTFS_IMG="$PROJECT_ROOT/rootfs.ext4"
ROOTFS_ARGS=()
if [ "$NO_ROOTFS" -eq 1 ] && [ -f "$ROOTFS_IMG" ]; then
    # Said out loud, because "the rootfs was not attached" and "the rootfs is
    # not present" produce identical downstream behaviour and must not produce
    # identical output: one is a deliberate probe, the other is a fresh
    # worktree that never packed the image.
    echo "=== Path-Z glibc rootfs suppressed (--no-rootfs); $ROOTFS_IMG exists but is not attached ==="
    echo "    Rungs that read /mnt will no-op.  This run is tagged as an experiment."
fi
if [ "$NO_ROOTFS" -eq 0 ] && [ -f "$ROOTFS_IMG" ]; then
    # Before QEMU is told to attach it: the image is packed by hand and can be
    # older than the tree, and a stale one produces passing Path-Z rungs that
    # tested nothing current.  See check_rootfs_freshness.
    check_rootfs_freshness
    ROOTFS_IMG_WIN="$(to_win_path "$ROOTFS_IMG")"
    # `snapshot=on`: the guest gets a fully writable vdb, but QEMU buffers the
    # writes into a throwaway overlay and the host file is never modified.
    #
    # WHY THIS IS NOT OPTIONAL.  Without it a boot rewrites rootfs.ext4's
    # *contents* -- measured 2026-08-19, sha256 f62e019d -> b2ecc74d across one
    # boot with nothing else touching the tree -- and that file is an input to
    # `scripts/src_digest.py`'s artifact half.  So the identity of the source
    # changed as a side effect of testing it, and every layout sweep fragmented:
    # arm N and arm N+1 disagreed about what they had built, landed in groups of
    # one, and no band could form.  The sweep of this date aborted after two arms
    # on exactly that (`full:ace827eb...` vs `full:f75959ab...`, same
    # accelerator).  A boot test that mutates its own fixture cannot be used to
    # establish what it booted.
    #
    # WHY NOT `readonly=on`: the Path-Z rungs mount /mnt read-write and writing
    # is part of what they exercise.  `snapshot=on` keeps that behaviour exactly
    # and discards it afterwards, which is what a fixture wants; read-only would
    # change what the test tests.
    #
    # WHY NOT "just exclude rootfs.ext4 from the digest": it genuinely is an
    # input -- 73 staged ELFs the Path-Z rungs execute.  Dropping it makes two
    # different images share one identity, which merges runs that should never
    # be compared.  Over-inclusion splits a band (safe: the answer is
    # "unmeasured"); under-inclusion merges one (unsafe: an inflated band
    # dismisses real regressions silently).  Fix the mutation, not the ledger.
    #
    # Note this also un-inverts the mtime idiom described above
    # check_rootfs_freshness: the image's timestamp now records when it was
    # last *packed*, which is what every other staleness check in this script
    # assumes about the files it compares.
    # SC2054: as with WATCHDOG_ARGS above, the comma is QEMU's property
    # separator within one `-device` argument, not a separator between ours.
    # shellcheck disable=SC2054
    ROOTFS_ARGS=(
        -device virtio-blk-pci,drive=rootfs-disk
        -drive "id=rootfs-disk,if=none,format=raw,snapshot=on,file=$ROOTFS_IMG_WIN"
    )
    echo "=== Attaching Path-Z glibc rootfs: $ROOTFS_IMG (vdb) ==="
fi

# CPU model.  QEMU's default (`qemu64`) advertises no SMEP, SMAP or UMIP, so
# without this the kernel's supervisor-mode protections are silently inert under
# test: `smep_smap::init()` logs "not supported by CPU", never touches CR4, and
# the STAC/CLAC paths are skipped.  We were shipping a SMEP we believed was
# active and was not (see B-QEMU-DEFAULT-CPU-HAS-NO-SMEP-SMAP-UMIP in
# known-issues.md).  Requesting them explicitly makes the boot test actually
# exercise those paths — and makes a future `clac` in the ISR stubs testable
# rather than dead code.  Override with QEMU_CPU=... to test other models.
#
# The override is remembered here rather than inferred later by comparing
# against the default string: a default edited in this line would silently stop
# matching such a comparison, and the consequence of a missed match is a probe
# recorded as an ordinary boot of the tree.
if [ -n "${QEMU_CPU:-}" ]; then
    QEMU_CPU_OVERRIDDEN=1
else
    QEMU_CPU_OVERRIDDEN=0
fi
QEMU_CPU="${QEMU_CPU:-qemu64,+smep,+smap,+umip}"

# Extra QEMU arguments, for diagnosing emulator-side effects without touching
# the guest.  Word-split on purpose so a caller can pass several:
#
#     QEMU_EXTRA="-accel tcg,tb-size=512" ./scripts/boot-test.sh --bench
#
# This exists because a benchmark result can move by 4x for reasons that are
# entirely QEMU's — commit 665fbb27b, which edits only `audio_mixer.rs`, moved
# `crypto_sha256_64B` from 7364 to 28184 cycles while the SHA-256 machine code
# stayed byte-identical (same symbol size, same mangled hash; only the address
# changed).  Separating "our code got slower" from "TCG got slower at running
# the same code" needs the binary held fixed and the emulator varied, which is
# exactly what this knob is for.  Default empty: no effect on ordinary runs.
#
# Setting it also marks the benchmark record an experiment (see BENCH_EXPERIMENT
# below), so a probe run cannot become the baseline a later honest run is judged
# against.
read -r -a QEMU_EXTRA_ARGS <<< "${QEMU_EXTRA:-}"

# Which virtio-GPU device to present, and therefore which of the driver's two
# resource-creation paths gets exercised:
#
#     SLATE_GPU=virtio-gpu-gl-pci ./scripts/boot-test.sh
#
# Default `virtio-gpu-pci` is the plain 2D device, which is what every tracking
# run has always used.  `virtio-gpu-gl-pci` routes the same commands through
# QEMU's virgl path, where the framebuffer has to be created with
# RESOURCE_CREATE_3D and the SCANOUT bind or SET_SCANOUT is refused (see
# kernel/src/virtio/gpu.rs::create_resource_3d).  That path has no other
# coverage, so without this knob a regression in it is invisible here and only
# surfaces when the graphics lane tries to run Mesa.
#
# Passing the device via QEMU_EXTRA cannot substitute for this: that *adds* a
# second GPU, and the driver binds the first one it finds, so the GL device is
# never the one under test.  The display backend has to move with it — a GL
# device needs a real EGL context and QEMU refuses to start it under
# `-display none` — which is the other thing a caller cannot express by
# appending arguments.
GPU_DEVICE="${SLATE_GPU:-virtio-gpu-pci}"
if [ "$GPU_DEVICE" = "virtio-gpu-pci" ]; then
    GPU_DISPLAY_ARGS=(-display none)
    GPU_OVERRIDDEN=0
else
    GPU_DISPLAY_ARGS=(-display egl-headless)
    GPU_OVERRIDDEN=1
fi

# Why this run is a deliberate probe rather than a tracking run, e.g.
#
#     BENCH_EXPERIMENT="alignment probe on crypto::compress" ./scripts/boot-test.sh --bench
#
# Such a run measures a kernel (or an emulator) that no checkout reproduces, so
# it is recorded in full but excluded from every future baseline.  Set it for
# any hand-modified build: a bisect step, a toggled compiler feature, a source
# patch applied only to answer a question.  QEMU_EXTRA implies it; a modified
# *guest* cannot be detected from here, so it must be declared.
#
# The cost of not having had it: five probe runs of the placement investigation
# went in unlabelled, three reading ~8085 ns for `crypto_sha256_64B` and two
# ~1936 for identical source, which between them would have stretched that
# benchmark's outlier fence past 4x and blinded the detector for it.

# Checked a second time here, and the placement is the decision.
#
# It has to be *after* the build, because our own build is a large consumer of
# commit charge and the margin that existed before it may not exist after --
# checking only up front would gate on a number the build then invalidates.
#
# It has to be *before* the boot lock, because this call can wait fifteen
# minutes, and waiting inside the lock would idle while holding the one resource
# the other two lanes queue for -- stalling them for a condition their own
# builds caused, which is the worst possible place to put a sleep.
#
# What that ordering gives up: pressure arriving during the lock wait itself is
# not caught.  That is the residual, and it is the right one to accept -- the
# alternative trades a rare miss for a certain stall.
check_commit_headroom "after building, before queueing to boot"

# --- Cross-worktree boot lock -------------------------------------------------
#
# The three lanes each work in their OWN git worktree (D:/…/os, os-lane-a,
# os-lane-c), so `target/` and `build/serial-test.txt` are NO LONGER shared —
# roadmap.md §6 predates the worktree split and is wrong about that.  What IS
# still shared is the machine: we boot under TCG (pure emulation, CPU-bound),
# so two concurrent QEMUs roughly double each other's wall-clock boot time and
# push long boots past TIMEOUT, producing phantom "hang" failures.  A soak that
# takes ~480s/iteration solo starts timing out when another lane boots
# alongside it.
#
# So the lock must live somewhere ALL worktrees can see.  `git rev-parse
# --git-common-dir` resolves to the single real .git directory shared by every
# worktree (linked worktrees return its absolute path), which is exactly the
# anchor we need.  Fall back to build/ when git is unavailable — that degrades
# to the old per-tree behaviour rather than failing.
#
# Acquisition is `mkdir`, which is atomic on both NTFS and POSIX (unlike a
# test-then-create on a file).  Metadata goes in a file inside the directory.
#
# Escape hatches:
#   BOOT_LOCK=0          skip locking entirely (single-lane / debugging)
#   BOOT_LOCK_WAIT=<sec> max seconds to wait for the lock (default 3600).
#                        On expiry we proceed anyway rather than failing the
#                        test — a slow boot beats a spurious error.
BOOT_LOCK_DIR=""
BOOT_LOCK_OWNER=""   # must exist before release_boot_lock runs under `set -u`
if [ "${BOOT_LOCK:-1}" != "0" ]; then
    _common_git="$(git -C "$PROJECT_ROOT" rev-parse --git-common-dir 2>/dev/null || echo "")"
    if [ -n "$_common_git" ]; then
        # A main worktree reports a relative ".git"; make it absolute.
        case "$_common_git" in
            /*|[A-Za-z]:*) : ;;
            *) _common_git="$PROJECT_ROOT/$_common_git" ;;
        esac
        BOOT_LOCK_DIR="$_common_git/slateos-boot-lock"
    else
        BOOT_LOCK_DIR="$PROJECT_ROOT/build/.boot-lock"
    fi
fi

# The region between the two BOOT-LOCK-REGION markers below is extracted
# verbatim and executed by `scripts/test-boot-lock.sh`, which is the only test
# this logic has: the acquire loop runs *after* the kernel build, so reaching it
# through a normal `boot-test.sh` run costs a full build (~7 min) per case, and
# the interesting cases need a second lane holding the lock.  Keep the markers
# in place, and keep everything between them free of dependencies on the build
# steps above — the harness supplies only PROJECT_ROOT, BOOT_LOCK_DIR and the
# `which-lane.py` it points at.
# --- BEGIN BOOT-LOCK-REGION ---
# Release is idempotent and safe to call when we never acquired: we only remove
# the lock if the owner file still names THIS process, so we can never delete a
# lock that another lane acquired after we broke/released ours.  It also drops
# our queue ticket, which exists from before the acquire loop and so outlives
# every path through it — including the ones that never acquire anything.
release_boot_lock() {
    _boot_lock_drop_ticket
    [ -n "$BOOT_LOCK_DIR" ] || return 0
    [ -d "$BOOT_LOCK_DIR" ] || return 0
    if [ "$(cat "$BOOT_LOCK_DIR/owner" 2>/dev/null || echo "")" = "$BOOT_LOCK_OWNER" ]; then
        rm -rf "$BOOT_LOCK_DIR" 2>/dev/null || true
    fi
}

# ---------------------------------------------------------------------------
# The queue.
#
# `mkdir` is atomic, which makes it a correct mutex and a terrible scheduler:
# whoever calls it first wins, and nothing remembers who has been waiting.  A
# lane that finishes a boot and immediately starts another is already at the
# `mkdir` while a waiter is somewhere inside its 5-second sleep, so it wins
# every time.  Worse, both waiters poll on the same 5s period, so their probes
# are phase-locked and whoever entered the loop earlier probes earlier in every
# subsequent cycle: there is no randomness for repeated tries to average out,
# and the lane that is behind stays behind.  Lane B observed exactly this —
# five consecutive lane A runs, ~6 minutes apart, across forty minutes in which
# lane B never once won the `mkdir`
# (requests/b-a-the-boot-lock-has-no-queue-so-a-waiting-lane-can-starve.md).
#
# So: keep `mkdir` as the atomic primitive and put a ticket queue in front of
# it.  Every run drops a `<epoch>-<pid>` file in `$BOOT_LOCK_DIR.waiters/`
# before the loop, and only attempts `mkdir` when its own ticket is the oldest.
# A run that releases and immediately re-runs takes a *new* ticket at the back
# of the queue, which is what turns the observed starvation into a handover.
#
# The tickets live in a sibling directory rather than inside `$BOOT_LOCK_DIR`,
# because the lock dir is created and destroyed by acquisition; the queue has
# to outlive that.
#
# Note the `|| true` and the explicit `return 0` on the helpers below: this
# script runs under `set -euo pipefail`, where an empty/absent queue directory
# would otherwise make `ls` fail, `pipefail` propagate it, and the whole boot
# test abort during what is merely a poll of a queue nobody is in.
_boot_lock_head() {
    # `sed -n 1p` rather than `head -1`: head closes the pipe after one line,
    # which can SIGPIPE `sort` and — under `pipefail` — turn a successful query
    # into a failed one.
    ls "$BOOT_LOCK_WAITERS" 2>/dev/null | sort -t- -k1,1n -k2,2n | sed -n '1p' || true
}

# Is the process named by a ticket still running?  Returns non-zero for "no"
# *and* for "cannot tell" — callers use this to decide whether to refuse, and
# refusing on a guess is worse than proceeding.
_boot_lock_ticket_alive() {  # $1 = ticket name
    local pid
    [ "$_lock_pidcheck" = "1" ] || return 1
    pid="${1##*-}"
    case "$pid" in ''|*[!0-9]*) return 1 ;; esac
    kill -0 "$pid" 2>/dev/null
}

_boot_lock_drop_ticket() {
    [ -n "${BOOT_LOCK_WAITERS:-}" ] || return 0
    [ -n "${_lock_ticket:-}" ] || return 0
    rm -f "$BOOT_LOCK_WAITERS/$_lock_ticket" 2>/dev/null || true
    rmdir "$BOOT_LOCK_WAITERS" 2>/dev/null || true
}

# Re-assert our ticket every poll.  It is normally created once, but a sweep in
# another lane can misjudge us (a `kill -0` that briefly fails, a clock jump),
# and a waiter whose ticket has vanished would never be head again and would
# wait out the full BOOT_LOCK_WAIT for nothing.  Re-creating it under the same
# name is idempotent and keeps our original queue position, because the
# position is the epoch baked into the name, not the file's mtime.
_boot_lock_ensure_ticket() {
    [ -e "$BOOT_LOCK_WAITERS/$_lock_ticket" ] && return 0
    mkdir -p "$BOOT_LOCK_WAITERS" 2>/dev/null || true
    : > "$BOOT_LOCK_WAITERS/$_lock_ticket" 2>/dev/null || true
    return 0
}

# Tickets need the same liveness/age sweep the lock itself has, and for the
# same reason: a run killed by `run-timeout.py`'s Job Object is torn down
# without executing any exit path, so it leaves its ticket behind — and a dead
# ticket at the *head* of the queue blocks every lane, which would be a worse
# failure than the starvation this queue was added to fix.
#
# The rules are deliberately the lock's rules, not a second set: proven-alive
# is never swept (a lane legitimately waiting a long time must keep its place,
# or the queue is just a slower race); a pid we can prove gone is swept once
# the ticket is at least 60s old (below that a transient is likelier than a
# real death); and everything we cannot judge — unparseable name, `kill`
# unusable, a pid from a previous Windows session — falls to the same 1200s age
# backstop the lock uses.  That backstop is safely above the 3600s
# BOOT_LOCK_WAIT only in the sense that a *live* waiter is exempt from it; a
# waiter we cannot see is indistinguishable from a corpse after 20 minutes.
_boot_lock_sweep_tickets() {
    local now t base epoch pid age
    now="$(date +%s)"
    for t in "$BOOT_LOCK_WAITERS"/*-*; do
        [ -e "$t" ] || continue
        base="${t##*/}"
        [ "$base" = "$_lock_ticket" ] && continue
        epoch="${base%%-*}"
        pid="${base##*-}"
        case "$epoch$pid" in
            ''|*[!0-9]*)
                echo "=== Dropping unparseable boot-lock ticket '$base' ==="
                rm -f "$t" 2>/dev/null || true
                continue
                ;;
        esac
        age=$(( now - epoch ))
        if [ "$_lock_pidcheck" = "1" ] && kill -0 "$pid" 2>/dev/null; then
            continue
        fi
        if [ "$_lock_pidcheck" = "1" ] && [ "$age" -ge 60 ]; then
            echo "=== Dropping boot-lock ticket $base: waiter pid $pid is gone (age ${age}s) ==="
            rm -f "$t" 2>/dev/null || true
        elif [ "$age" -gt 1200 ]; then
            echo "=== Dropping stale boot-lock ticket $base (age ${age}s) ==="
            rm -f "$t" 2>/dev/null || true
        fi
    done
    return 0
}

if [ -n "$BOOT_LOCK_DIR" ]; then
    BOOT_LOCK_WAITERS="$BOOT_LOCK_DIR.waiters"
    BOOT_LOCK_OWNER="$(python "$PROJECT_ROOT/scripts/which-lane.py" 2>/dev/null | awk '/^lane:/{print $2}' || true)"
    BOOT_LOCK_OWNER="lane-${BOOT_LOCK_OWNER:-?}/pid-$$/$(date +%s)"
    _lock_ticket="$(date +%s)-$$"
    _lock_wait="${BOOT_LOCK_WAIT:-3600}"
    _lock_waited=0
    # Can we ask whether a pid is alive at all?  Probe on ourselves, where the
    # answer is known: if `kill -0 $$` fails, `kill` is unusable here and every
    # owner would look dead, which would break live locks on every poll.  In
    # that case we disable the liveness breaker entirely and fall back to the
    # age rule.  This is the "stay conservative when the answer is unknown"
    # half of requests/b-a-boot-lock-survives-its-dead-owner.md.
    if kill -0 $$ 2>/dev/null; then _lock_pidcheck=1; else _lock_pidcheck=0; fi
    # Take our place in the queue *before* the first probe, and arm the exit
    # path immediately: from here on every way out of this script — acquire,
    # give up, Ctrl-C, SIGTERM — has to drop the ticket, or the next lane
    # queues behind a corpse.
    _boot_lock_ensure_ticket
    trap 'release_boot_lock' EXIT INT TERM
    while :; do
        _boot_lock_ensure_ticket
        _boot_lock_sweep_tickets
        _lock_head="$(_boot_lock_head)"
        # Only the head of the queue races for the lock, so there is no race:
        # everyone else sleeps.  `mkdir` can still fail here (the incumbent
        # holds it), which is the ordinary wait.
        if [ "$_lock_head" = "$_lock_ticket" ] && mkdir "$BOOT_LOCK_DIR" 2>/dev/null; then
            break
        fi
        # These describe the *lock*, and stay at their unknown values when
        # there is no lock to describe — i.e. when we are merely queued behind
        # another ticket.  Setting them per-iteration rather than leaving last
        # iteration's values matters now that the loop body can run without
        # ever looking at a lock directory.
        _lock_age=999999
        _lock_alive="unknown"
        _lock_pid=""
        if [ -d "$BOOT_LOCK_DIR" ]; then
            # Age of the lock, for the stale breaker below.
            #
            # The owner file is written a moment AFTER the `mkdir` that
            # acquires, so a waiter polling inside that window sees a lock
            # directory with no owner file.  That is a *young* lock, not an old
            # one — treating the missing file as "infinitely old" (which this
            # did, via 999999) let a waiter delete a lock another lane had just
            # legitimately taken, and then both would boot QEMU at once.  So
            # fall back to the directory's own mtime, which `mkdir` stamps at
            # acquisition, and only claim ignorance when neither can be stat'd.
            _lock_mtime="$(date -r "$BOOT_LOCK_DIR/owner" +%s 2>/dev/null \
                           || date -r "$BOOT_LOCK_DIR" +%s 2>/dev/null || echo 0)"
            [ "$_lock_mtime" -gt 0 ] && _lock_age=$(( $(date +%s) - _lock_mtime ))
            # Break a dead lock: the owner string carries the holder's pid, and
            # all lanes run this script under the same MSYS bash, so `kill -0`
            # answers across worktrees.  A run killed by `run-timeout.py`'s Job
            # Object is torn down without executing any exit path, so
            # `release_boot_lock` never fires and the lock outlives its owner by
            # up to the 20 minutes the age rule needs — landing on whichever
            # lane runs next as a stall indistinguishable from a hung boot.
            #
            # The 60s floor is deliberate.  A pid that cannot be seen is only
            # evidence of death if the lock has existed long enough for the
            # owner to be observable at all; below that we would be acting on a
            # lock taken seconds ago, where a transient (an owner file not yet
            # flushed, a pid not yet visible) is likelier than a real death.  A
            # healthy boot holds the lock for minutes, so the floor costs a
            # waiter nothing and still cuts the worst case from 1200s to ~60s.
            #
            # Liveness is a tri-state — alive / dead / unknown — and all three
            # answers matter.  Collapsing "unknown" into either of the other two
            # is how this goes wrong: into "dead" and we break live locks
            # whenever `kill` is unavailable, into "alive" and we never break
            # anything.
            if [ "$_lock_pidcheck" = "1" ]; then
                _lock_pid="$(sed -n 's#.*/pid-\([0-9][0-9]*\)/.*#\1#p' \
                             "$BOOT_LOCK_DIR/owner" 2>/dev/null || echo "")"
                if [ -n "$_lock_pid" ]; then
                    if kill -0 "$_lock_pid" 2>/dev/null; then
                        _lock_alive="yes"
                    elif [ "$_lock_age" -ge 60 ]; then
                        _lock_alive="no"
                    fi
                fi
            fi
            if [ "$_lock_alive" = "no" ]; then
                echo "=== Breaking boot lock: owner pid $_lock_pid is gone (held by $(cat "$BOOT_LOCK_DIR/owner" 2>/dev/null || echo unknown), age ${_lock_age}s) ==="
                rm -rf "$BOOT_LOCK_DIR" 2>/dev/null || true
                continue
            fi
            # The age rule is the backstop for everything the pid check cannot
            # see: a recycled pid now belonging to something unrelated, an owner
            # from a previous Windows session, an owner whose pid lives in a
            # different MSYS instance's process table, an unparseable owner
            # string, and the case where `kill` itself is unavailable.
            #
            # It deliberately does NOT apply to an owner we can prove is alive.
            # It was written when liveness was unknowable, so age was the only
            # available proxy for death and 1200s was picked as "longer than any
            # healthy boot".  But a boot that outlives that estimate is not
            # dead, it is slow — a cold host, a QEMU stalled on I/O — and
            # breaking its lock starts a second QEMU alongside the first, which
            # is the one outcome worse than waiting: two mutually-slowed runs,
            # either of which may now fail for reasons that have nothing to do
            # with the code under test.
            if [ "$_lock_alive" != "yes" ] && [ "$_lock_age" -gt 1200 ]; then
                echo "=== Breaking stale boot lock (age ${_lock_age}s, held by $(cat "$BOOT_LOCK_DIR/owner" 2>/dev/null || echo unknown)) ==="
                rm -rf "$BOOT_LOCK_DIR" 2>/dev/null || true
                continue
            fi
        fi
        if [ "$_lock_waited" -ge "$_lock_wait" ]; then
            # Two different endings, because they mean opposite things.
            #
            # If we can point at a *live* process that is entitled to the lock
            # ahead of us, booting anyway would put a second QEMU on the host
            # alongside a run that is either in progress or about to start —
            # precisely the outcome this lock exists to prevent, arrived at an
            # hour later when nobody is watching, and then reported as an
            # ordinary slow/failed boot of the code under test.  A wait that
            # ends by doing the forbidden thing is not a bounded wait.  So
            # refuse, with a status of its own: "I waited an hour and gave up"
            # is a true statement a reader (or a retry loop) can act on; a
            # phantom failure caused by contention is not.
            #
            # "Entitled ahead of us" is two things, not one.  The obvious one
            # is a live lock owner.  The other is a live waiter at the head of
            # the queue: it holds no lock yet, so the old owner-only test would
            # cheerfully boot alongside it — and the head waiter is precisely
            # the process most likely to enter QEMU in the next few seconds.
            # Adding the queue and then not consulting it here would leave the
            # two-QEMU escalation intact, just moved.
            _lock_blocker=""
            if [ "$_lock_alive" = "yes" ]; then
                _lock_blocker="owner $(cat "$BOOT_LOCK_DIR/owner" 2>/dev/null || echo unknown)"
            elif [ -n "$_lock_head" ] && [ "$_lock_head" != "$_lock_ticket" ] \
                 && _boot_lock_ticket_alive "$_lock_head"; then
                _lock_blocker="queued waiter $_lock_head"
            fi
            if [ -n "$_lock_blocker" ]; then
                echo "=== Boot lock unavailable after ${_lock_waited}s: LIVE $_lock_blocker is ahead of us; refusing to boot alongside it ==="
                echo "=== Nothing was booted — this says nothing about the code under test.  Retry, or raise BOOT_LOCK_WAIT. ==="
                # Take the previous run's artefacts with us.  The serial log is
                # normally truncated a few lines below, *after* the lock, so on
                # this path it still holds the last boot's output — and every
                # soak wrapper we have greps it for wedge/panic signatures the
                # moment the script returns.  A caller that has not been taught
                # about exit 4 would then classify a stale log as this run's
                # result: re-reporting an old catch as new, or inventing one.
                # Deleting them makes "nothing was booted" self-evident to any
                # caller, including ones written later that never heard of this
                # status.  (`${…:-}` because the lock region is extracted and
                # run standalone by scripts/test-boot-lock.sh, which supplies
                # none of the build variables.)
                rm -f "${SERIAL_FILE:-}" "${SERIAL_FILE:+${SERIAL_FILE%.txt}-regs.txt}" 2>/dev/null || true
                exit 4
            fi
            # Otherwise nothing live can be demonstrated — an ownerless or
            # unreadable lock, a queue of processes we cannot see — so
            # proceeding is the same conservative default it always was.
            echo "=== Boot lock still held after ${_lock_waited}s; booting anyway (results may be slow) ==="
            BOOT_LOCK_DIR=""
            break
        fi
        if [ $(( _lock_waited % 60 )) -eq 0 ]; then
            if [ -d "$BOOT_LOCK_DIR" ]; then
                echo "=== Waiting for boot lock, held by $(cat "$BOOT_LOCK_DIR/owner" 2>/dev/null || echo unknown) (${_lock_waited}s) ==="
            else
                echo "=== Waiting for boot lock: queued behind ticket ${_lock_head:-unknown} (${_lock_waited}s) ==="
            fi
        fi
        sleep 5
        _lock_waited=$(( _lock_waited + 5 ))
    done
    if [ -n "$BOOT_LOCK_DIR" ]; then
        echo "$BOOT_LOCK_OWNER" > "$BOOT_LOCK_DIR/owner" 2>/dev/null || true
        echo "=== Boot lock acquired: $BOOT_LOCK_OWNER ==="
    fi
    # We hold the lock (or gave up on it); either way our place in the queue is
    # spent, and leaving it would make the next lane wait behind a ticket whose
    # holder is no longer waiting for anything.
    _boot_lock_drop_ticket
fi
# --- END BOOT-LOCK-REGION ---

# Step 4: Boot QEMU
echo "=== Booting QEMU (timeout: ${TIMEOUT}s, cpu: $QEMU_CPU) ==="
rm -f "$SERIAL_FILE"
# Removed together with the serial log, and for the identical reason: both are
# evidence about *this* boot, and a leftover from the previous one is worse
# than nothing because it looks like evidence.  record_boot_outcome only passes
# the file when it exists, so a run that dies before QEMU starts records no
# host-failure claim at all rather than the last run's.
rm -f "$QEMU_STDERR"

OVMF_WIN="$(to_win_path "$OVMF")"
rm -f "$PIDFILE"
# Wall clock across the QEMU window only — not the build, which runs at full
# parallelism and says nothing about host interference.
#
# This is the most sensitive contamination signal the harness has, and until
# 2026-08-15 it was computed (as ELAPSED, for the progress message) and thrown
# away.  TCG is pure emulation, CPU-bound and single-threaded, so for a fixed
# amount of guest work the wall time is guest-work divided by the share of a
# core the emulator actually got: a run descheduled half the time simply takes
# twice as long, and there is nowhere for that time to hide because it is
# measured on the host's clock, outside the guest.  That is exactly the frame
# the in-guest canary cannot reach.  Two boots of one binary minutes apart read
# 160s and 365s while the canary called the 365s run its cleanest ever.
#
# A separate epoch stamp rather than reusing ELAPSED.  When this was written
# that was because ELAPSED counted `sleep 1` iterations plus the loop body's own
# work, so it drifted on exactly the busy hosts whose measurement matters most.
# ELAPSED is wall-clock as of 2026-08-24 (see the wait loop below), so the two
# would now agree -- but they are still kept apart, because they start at
# different instants: this one is stamped before QEMU is launched, ELAPSED's
# after, and the launch itself is not free.  Fixing ELAPSED does not make this
# redundant; it makes the pair consistent.
QEMU_START_EPOCH=$(date +%s)
#
# `-device ati-vga,model=rv100` presents a Radeon 7000 (PCI 1002:5159) whose
# register interface QEMU emulates for real, including the CRTC block.  It is
# here so the ATI driver's register offsets are checked against a device on
# every boot instead of being trusted: a transposed offset in
# kernel/src/drm/ati/regs.rs is otherwise invisible until it drives a monitor.
# The kernel only *reads* it and registers no backend for it.
#
# `-vga std` is REQUIRED here and must not be dropped.  ati-vga is a VGA-class
# device, so naming it on the command line makes QEMU suppress the machine's
# default VGA -- and then the only VGA controller is one OVMF has no driver
# for.  No driver means no GOP, which means limine hands the kernel no
# framebuffer, which means the console never initialises and console::self_test
# panics with "console not initialized".  That is not a display-subsystem bug
# and the backtrace does not mention graphics at all; it took a full boot to
# diagnose.  With `-vga std` both controllers are present (1234:1111 driving
# the display, 1002:5159 for the driver to read), QEMU emits no warning, and
# OVMF still binds GOP to the standard adapter.  Verified on QEMU 11.0.93.
#
# (Note that virtio-gpu-pci does NOT have this effect: it is display-class but
# not VGA-class, so it never suppressed the default.  Testing coexistence with
# virtio-gpu alone is what missed this.)
#
# The three audio devices are here for the same reason as ati-vga: without
# them, ac97::self_test, hda::self_test and virtio::sound::self_test each print
# "no device (skipped)" on every boot, so the drivers' register programming,
# DMA setup and playback paths are *never executed* -- and a driver that is
# only ever compiled is a driver whose bugs ship.  Each device below is chosen
# to match the first entry of its driver's own PCI ID table:
#
#   AC97               8086:2415 (ICH AC'97)  -> AC97_DEVICE_IDS[0]
#   intel-hda          8086:2668 (ICH6 HDA)   -> HDA_DEVICE_IDS[0]
#   virtio-sound-pci   virtio device type 25  -> virtio/sound.rs
#
# `hda-duplex` is the *codec* that hangs off the intel-hda controller.  It is
# not optional decoration: with no codec on the link, STATESTS reads 0, the
# driver's codec enumeration finds nothing, and probe_codec / probe_afg / the
# whole CORB-RIRB verb round-trip stay untested -- which is most of the
# driver.  The controller alone would only prove that a BAR maps.
#
# `-audiodev none` is deliberate and correct for a headless harness: it
# discards the decoded samples, but it changes nothing about the *device
# model*, which is the only part the kernel talks to.  The BDL walk, the
# CORB/RIRB rings, the virtqueue and every MMIO/PIO register still behave
# exactly as with a real backend, and the guest still observes buffer-completion
# progress.  A real backend would only add a dependency on the build host
# having a sound card.
#
# Unlike ati-vga these are multimedia-class, not VGA-class, so they suppress
# nothing and need no counterpart to `-vga std`.
#
# `nvme` and `qemu-xhci` close the same gap for the last two whole subsystems
# that had no hardware here.  Before they were added, every boot printed
#
#     [nvme] No NVMe controller found
#     [xhci] No xHCI controller found (USB not available)
#
# so nvme.rs (~950 lines) and xhci.rs (~2400 lines) had never executed past
# their PCI scan.  Both are found by CLASS, not by vendor/device id, so the
# choice of model matters less than it does above -- any controller QEMU offers
# in the right class will bind:
#
#   nvme       class 01h/08h (NVM)        -> nvme.rs   find_devices_by_class
#   qemu-xhci  class 0Ch/03h (USB)        -> xhci.rs   find_devices_by_class
#
# `usb-kbd` is to qemu-xhci what `hda-duplex` is to intel-hda: without a device
# on the bus, the driver enumerates an empty root hub and everything that makes
# xHCI hard -- slot enable, address-device, the control-transfer TRB rings,
# descriptor parsing, the HID boot-protocol path -- stays untested.  A keyboard
# is the right choice because xhci.rs already has a HID boot-protocol driver
# looking for exactly this (USB_CLASS_HID / USB_HID_SUBCLASS_BOOT).
#
# The NVMe drive is `if=none` and named explicitly so it attaches ONLY to the
# nvme controller.  A bare `-drive` would be auto-assigned to the q35 AHCI bus
# and change which disk is which, which is the sort of thing that turns a
# coverage improvement into a mysterious rootfs failure.
#
# --- The three NICs ---------------------------------------------------------
#
# Until now the boot test attached no NIC at all, and got one anyway: with no
# -netdev/-nic/-net on the command line QEMU silently creates a default NIC,
# which on q35 is an e1000e (8086:10d3).  So e1000.rs was tested by accident,
# and rtl8139.rs and virtio/net.rs printed "no device" on every boot and had
# never executed a single line past their PCI scan.
#
# All three are now named explicitly, each on its own user-mode netdev.  Being
# explicit is the point: the implicit default is a QEMU policy that changes by
# machine type and by version, so a test that depends on it is a test that
# reports on QEMU as much as on us.  `e1000e` is named rather than `e1000`
# because 8086:10d3 is what the default produced, so this keeps the device the
# e1000 driver has actually been running against rather than quietly swapping
# it for an 82540EM (8086:100e).
#
# Distinct MACs are required, not cosmetic: `net::interface::init` takes the
# MAC of whichever NIC it selects, and two NICs answering to the same address
# on one user-net segment would make ARP results depend on arrival order.
#
# What this actually tests is the transmit datapath.  Each of the three
# drivers now sends one inert frame in its self-test (addressed to itself,
# with the IEEE 802 local-experimental EtherType 0x88B5) and waits for the
# hardware to hand the descriptor back -- e1000's DD bit, the RTL8139's OWN
# bit, virtio's used ring.  That needs no peer on the wire and proves what no
# register read-back can: that the ring lives where the device was told, that
# the addresses programmed into it are physical, and that the doorbell write
# reached the right register.
#
# Note that adding virtio-net changes which NIC the *stack* uses:
# `net::send_frame` and `net::interface::init` both prefer virtio-net, then
# e1000, then rtl8139.  That is a deliberate consequence -- it puts the virtio
# path under the existing DHCP/ARP/ICMP tests, while e1000 and rtl8139 keep
# their own direct datapath coverage regardless of which one is primary.
"$QEMU" \
    -netdev user,id=net0 \
    -device e1000e,netdev=net0,mac=52:54:00:12:34:56 \
    -netdev user,id=net1 \
    -device rtl8139,netdev=net1,mac=52:54:00:12:34:57 \
    -netdev user,id=net2 \
    -device virtio-net-pci,netdev=net2,mac=52:54:00:12:34:58 \
    -drive "if=pflash,format=raw,readonly=on,file=$OVMF_WIN" \
    "${ESP_DRIVE_ARGS[@]}" \
    -device virtio-blk-pci,drive=swap-disk \
    -drive "id=swap-disk,if=none,format=raw,file=$SWAP_IMG_WIN" \
    "${ROOTFS_ARGS[@]}" \
    "${WATCHDOG_ARGS[@]}" \
    "${MONITOR_ARGS[@]}" \
    -device "$GPU_DEVICE" \
    -vga std \
    -device ati-vga,model=rv100 \
    -audiodev none,id=snd0 \
    -device AC97,audiodev=snd0 \
    -device intel-hda \
    -device hda-duplex,audiodev=snd0 \
    -device virtio-sound-pci,audiodev=snd0,streams=2 \
    -drive "id=nvme-disk,if=none,format=raw,file=$NVME_IMG_WIN" \
    -device nvme,drive=nvme-disk,serial=SLATE-NVME-1 \
    -device qemu-xhci,id=xhci0 \
    -device usb-kbd,bus=xhci0.0 \
    "${USB_STICK_ARGS[@]}" \
    -serial "file:$SERIAL_FILE_WIN" \
    -pidfile "$PIDFILE_WIN" \
    "${GPU_DISPLAY_ARGS[@]}" \
    -no-reboot \
    -m 3072M \
    -cpu "$QEMU_CPU" \
    "${QEMU_EXTRA_ARGS[@]}" \
    -machine q35 2> "$QEMU_STDERR" &
QEMU_PID=$!
# Ensure QEMU is reaped even if the harness is interrupted (Ctrl-C, SIGTERM)
# or exits early — a surviving qemu keeps the serial file locked and breaks
# the next run.  (A hard SIGKILL/TaskStop of the harness cannot run this, so
# callers that force-stop the script must still clean up qemu themselves.)
#
# NOTE: this trap must ALSO release the boot lock — it replaces the
# release-only trap installed at lock-acquisition time, and bash keeps just one
# handler per signal.  Reaping qemu first is deliberate: the next lane must not
# be handed the lock while our emulator is still burning CPU.
#
# `$?` is captured as the handler's FIRST argument, before anything else runs:
# on_boot_exit reaps qemu and invokes the outcome recorder, either of which
# would otherwise overwrite the status we are trying to record.  EXIT and
# INT/TERM get separate handlers only so the recorder can tell an interrupted
# run (not a boot outcome) from a completed one; on_boot_exit's own once-guard
# keeps the cleanup single even though bash runs both on a signal.
trap 'on_boot_exit "$?" signal' INT TERM
trap 'on_boot_exit "$?" exit' EXIT

# Wait for BOOT_OK or timeout
#
# ELAPSED IS WALL-CLOCK SECONDS, NOT ITERATIONS, AND THE DIFFERENCE DECIDES
# PASS/FAIL.  Until 2026-08-24 this loop counted its own iterations
# (`ELAPSED=$((ELAPSED + 1))` after a `sleep 1`).  An iteration is not a second:
# it is `sleep 1` *plus* a `grep -q` over a serial log that reaches 2.7 MB, plus
# the stall-tracking `stat`.  Measured against this script's own epoch stamps,
# the counter ran 22-26% slow on an **idle** host and worse on a loaded one:
#
#   | boot    | host   | ELAPSED at BOOT_OK | guest armed+arm | real QEMU wall |
#   |---------|--------|--------------------|-----------------|----------------|
#   | batch40 | loaded | 665 s              | ~890 s          | 903 s          |
#   | batch41 | idle   | 349 s              | ~472 s          | 465 s          |
#   | batch42 | idle   | 439 s              |  549 s          | 563 s          |
#
# The guest's own clock tracks real time to within ~2.5%; the drift was all
# here.  Three things were wrong as a result:
#
#  1. **`$TIMEOUT` did not bound wall time.**  batch40's "900 s timeout" let
#     QEMU run 903 s and would have let it reach ~1200 s.  The overrun scales
#     with host load, so the kill under-fires exactly when a run is most likely
#     to need it.
#  2. **`"$WAIT_MARKER detected after Ns"` was systematically low**, and it is
#     the number a reader quotes when comparing boots.
#  3. **The kernel's liveness watchdog and this timeout were in different
#     units.**  The harness passes `$TIMEOUT` to the guest as
#     `sched.boot_deadline_ms`, and `liveness_arm` derives
#     `deadline = timeout - 45 s - now_at_arm` measured in *real* monotonic
#     nanoseconds.  With `$TIMEOUT` denominated in slow iterations, the guest's
#     deadline landed hundreds of seconds before the harness's kill instead of
#     the 45 s the design intends -- so a healthy-but-slow boot tripped the
#     watchdog while the harness still thought it had a quarter of its budget
#     left.  That is the mechanism behind
#     known-issues.md -> TD-A-BOOT-TEST-IS-NOT-ISOLATED-FROM-HOST-LOAD, which
#     was filed as "host contention breaks an assumption" when it is really a
#     unit mismatch between two clocks that are supposed to be the same one.
#
# This does *not* make a loaded host's boot pass -- batch40 genuinely needed
# 903 s of a 855 s allowance, and no clock change invents time.  What it buys is
# that both sides now measure the same thing, so the verdict says "this boot
# exceeded its wall-clock budget", which is true and actionable, instead of a
# watchdog report that reads as a hang.
#
# `date +%s` per iteration rather than bash's `SECONDS`: SECONDS counts from
# shell start, which includes the gates, the build and staging.
WAIT_START_EPOCH="$(date +%s)"
ELAPSED=0
# Serial-stall tracking.  We remember the serial log's last observed size and
# the elapsed time at which it last grew; if (ELAPSED - last-growth) reaches
# STALL_SECS the kernel has gone silent.  Both are wall-clock seconds now, so
# STALL_SECS means what its name says on a busy host too -- previously a stall
# had to last ~1.3x STALL_SECS of real time before it was called one.
#
# The *tracking* is unconditional even though the stall verdict is opt-in
# (STALL_SECS > 0), because "was the guest still producing output when the
# clock ran out?" is what decides whether a timeout means a hung kernel or
# merely a budget too small for it.  Answering that only when the opt-in
# detector happens to be armed is how a slow boot gets reported as a wedge --
# see the timeout path below.
STALL_LAST_SIZE=-1
STALL_LAST_GROWTH=0
while kill -0 "$QEMU_PID" 2>/dev/null && [ "$ELAPSED" -lt "$TIMEOUT" ]; do
    sleep 1
    ELAPSED=$(( $(date +%s) - WAIT_START_EPOCH ))

    # Anchor to line start: the success marker is printed as a standalone line
    # (`serial_println!("BOOT_OK")`).  An UNanchored match also trips on the
    # livelock diagnostic "...still armed 200s after arming (no BOOT_OK)...",
    # which contains the substring BOOT_OK — a false PASS on a hung boot.
    if [ -f "$SERIAL_FILE" ] && grep -q "^$WAIT_MARKER" "$SERIAL_FILE" 2>/dev/null; then
        echo "$WAIT_MARKER detected after ${ELAPSED}s!"
        kill_qemu "$QEMU_PID"
        if ! check_selftest_failures "$SERIAL_FILE"; then
            echo "=== Boot test FAILED ($WAIT_MARKER reached but a self-test failed) ==="
            exit 1
        fi
        if ! check_liveness_failures "$SERIAL_FILE"; then
            echo "=== Boot test FAILED ($WAIT_MARKER reached but the liveness watchdog reported) ==="
            exit 1
        fi
        if ! check_bench_coverage "$SERIAL_FILE" "$BENCH"; then
            echo "=== Boot test FAILED ($WAIT_MARKER reached but the benchmark suite left windows unjudged) ==="
            exit 1
        fi
        finish_pass "$SERIAL_FILE"
    fi

    # Early death detection.  Checked AFTER the marker so a log that somehow
    # holds both still reports PASS, matching the post-loop elif ordering.
    # The 2s settle lets the panic handler's trailing lines (the Display of
    # PanicInfo, the location) land before we take the emulator away, so the
    # printed evidence is the whole panic and not just its first line.
    if kernel_is_dead "$SERIAL_FILE"; then
        sleep 2
        echo "=== Kernel died after ${ELAPSED}s (not waiting out the ${TIMEOUT}s timeout) ==="
        report_kernel_death "$SERIAL_FILE"
        kill_qemu "$QEMU_PID"
        echo "=== Boot test FAILED ==="
        exit 1
    fi

    # Serial-stall wedge detection (opt-in).  A wedged kernel stops writing to
    # the serial log; a slow-but-healthy boot keeps appending self-test output.
    # If the log has not grown for STALL_SECS seconds and the marker still isn't
    # present, treat it as a genuine hang (distinct from a slow host that would
    # eventually reach the marker) — capture the frozen RIP and exit 2.
    if [ -f "$SERIAL_FILE" ]; then
        cur_size=$(wc -c < "$SERIAL_FILE" 2>/dev/null || echo 0)
        if [ "$cur_size" -ne "$STALL_LAST_SIZE" ]; then
            STALL_LAST_SIZE=$cur_size
            STALL_LAST_GROWTH=$ELAPSED
        elif [ "$STALL_SECS" -gt 0 ] && [ $((ELAPSED - STALL_LAST_GROWTH)) -ge "$STALL_SECS" ]; then
            echo "=== WEDGE: serial output stalled for ${STALL_SECS}s at ${ELAPSED}s (kernel not progressing; $WAIT_MARKER never reached) ==="
            if [ "${#MONITOR_ARGS[@]}" -gt 0 ] && kill -0 "$QEMU_PID" 2>/dev/null; then
                RIPDUMP="${SERIAL_FILE%.txt}-regs.txt"
                capture_guest_state "$MONITOR_PORT" "$RIPDUMP" "Wedged RIP" || true
            fi
            kill_qemu "$QEMU_PID"
            echo "=== Boot test FAILED (WEDGE: serial stalled) ==="
            exit 2
        fi
    fi
done

# Timed out (or QEMU died).  If the diagnostic monitor is attached and QEMU is
# still alive, capture the RIP from the emulator BEFORE we kill it.  This is the
# primary observability tool for the silent BSP-dead hang, which never takes the
# injected NMI in-guest.
#
# Say which kind of timeout this was.  The loop above has been watching the
# serial log grow, so we know the answer rather than having to hedge: a log that
# was still growing when the clock ran out is a kernel that was working, and
# calling its RIP "wedged" sends the reader hunting a hang that never happened.
# That is not hypothetical -- an instrumented (KASAN) boot on 2026-08-19 was
# reported as "Wedged RIP = kasan::byte_bad" while it was in fact 27 000 lines
# deep and still printing; the RIP was simply wherever the sample happened to
# land, and under KASAN that is the shadow checker on nearly every sample.
if [ "${#MONITOR_ARGS[@]}" -gt 0 ] && kill -0 "$QEMU_PID" 2>/dev/null; then
    if ! grep -q "^$WAIT_MARKER" "$SERIAL_FILE" 2>/dev/null; then
        SINCE_GROWTH=$((ELAPSED - STALL_LAST_GROWTH))
        if [ "$STALL_LAST_SIZE" -gt 0 ] && [ "$SINCE_GROWTH" -lt 10 ]; then
            echo "=== Timeout at ${TIMEOUT}s with the guest STILL PRODUCING OUTPUT (serial grew ${SINCE_GROWTH}s ago) ==="
            echo "=== This is a budget that was too small, not a hang. Re-run with a larger --timeout. ==="
            RIP_LABEL="RIP when the clock ran out (guest was live; not a hang)"
        else
            echo "=== Timeout at ${TIMEOUT}s; serial last grew ${SINCE_GROWTH}s ago ($WAIT_MARKER never reached) ==="
            RIP_LABEL="RIP at timeout"
        fi
        RIPDUMP="${SERIAL_FILE%.txt}-regs.txt"
        capture_guest_state "$MONITOR_PORT" "$RIPDUMP" "$RIP_LABEL" || true
    fi
fi

# Clean up
kill_qemu "$QEMU_PID"

# Check final output
if [ -f "$SERIAL_FILE" ]; then
    if grep -q "^$WAIT_MARKER" "$SERIAL_FILE"; then
        echo "$WAIT_MARKER found."
        if ! check_selftest_failures "$SERIAL_FILE"; then
            echo "=== Boot test FAILED ($WAIT_MARKER reached but a self-test failed) ==="
            exit 1
        fi
        if ! check_liveness_failures "$SERIAL_FILE"; then
            echo "=== Boot test FAILED ($WAIT_MARKER reached but the liveness watchdog reported) ==="
            exit 1
        fi
        if ! check_bench_coverage "$SERIAL_FILE" "$BENCH"; then
            echo "=== Boot test FAILED ($WAIT_MARKER reached but the benchmark suite left windows unjudged) ==="
            exit 1
        fi
        finish_pass "$SERIAL_FILE"
    elif grep -q "PANIC\|FATAL" "$SERIAL_FILE"; then
        # Wider (unanchored) net than the in-loop kernel_is_dead check: by this
        # point the boot has already failed to reach the marker, so a loose
        # match cannot turn a healthy boot into a failure.
        report_kernel_death "$SERIAL_FILE"
        echo "=== Boot test FAILED ==="
        exit 1
    fi
fi

# In --bench mode, BENCH_OK is not currently reachable: the deferred
# benchmark task livelocks in bench_pick_next (see known-issues.md "deferred
# benchmark suite hangs after context_switch").  So even on timeout, surface
# whatever benchmark numbers DID get captured — they are still useful for
# spotting regressions in the early benchmarks — before reporting failure.
if [ "$BENCH" -eq 1 ] && [ -f "$SERIAL_FILE" ] && grep -q "^BOOT_OK" "$SERIAL_FILE"; then
    echo "Note: BOOT_OK reached but $WAIT_MARKER did not arrive within ${TIMEOUT}s."
    echo "      (Known issue: the deferred benchmark suite hangs in bench_pick_next."
    echo "       Partial benchmark numbers captured up to the hang are shown below.)"
    print_bench_results "$SERIAL_FILE"
    echo "=== Boot test FAILED ($WAIT_MARKER not reached) ==="
    exit 1
fi

echo "$WAIT_MARKER not found within ${TIMEOUT}s."
# Surface WHERE the boot froze.  On a silent wedge the harness otherwise prints
# only this line, and the operator/next session must manually `tail` the serial
# file — which a subsequent re-run then overwrites, losing the freeze context
# (this exact loss bit the fork+exec-hang investigation, known-issues.md
# B-FORKEXEC-BOOT-HANG).  Echoing the tail to stdout preserves it in the test
# output independently of the serial file.  Pure post-kill log processing: the
# guest is already dead, so this cannot perturb any (timing-sensitive) boot.
if [ -f "$SERIAL_FILE" ]; then
    echo "=== Last 25 serial lines before the wedge (freeze point) ==="
    tail -n 25 "$SERIAL_FILE" || true
    echo "=== (end serial tail) ==="
    # The RIP for this timeout was already captured above, from the emulator,
    # over the always-on HMP monitor — look for the "RIP at timeout" block.
    # The hint below is about the *other* half: an in-guest task-table dump,
    # which needs an NMI to interrupt an IF=0 wedge and therefore needs the
    # opt-in device.  Only worth suggesting when the RIP alone was not enough.
    if [ "$HARD_LOCKUP_WATCHDOG" -eq 0 ]; then
        echo "Hint: the wedged RIP is above (HMP monitor).  If it is not enough,"
        echo "      re-run with --hard-lockup-watchdog for an in-guest NMI task-table"
        echo "      dump as well (see Q20) — note an intermittent hang usually does"
        echo "      not reproduce on the next boot, so prefer reading the RIP first."
    fi
fi
echo "=== Boot test FAILED ==="
exit 1
