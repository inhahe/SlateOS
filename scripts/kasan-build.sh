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
# Requires a nightly toolchain: `-Zsanitizer` and the `sanitize` attribute are
# both unstable.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

TOOLCHAIN="${KASAN_TOOLCHAIN:-nightly}"
BOOT=0
PROFILE_ARGS=()

for arg in "$@"; do
    case "$arg" in
        --boot)    BOOT=1 ;;
        --release) PROFILE_ARGS+=("--release") ;;
        -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
        *) echo "unknown argument: $arg" >&2; exit 2 ;;
    esac
done

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
    -Cllvm-args=-asan-stack=0
    -Cllvm-args=-asan-globals=0
    --cfg kasan_instrumented
)

echo "=== Building kernel with KASAN instrumentation (+$TOOLCHAIN) ==="
cd "$PROJECT_ROOT"
cargo "+$TOOLCHAIN" rustc -p kernel ${PROFILE_ARGS[@]+"${PROFILE_ARGS[@]}"} -- "${KASAN_ARGS[@]}"
echo "Build OK (instrumented)."

if [ "$BOOT" -eq 1 ]; then
    echo "=== Booting the instrumented kernel ==="
    exec "$SCRIPT_DIR/boot-test.sh" --no-build
fi
