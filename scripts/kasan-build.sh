#!/usr/bin/env bash
# kasan-build.sh — build the kernel with LLVM's KernelAddressSanitizer.
#
# This is a *debug profile*, not the shipping build: it exists to root-cause
# heap corruption (B-KNULLJUMP) by having the compiler check every load and
# store against the KASAN shadow, instead of only the allocator-boundary checks
# the ordinary build performs.  Nothing about the normal build changes; you get
# an instrumented kernel binary at the usual path, and `./scripts/boot-test.sh
# --no-build` will boot whatever is there.
#
# Usage:
#   ./scripts/kasan-build.sh              # build only
#   ./scripts/kasan-build.sh --boot       # build, then boot-test the result
#   ./scripts/kasan-build.sh --release    # optimized instrumented build
#
#   ./scripts/kasan-build.sh --boot -- --hard-lockup-watchdog --stall-secs=180
#       everything after `--` is forwarded verbatim to boot-test.sh
#
# The `--` pass-through is not a convenience.  An instrumented boot runs ~20x
# slower and is the profile *most* likely to wedge, so it is the one that most
# needs boot-test.sh's diagnostic options — yet without pass-through this script
# could only ever invoke the bare default.  That is precisely how
# B-KASAN-INSTRUMENTED-BOOT-WEDGES-MID-PRINT-ON-A-PAGE-FAULT was captured with
# no RIP: the HMP monitor is only attached under --hard-lockup-watchdog, and
# there was no way to ask for it.
#
# Requires a nightly toolchain: `-Zsanitizer` and the `sanitize` attribute are
# both unstable.
#
# ---------------------------------------------------------------------------
# The instrumented build turns KASAN on for the whole boot, by itself
# ---------------------------------------------------------------------------
#
# You do not need `mm.corruption_hunt` on the kernel cmdline, and you should not
# add it expecting more coverage: `mm::kasan::init` sees `--cfg
# kasan_instrumented` and enables checking at init, for the entire boot.
#
# This was not always so, and the failure mode was silent. `on_alloc`/`on_free`
# are gated on `kasan::is_enabled()`, and until 2026-08-19 the only thing that
# ever enabled it was the narrow `mm.corruption_hunt` window in `main.rs`. So an
# ordinary `kasan-build.sh --boot` paid the full ~3.5x instrumented boot cost to
# check every load and store against a shadow that nothing had ever written —
# all-zero, "addressable" everywhere, no report possible. The script did exactly
# what it said and found nothing, because there was nothing it *could* find.
#
# The `mm.corruption_hunt` flag still means what it always did in the *ordinary*
# build, where checking is opt-in because the cost is not otherwise being paid.
# What it additionally arms here is `mm::quarantine` (delayed slot reuse), which
# is orthogonal to the shadow and still worth passing for a corruption hunt.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

TOOLCHAIN="${KASAN_TOOLCHAIN:-nightly}"
BOOT=0
PROFILE_ARGS=()
BOOT_ARGS=()

# Everything before `--` is ours; everything after is boot-test.sh's.  Use a
# `while`/`shift` loop rather than `for arg in "$@"` so the separator can stop
# the scan — a `for` loop cannot break out and collect the remainder.
while [ "$#" -gt 0 ]; do
    case "$1" in
        --)        shift; BOOT_ARGS=("$@"); break ;;
        --boot)    BOOT=1 ;;
        --release) PROFILE_ARGS+=("--release") ;;
        # 2..49 is the whole header comment block, ending at the blank line
        # before `set -euo pipefail`. Keep this in step when the header grows —
        # a stale upper bound silently truncates the help halfway through a
        # sentence, which is how the "turns KASAN on by itself" paragraph would
        # be the first thing lost.
        -h|--help) sed -n '2,49p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done

if [ "${#BOOT_ARGS[@]}" -gt 0 ] && [ "$BOOT" -eq 0 ]; then
    echo "error: arguments after \`--\` are for boot-test.sh, but --boot was not given" >&2
    exit 2
fi

# ---------------------------------------------------------------------------
# Why `cargo rustc` and not RUSTFLAGS
# ---------------------------------------------------------------------------
#
# A `RUSTFLAGS` environment variable *replaces* the `[target.x86_64-unknown-none]
# rustflags` in .cargo/config.toml rather than adding to them, so it would
# silently drop `-C code-model=kernel`, `-C relocation-model=static` and
# `-C force-frame-pointers=yes` — the kernel would build and then fault in
# unrecognisable ways.  `cargo rustc -- <flags>` appends to the configured
# flags, and only for the final crate, which is exactly the scope we want:
# instrument the kernel, leave precompiled core/alloc alone.
#
# ---------------------------------------------------------------------------
# The LLVM arguments
# ---------------------------------------------------------------------------
#
# -asan-mapping-offset=0xDFFFE00000000000
#     shadow(addr) = (addr >> 3) + offset.  Derived in mm::kvspace so that the
#     lowest kernel address maps to the base of the KASAN_SHADOW reservation;
#     see KASAN_SHADOW_OFFSET there for the arithmetic.  Supplying an explicit
#     offset is not optional: LLVM's *default* mapping is an OR
#     (`(addr >> 3) | 0x100000000000`), which is only meaningful below 2^47 and
#     therefore cannot express a higher-half kernel address at all.  Passing an
#     offset switches codegen to an ADD.  Linux does the same thing with
#     0xDFFFFC0000000000.
#
# -asan-mapping-scale=3
#     One shadow byte per 8 bytes.  Must match KASAN_GRANULE_SHIFT.
#
# -asan-recover=1
#     Emit `_noabort` report calls and no trailing `ud2`, so a detected bad
#     access is reported and then *performed*, and the kernel keeps running.
#     Aborting on the first hit would tell us about one corruption; recovering
#     lets a single boot enumerate all of them.  Linux uses the same mode.
#
# -asan-instrumentation-with-call-threshold=0
#     Emit a *call* to `__asan_load8_noabort` & co. for every checked access
#     instead of the inline shadow-compare sequence ("outline" mode; Linux
#     offers the same choice as CONFIG_KASAN_OUTLINE).  This is not a size or
#     taste preference — it is a correctness requirement for this kernel.
#
#     The inline sequence computes `(addr >> 3) + offset` and dereferences it
#     unconditionally.  That is only a valid address for a *kernel* address: the
#     shadow of a user address is `shadow(0x4000000000) = 0xDFFFE00800000000`,
#     whose bits 63:47 are not sign-extended, so it is non-canonical and the
#     probe is a #GP rather than a report.  Kernel code legitimately dereferences
#     user pointers (the SEH context written onto the faulting thread's user
#     stack in `idt.rs`, every uaccess helper in `mm/user.rs`, …), so with inline
#     checks every such site is a kernel panic that has nothing to do with the
#     bug being hunted — and each one costs a full boot cycle to find.
#
#     Outlined, the address never reaches a shadow dereference unless
#     `mm::kasan::shadow_of` maps it into the *backed* window; a user address
#     returns `None` and the check passes.  One rule, applied by construction,
#     instead of an open-ended hunt for raw user derefs.
#
#     The cost is a call per access.  This profile is already `-O0` with a check
#     on every load and store; a debug build whose purpose is one bug hunt can
#     pay it.
#
# -asan-stack=0 -asan-globals=0
#     No redzones around stack slots or statics.  Accesses to them are still
#     checked — this only turns off the *poisoning*.  Poisoning stack redzones
#     means LLVM emitting shadow *stores* on every function entry and exit,
#     which would require real writable shadow over every kernel, AP and boot
#     stack rather than the shared read-only zero page, i.e. a completely
#     different (and much more expensive) bootstrap.  Heap redzones — the ones
#     that matter for the bug being hunted — are poisoned by mm::kasan itself
#     and are unaffected.
#
# -Cunsafe-allow-abi-mismatch=sanitizer
#     Links the instrumented kernel against the *uninstrumented* precompiled
#     core/alloc instead of requiring -Zbuild-std.  Sound here because ASan
#     does not change calling conventions; the practical consequence is only
#     that non-generic precompiled functions go unchecked.  Generic `alloc`
#     code (Vec, BTreeMap, …) monomorphises into the kernel crate and *is*
#     instrumented.
#
# --cfg kasan_instrumented
#     Turns on `#![feature(sanitize)]` and the `sanitize(address = "off")`
#     exemptions in the kernel source.  It has to be a --cfg passed alongside
#     these flags rather than a Cargo feature, because the exemptions are only
#     valid when the sanitizer flags are actually present.

KASAN_ARGS=(
    -Zsanitizer=kernel-address
    -Cunsafe-allow-abi-mismatch=sanitizer
    -Cllvm-args=-asan-mapping-offset=0xDFFFE00000000000
    -Cllvm-args=-asan-mapping-scale=3
    -Cllvm-args=-asan-recover=1
    -Cllvm-args=-asan-instrumentation-with-call-threshold=0
    -Cllvm-args=-asan-stack=0
    -Cllvm-args=-asan-globals=0
    --cfg kasan_instrumented
)

echo "=== Building kernel with KASAN instrumentation (+$TOOLCHAIN) ==="
cd "$PROJECT_ROOT"
cargo "+$TOOLCHAIN" rustc -p kernel ${PROFILE_ARGS[@]+"${PROFILE_ARGS[@]}"} -- "${KASAN_ARGS[@]}"
echo "Build OK (instrumented)."

# ---------------------------------------------------------------------------
# Verify the pre-shadow window
# ---------------------------------------------------------------------------
#
# Between the entry point and the end of `mm::kasan::install_zero_shadow` there
# is no shadow to read and no IDT to catch the resulting fault, so one
# instrumented access is a triple fault: QEMU resets, the kernel prints nothing
# at all, and the boot test can only report "no BOOT_OK".  Source review does
# not establish this — `sanitize(address = "off")` is per-function and does not
# reach the generic `core` code that monomorphises into this crate — so it is
# checked mechanically against the binary instead.  Failing here costs a
# message; failing at boot costs a `-d int,cpu_reset` session.
KERNEL_BIN="$PROJECT_ROOT/target/x86_64-unknown-none/debug/kernel"
if [ ${#PROFILE_ARGS[@]} -gt 0 ]; then
    KERNEL_BIN="$PROJECT_ROOT/target/x86_64-unknown-none/release/kernel"
fi
python "$SCRIPT_DIR/kasan-check-preshadow.py" "$KERNEL_BIN"

# Timeout for an instrumented boot, used unless the caller passed --timeout=.
#
# boot-test.sh's 900s default is calibrated on an uninstrumented kernel, and an
# instrumented one is not a little slower -- every load and store in the kernel
# grows a shadow-byte lookup first.
#
# Measured 2026-08-19, same host, same QEMU, back to back:
#
#   uninstrumented   BOOT_OK at 285s
#   instrumented     killed at the 900s default having reached serial line
#                    26410 of the 27497 an uninstrumented boot prints before
#                    BOOT_OK -- 96% of the way through, needing ~938s
#
# So the documented command in this script's own header ("--boot") was certain
# to fail, and it failed in the most expensive way available: the harness had a
# monitor attached, sampled the RIP of a perfectly healthy guest, found it (of
# course) inside the KASAN shadow checker, and reported
# "Wedged RIP = kernel::mm::kasan::byte_bad". A boot that missed the finish line
# by under a minute was read as a kernel hang in the sanitizer.
#
# 3600 is ~3.8x the measured 938s. The margin is deliberately generous: the
# cost of being wrong upward is waiting, and the cost of being wrong downward
# is the paragraph above.  This is the same reasoning, and the same shape of
# bug, as BENCH_TIMEOUT in boot-test.sh.
KASAN_BOOT_TIMEOUT=3600

if [ "$BOOT" -eq 1 ]; then
    # An explicit --timeout= from the caller always wins.
    boot_timeout_given=0
    for arg in ${BOOT_ARGS[@]+"${BOOT_ARGS[@]}"}; do
        case "$arg" in --timeout=*) boot_timeout_given=1 ;; esac
    done
    if [ "$boot_timeout_given" -eq 0 ]; then
        BOOT_ARGS+=("--timeout=$KASAN_BOOT_TIMEOUT")
        echo "=== Instrumented boot: raising timeout to ${KASAN_BOOT_TIMEOUT}s (measured need ~938s vs 285s uninstrumented) ==="
    fi
    echo "=== Booting the instrumented kernel ==="
    exec "$SCRIPT_DIR/boot-test.sh" --no-build ${BOOT_ARGS[@]+"${BOOT_ARGS[@]}"}
fi
