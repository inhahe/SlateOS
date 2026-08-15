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
            echo "  (rebuild the image: wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh)"
            ;;
    esac
    return 0
}

# Directories whose contents are performance-critical per CLAUDE.md's
# "Performance-Critical Subsystems" table.  A change under any of these is a
# change that CLAUDE.md requires benchmarking, so it is the trigger for
# nagging about a stale benchmark record.
# Each entry is annotated with the benchmarks it actually guards.  Keep that
# mapping accurate: this list is only useful if it covers everything the suite
# measures, and the failure mode when it does not is SILENT -- an unwatched
# path reports "no perf-critical changes", which is exactly the false negative
# this whole mechanism exists to prevent.
#
# The first version of this list was derived from CLAUDE.md's perf-critical
# table read as *directories*, and it missed more than half the suite: 30+ of
# the 63 benchmarks measured code in idt.rs, fs/, net/ and crypto.rs, none of
# which were listed.  Cross-check against `python scripts/bench-history.py
# --list` / the recorded entry names when adding a benchmark.
BENCH_CRITICAL_PATHS=(
    "kernel/src/mm"           # page_alloc_free, heap_alloc_free_64
    "kernel/src/sched"        # context_switch, pick_next, sched_pick_next
    "kernel/src/ipc"          # ipc_*, futex_wake_empty, shm_*, cp_*,
                              #   io_ring_nop, service_connect
    "kernel/src/syscall"      # syscall_dispatch
    "kernel/src/smp.rs"       # cross-CPU paths behind the above
    "kernel/src/idt.rs"       # isr_latency, page_fault -- CLAUDE.md lists both
                              #   "interrupt dispatch" and "page fault
                              #   handling"; the handlers live here, not in mm/
    "kernel/src/fs"           # vfs_read_256, vfs_write_256, vfs_readdir,
                              #   vfs_stat_{root,3comp,deep},
                              #   vfs_throughput_16k_{read,write}
    "kernel/src/net"          # net_*, tcp_checksum_*, dns_build_query,
                              #   firewall_check, and http_*/dashboard_api_*
                              #   (net/http.rs, net/dashboard.rs)
    "kernel/src/crypto.rs"    # crypto_* (sha256/sha512/hmac/chacha20/poly1305/
                              #   aead/ed25519/x25519)
)

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
    local last_commit=""
    if [ -f "$hist" ]; then
        last_commit="$(sed -n 's/.*"commit"[[:space:]]*:[[:space:]]*"\([0-9a-f]*\)".*/\1/p' "$hist" | tail -1)"
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

    local changed
    changed="$(git -C "$PROJECT_ROOT" diff --name-only "$last_commit" HEAD -- \
        "${BENCH_CRITICAL_PATHS[@]}" 2>/dev/null)"

    if [ -n "$changed" ]; then
        echo "  !! Performance-critical code changed since the last benchmarked commit"
        echo "     ($last_commit). CLAUDE.md requires benchmarking these:"
        echo "$changed" | head -8 | sed 's/^/       /'
        local n
        n="$(echo "$changed" | grep -c .)"
        [ "$n" -gt 8 ] && echo "       ... and $((n - 8)) more"
        echo "     Run: ./scripts/boot-test.sh --bench"
    else
        echo "  No perf-critical changes since the last benchmarked commit ($last_commit),"
        echo "  so skipping the suite is reasonable here."
    fi
    return 0
}

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Convert to Windows paths if running under MSYS/Git Bash (QEMU needs them).
to_win_path() {
    if command -v cygpath &>/dev/null; then
        cygpath -w "$1"
    else
        echo "$1"
    fi
}

# Default (debug) artefact path.  --bench overrides this to the release path
# after arg parsing — see the CARGO_PROFILE_ARGS block below for why.
KERNEL_BIN="$PROJECT_ROOT/target/x86_64-unknown-none/debug/kernel"
ESP_DIR="$PROJECT_ROOT/build/esp"
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
        --timeout=*) TIMEOUT="${arg#*=}"; TIMEOUT_EXPLICIT=1 ;;
        --stall-secs=*) STALL_SECS="${arg#*=}" ;;
        --hard-lockup-watchdog) HARD_LOCKUP_WATCHDOG=1 ;;
    esac
done

# --bench waits for a marker that is emitted long after BOOT_OK, so it needs a
# correspondingly longer budget.  Applied only if the caller did not pick a
# timeout themselves — an explicit --timeout= always wins, in either direction.
if [ "$BENCH" = "1" ] && [ "$TIMEOUT_EXPLICIT" = "0" ]; then
    TIMEOUT="$BENCH_TIMEOUT"
fi

# --bench builds --release; every other run stays on the debug profile.
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
if [ "$BENCH" = "1" ]; then
    CARGO_PROFILE_ARGS=("--release")
    KERNEL_BIN="$PROJECT_ROOT/target/x86_64-unknown-none/release/kernel"
    BENCH_PROFILE="release"
else
    CARGO_PROFILE_ARGS=()
    BENCH_PROFILE="debug"
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

# Capture the frozen guest CPU state over the HMP monitor socket, then resolve
# RIP to a kernel symbol.  Called on timeout (guest still running) so the RIP is
# the wedged instruction pointer.  Best-effort: prints a warning and returns
# non-zero if the monitor is unreachable or the shell lacks /dev/tcp support.
#
# Args: $1 = monitor TCP port, $2 = output file for the raw register dump.
capture_guest_state() {
    local port="$1" out="$2"
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
        echo "  Wedged RIP = 0x$rip"
        resolve_kernel_symbol "$rip"
    else
        echo "  (no RIP= line in monitor output; see $out)"
    fi
    return 0
}

# Resolve a hex address to the nearest preceding kernel symbol.
#
# There is no addr2line/llvm-symbolizer in any installed toolchain on this box,
# only llvm-nm/llvm-objdump.  So we do nearest-symbol resolution ourselves:
# dump the sorted defined symbol table with llvm-nm and pick the last symbol
# whose address is <= RIP (that is the function the RIP lies within).  This is
# exactly what addr2line's symbol column would report, minus line numbers.
resolve_kernel_symbol() {
    local rip="$1"
    if [ ! -f "$KERNEL_BIN" ]; then
        echo "  (kernel ELF missing; resolve 0x$rip manually)"
        return 1
    fi
    # Locate an llvm-nm: PATH first, then any rustup toolchain sysroot bin.
    local nm=""
    if command -v llvm-nm &>/dev/null; then
        nm="llvm-nm"
    else
        local sr
        sr="$(rustc --print sysroot 2>/dev/null || true)"
        if [ -n "$sr" ]; then
            local cand
            cand="$(ls "$sr"/lib/rustlib/*/bin/llvm-nm* 2>/dev/null | head -n1 || true)"
            [ -n "$cand" ] && nm="$cand"
        fi
    fi
    if [ -z "$nm" ]; then
        echo "  (no llvm-nm found; resolve 0x$rip manually against $KERNEL_BIN)"
        return 1
    fi
    # llvm-nm -nC: numeric-sort, demangled.  Rows: "<hexaddr> <type> <name>".
    # awk finds the last defined symbol with addr <= target.  We compare
    # zero-padded 16-digit hex STRINGS (lexicographic == numeric for equal
    # length) rather than strtonum(), because higher-half kernel addresses
    # (~1.8e19) exceed a double's 2^53 exact-integer range and would compare
    # imprecisely.  awk emits "<name>\t<besta_hex>"; bash computes the byte
    # offset in exact 64-bit arithmetic.
    local row name besta
    row="$("$nm" -nC --defined-only "$KERNEL_BIN" 2>/dev/null | awk -v tgt="$rip" '
        function pad(h,  n){ h = tolower(h); n = 16 - length(h); while (n-- > 0) h = "0" h; return h }
        BEGIN { t = pad(tgt); best = ""; besta = "" }
        NF >= 3 && $1 ~ /^[0-9a-fA-F]+$/ {
            a = pad($1)
            if (a <= t && a >= besta) {
                besta = a
                araw = $1
                $1 = ""; $2 = ""; sub(/^  */, "")
                best = $0
            }
        }
        END { if (best != "") printf "%s\t%s", best, araw }
    ')"
    name="${row%$'\t'*}"
    besta="${row##*$'\t'}"
    if [ -n "$name" ] && [ -n "$besta" ]; then
        # Exact 64-bit offset (bash arithmetic is 64-bit; both operands share
        # the sign bit in the higher half, so the difference is a small +ve).
        local off
        off="$(( 0x$rip - 0x$besta ))"
        printf '  Symbol: %s (+0x%x)\n' "$name" "$off"
    else
        echo "  (0x$rip below all symbols — likely userspace/ring-3 RIP, not kernel)"
    fi
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
    local rc=0
    if command -v python &>/dev/null; then
        python "$SCRIPT_DIR/bench-history.py" --serial "$file" --profile "$BENCH_PROFILE" || rc=$?
    elif command -v python3 &>/dev/null; then
        python3 "$SCRIPT_DIR/bench-history.py" --serial "$file" --profile "$BENCH_PROFILE" || rc=$?
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
    report_pathz_skips "$file"

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

# Step 1: Build
if [ "$NO_BUILD" -eq 0 ]; then
    echo "=== Building kernel ==="
    CARGO="${CARGO:-cargo}"
    # Try full path on Windows if cargo not in PATH
    if ! command -v "$CARGO" &>/dev/null; then
        CARGO="/c/Users/${USER:-${USERNAME:-$(whoami)}}/.cargo/bin/cargo.exe"
    fi
    (cd "$PROJECT_ROOT" && "$CARGO" build ${CARGO_PROFILE_ARGS[@]+"${CARGO_PROFILE_ARGS[@]}"})
    echo "Build OK ($BENCH_PROFILE profile)."
fi

if [ "$NO_STAGE" -eq 0 ] && [ ! -f "$KERNEL_BIN" ]; then
    echo "ERROR: Kernel binary not found at $KERNEL_BIN" >&2
    exit 1
fi

# Step 2: Stage boot files
echo "=== Staging boot files ==="
mkdir -p "$ESP_DIR/EFI/BOOT" "$ESP_DIR/boot"
cp "$PROJECT_ROOT/limine/BOOTX64.EFI" "$ESP_DIR/EFI/BOOT/BOOTX64.EFI"

# Strip debug symbols — the unstripped debug binary is ~280 MiB.  Stripping
# removes the symbol table + .debug_* sections (~80 MiB), but the staged image
# is still large because it carries a big .rodata payload: ~47 fastpy self-test
# ELFs are embedded into the kernel via include_bytes! (~3.5 MiB each → ~165 MiB
# of .rodata that stripping CANNOT remove — it's genuine program data).  Limine
# must load that whole image into high memory, so the QEMU RAM below (-m) has to
# comfortably exceed the staged kernel size (see the "-m" note near the QEMU
# invocation).  We try llvm-strip (ships with rustup) first, falling back to a
# plain copy if no strip tool is found.
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
    echo "Stripping kernel binary with $LLVM_STRIP..."
    if "$LLVM_STRIP" "$KERNEL_BIN" -o "$STAGED_KERNEL"; then
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

# Step 3: Create a small swap disk image (16 MiB) for disk-backed swap testing.
SWAP_IMG="$PROJECT_ROOT/build/swap.img"
SWAP_IMG_WIN="$(to_win_path "$SWAP_IMG")"
if [ ! -f "$SWAP_IMG" ]; then
    echo "=== Creating 16 MiB swap disk image ==="
    dd if=/dev/zero of="$SWAP_IMG" bs=1M count=16 status=none 2>/dev/null
fi

# Step 3b: Attach the Path-Z glibc rootfs (rootfs.ext4) as a second virtio-blk
# disk when present.  It is enumerated AFTER swap-disk, so it becomes vdb: the
# kernel's swap loop skips it (ext4 superblock detected) and the /mnt ext4 probe
# mounts it, enabling the real-glibc dynamic-execution self-test.  Built on the
# dev box via `wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh`; git-ignored,
# so the boot test simply omits it (and the self-test no-ops) when it is absent.
ROOTFS_IMG="$PROJECT_ROOT/rootfs.ext4"
ROOTFS_ARGS=()
if [ -f "$ROOTFS_IMG" ]; then
    ROOTFS_IMG_WIN="$(to_win_path "$ROOTFS_IMG")"
    ROOTFS_ARGS=(
        -device virtio-blk-pci,drive=rootfs-disk
        -drive "id=rootfs-disk,if=none,format=raw,file=$ROOTFS_IMG_WIN"
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
QEMU_CPU="${QEMU_CPU:-qemu64,+smep,+smap,+umip}"

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

# Release is idempotent and safe to call when we never acquired: we only remove
# the lock if the owner file still names THIS process, so we can never delete a
# lock that another lane acquired after we broke/released ours.
release_boot_lock() {
    [ -n "$BOOT_LOCK_DIR" ] || return 0
    [ -d "$BOOT_LOCK_DIR" ] || return 0
    if [ "$(cat "$BOOT_LOCK_DIR/owner" 2>/dev/null || echo "")" = "$BOOT_LOCK_OWNER" ]; then
        rm -rf "$BOOT_LOCK_DIR" 2>/dev/null || true
    fi
}

if [ -n "$BOOT_LOCK_DIR" ]; then
    BOOT_LOCK_OWNER="$(python "$PROJECT_ROOT/scripts/which-lane.py" 2>/dev/null | awk '/^lane:/{print $2}' || true)"
    BOOT_LOCK_OWNER="lane-${BOOT_LOCK_OWNER:-?}/pid-$$/$(date +%s)"
    _lock_wait="${BOOT_LOCK_WAIT:-3600}"
    _lock_waited=0
    while ! mkdir "$BOOT_LOCK_DIR" 2>/dev/null; do
        # Break a stale lock: >20 min old means the holder died without
        # releasing (hard kill, power loss).  20 min > our longest healthy
        # boot (~8 min) with a wide margin, so this cannot steal a live lock.
        _lock_age=999999
        if [ -f "$BOOT_LOCK_DIR/owner" ]; then
            _lock_mtime="$(date -r "$BOOT_LOCK_DIR/owner" +%s 2>/dev/null || echo 0)"
            [ "$_lock_mtime" -gt 0 ] && _lock_age=$(( $(date +%s) - _lock_mtime ))
        fi
        if [ "$_lock_age" -gt 1200 ]; then
            echo "=== Breaking stale boot lock (age ${_lock_age}s, held by $(cat "$BOOT_LOCK_DIR/owner" 2>/dev/null || echo unknown)) ==="
            rm -rf "$BOOT_LOCK_DIR" 2>/dev/null || true
            continue
        fi
        if [ "$_lock_waited" -ge "$_lock_wait" ]; then
            echo "=== Boot lock still held after ${_lock_waited}s; booting anyway (results may be slow) ==="
            BOOT_LOCK_DIR=""
            break
        fi
        if [ $(( _lock_waited % 60 )) -eq 0 ]; then
            echo "=== Waiting for boot lock, held by $(cat "$BOOT_LOCK_DIR/owner" 2>/dev/null || echo unknown) (${_lock_waited}s) ==="
        fi
        sleep 5
        _lock_waited=$(( _lock_waited + 5 ))
    done
    if [ -n "$BOOT_LOCK_DIR" ]; then
        echo "$BOOT_LOCK_OWNER" > "$BOOT_LOCK_DIR/owner" 2>/dev/null || true
        trap 'release_boot_lock' EXIT INT TERM
        echo "=== Boot lock acquired: $BOOT_LOCK_OWNER ==="
    fi
fi

# Step 4: Boot QEMU
echo "=== Booting QEMU (timeout: ${TIMEOUT}s, cpu: $QEMU_CPU) ==="
rm -f "$SERIAL_FILE"

OVMF_WIN="$(to_win_path "$OVMF")"
rm -f "$PIDFILE"
"$QEMU" \
    -drive "if=pflash,format=raw,readonly=on,file=$OVMF_WIN" \
    -drive "format=raw,file=fat:rw:$ESP_DIR_WIN" \
    -device virtio-blk-pci,drive=swap-disk \
    -drive "id=swap-disk,if=none,format=raw,file=$SWAP_IMG_WIN" \
    "${ROOTFS_ARGS[@]}" \
    "${WATCHDOG_ARGS[@]}" \
    "${MONITOR_ARGS[@]}" \
    -device virtio-gpu-pci \
    -serial "file:$SERIAL_FILE_WIN" \
    -pidfile "$PIDFILE_WIN" \
    -display none \
    -no-reboot \
    -m 3072M \
    -cpu "$QEMU_CPU" \
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
trap 'kill_qemu "$QEMU_PID"; release_boot_lock' EXIT INT TERM

# Wait for BOOT_OK or timeout
ELAPSED=0
# Serial-stall tracking (only acted on when STALL_SECS > 0).  We remember the
# serial log's last observed size and the elapsed time at which it last grew;
# if (ELAPSED - last-growth) reaches STALL_SECS the kernel has gone silent.
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
        finish_pass "$SERIAL_FILE"
    fi

    # Serial-stall wedge detection (opt-in).  A wedged kernel stops writing to
    # the serial log; a slow-but-healthy boot keeps appending self-test output.
    # If the log has not grown for STALL_SECS seconds and the marker still isn't
    # present, treat it as a genuine hang (distinct from a slow host that would
    # eventually reach the marker) — capture the frozen RIP and exit 2.
    if [ "$STALL_SECS" -gt 0 ] && [ -f "$SERIAL_FILE" ]; then
        cur_size=$(wc -c < "$SERIAL_FILE" 2>/dev/null || echo 0)
        if [ "$cur_size" -ne "$STALL_LAST_SIZE" ]; then
            STALL_LAST_SIZE=$cur_size
            STALL_LAST_GROWTH=$ELAPSED
        elif [ $((ELAPSED - STALL_LAST_GROWTH)) -ge "$STALL_SECS" ]; then
            echo "=== WEDGE: serial output stalled for ${STALL_SECS}s at ${ELAPSED}s (kernel not progressing; $WAIT_MARKER never reached) ==="
            if [ "${#MONITOR_ARGS[@]}" -gt 0 ] && kill -0 "$QEMU_PID" 2>/dev/null; then
                RIPDUMP="${SERIAL_FILE%.txt}-regs.txt"
                capture_guest_state "$MONITOR_PORT" "$RIPDUMP" || true
            fi
            kill_qemu "$QEMU_PID"
            echo "=== Boot test FAILED (WEDGE: serial stalled) ==="
            exit 2
        fi
    fi
done

# Timed out (or QEMU died): the guest may be wedged.  If the diagnostic monitor
# is attached and QEMU is still alive, capture the frozen RIP from the emulator
# BEFORE we kill it.  This is the primary observability tool for the silent
# BSP-dead hang, which never takes the injected NMI in-guest.
if [ "${#MONITOR_ARGS[@]}" -gt 0 ] && kill -0 "$QEMU_PID" 2>/dev/null; then
    if ! grep -q "^$WAIT_MARKER" "$SERIAL_FILE" 2>/dev/null; then
        echo "=== Timeout with guest still running: capturing wedged RIP via HMP monitor ==="
        RIPDUMP="${SERIAL_FILE%.txt}-regs.txt"
        capture_guest_state "$MONITOR_PORT" "$RIPDUMP" || true
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
        finish_pass "$SERIAL_FILE"
    elif grep -q "PANIC\|FATAL" "$SERIAL_FILE"; then
        echo "KERNEL PANIC detected!"
        grep "PANIC\|FATAL\|EXCEPTION" "$SERIAL_FILE" || true
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
