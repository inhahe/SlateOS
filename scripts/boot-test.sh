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
#
# 2 and 3 are listed here because they were not: exit 2 has existed since the
# stall detector landed and this header still claimed the script only ever
# returned 0 or 1, so any caller branching on the documented set treated a wedge
# as an ordinary failure.  A status a caller cannot know about is a status the
# caller cannot handle.
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
    "kernel/src/fs/vfs.rs|vfs_read_256 vfs_write_256 vfs_readdir vfs_stat_root vfs_stat_3comp vfs_stat_deep vfs_throughput_16k_read vfs_throughput_16k_write vfs_stat_breakdown_full vfs_stat_breakdown_prologue vfs_stat_breakdown_resolve vfs_stat_breakdown_resolved"
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

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

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
# reliably kills a native Windows qemu from MSYS.  Falls back to killing by
# image name only if the pidfile is missing (should not happen).  Idempotent.
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
    if [ -f "$PIDFILE" ]; then
        local win_pid
        win_pid="$(tr -cd '0-9' < "$PIDFILE" 2>/dev/null || true)"
        if [ -n "$win_pid" ]; then
            taskkill //F //PID "$win_pid" >/dev/null 2>&1 || true
        fi
    fi
    # Reap the Cygwin-side child so the shell doesn't leave a zombie/handle.
    [ -n "$cyg_pid" ] && wait "$cyg_pid" 2>/dev/null || true
    rm -f "$PIDFILE" 2>/dev/null || true
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
# Measured: BOOT_OK at ~456s (2026-08-14, TCG, qemu64).  Re-measure and raise
# this when the self-test suite grows; override with --timeout= for slower
# hosts.
TIMEOUT=900
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
        --host-load=*) HOST_LOAD="${arg#*=}" ;;
        --min-free-gb=*) MIN_FREE_GB="${arg#*=}" ;;
        --min-free-temp-gb=*) MIN_FREE_TEMP_GB="${arg#*=}" ;;
        --reclaim-space) RECLAIM_SPACE=1 ;;
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
# Diagnostic HMP monitor for capturing the wedged guest RIP on timeout.  Only
# attached alongside the hard-lockup watchdog (i.e. deliberate hang-repro runs),
# so the default harness command line is byte-for-byte unchanged.  On timeout we
# query `info registers`/`info cpus` over this socket BEFORE killing QEMU, which
# captures the frozen CPU's RIP directly from the emulator — bypassing in-guest
# NMI delivery entirely (the silent BSP-dead wedge never takes the injected NMI,
# so the in-guest handler dump is blind; the emulator's own view is not).
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

if [ -n "${MONITOR_PORT:-}" ]; then
    MONITOR_PORT_SRC="env override"
else
    MONITOR_PORT="$(pick_monitor_port 57000)"
    MONITOR_PORT_SRC="auto-selected (excluded-range aware)"
fi
if [ "$HARD_LOCKUP_WATCHDOG" -eq 1 ]; then
    WATCHDOG_ARGS=(-device i6300esb,id=hwdog0 -action "watchdog=$WATCHDOG_ACTION")
    MONITOR_ARGS=(-monitor "tcp:127.0.0.1:$MONITOR_PORT,server,nowait")
    echo "=== Hard-lockup watchdog ENABLED (i6300esb -> $WATCHDOG_ACTION) ==="
    echo "=== Diagnostic HMP monitor ENABLED (tcp:127.0.0.1:$MONITOR_PORT, $MONITOR_PORT_SRC) ==="
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
    if [ -n "${BT_SRC_DIGEST:-}" ]; then
        args+=(--src-digest "$BT_SRC_DIGEST")
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

    echo "=== Boot test PASSED ==="
    exit 0
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
    if "$py" "$PROJECT_ROOT/scripts/check-self-tests-wired.py"; then
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

    echo "=== Checking for guards held across a re-acquiring call ==="
    if "$py" "$PROJECT_ROOT/scripts/check-recursive-locks.py"; then
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
    if "$py" "$PROJECT_ROOT/scripts/check-vfs-under-lock.py"; then
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
    if "$py" "$PROJECT_ROOT/scripts/check-user-access-sites.py"; then
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
    if "$py" "$PROJECT_ROOT/scripts/check-vfs-permission-gate.py"; then
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

    echo "=== Checking for unwrap/expect in kernel production paths ==="
    if "$py" "$PROJECT_ROOT/scripts/scan-unwrap.py" --summary; then
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

# Step 1: Build
if [ "$NO_BUILD" -eq 0 ]; then
    check_free_space "before building"
    echo "=== Building kernel ==="
    CARGO="${CARGO:-cargo}"
    # Try full path on Windows if cargo not in PATH
    if ! command -v "$CARGO" &>/dev/null; then
        CARGO="/c/Users/${USER:-${USERNAME:-$(whoami)}}/.cargo/bin/cargo.exe"
    fi
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
# A separate epoch stamp rather than reusing ELAPSED: ELAPSED counts `sleep 1`
# iterations plus the loop body's own work, so it drifts upward on exactly the
# busy hosts whose measurement matters most.
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
    -machine q35 &
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
ELAPSED=0
# Serial-stall tracking.  We remember the serial log's last observed size and
# the elapsed time at which it last grew; if (ELAPSED - last-growth) reaches
# STALL_SECS the kernel has gone silent.
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
    ELAPSED=$((ELAPSED + 1))

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
    if [ "$HARD_LOCKUP_WATCHDOG" -eq 0 ]; then
        echo "Hint: re-run with --hard-lockup-watchdog to capture the wedged"
        echo "      guest RIP via the i6300esb NMI + HMP monitor (see Q20)."
    fi
fi
echo "=== Boot test FAILED ==="
exit 1
