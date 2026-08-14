#!/usr/bin/env bash
# bootstrap-worktree.sh — build the service binaries the kernel embeds.
#
# WHY THIS EXISTS
#
# `kernel/src/main.rs` and `kernel/src/proc/spawn.rs` pull six ring-3
# binaries into the kernel image with `include_bytes!`, and the paths they
# use point into each service's *build output* directory:
#
#     include_bytes!("../../services/init/target/x86_64-unknown-none/release/init")
#
# Those `target/` directories are git-ignored build artifacts, so a freshly
# created worktree (or a fresh clone) does not have them and `cargo build -p
# kernel` fails with fifteen copies of:
#
#     error: couldn't read `.../services/init/target/.../release/init`:
#            The system cannot find the path specified. (os error 3)
#
# The failure is confusing because it points at the kernel, which is not
# what is missing. Run this once per worktree and the kernel builds.
#
# WHY THE SERVICES CANNOT JUST BE WORKSPACE MEMBERS
#
# They are `no_std`/`no_main` ring-3 binaries with their own linker script,
# static relocation model and large code model (see each service's
# `.cargo/config.toml`). The kernel workspace's own rustflags — kernel code
# model, kernel linker script — are incompatible, and cargo's config
# hierarchy is CWD-based, so the only reliable way to get a service's own
# flags is to invoke cargo from inside that service's directory. That is
# exactly what this script does, and it is why `scripts/build-userspace.ps1`
# takes the same approach for `userspace/init` and `userspace/hello`.
#
# USAGE
#
#     ./scripts/bootstrap-worktree.sh              # build all six
#     ./scripts/bootstrap-worktree.sh netstack     # build just one
#     ./scripts/bootstrap-worktree.sh --check      # report what is missing
#
# Safe to re-run: cargo no-ops when a service is already up to date, so this
# is cheap enough to run before any kernel build if you are unsure.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"

# The services the kernel embeds. Keep in sync with the `include_bytes!`
# calls in kernel/src/main.rs and kernel/src/proc/spawn.rs — the --check
# mode below verifies the produced set, but it cannot discover a service
# that was added to the kernel and not added here.
SERVICES=(init hello ticker netstack httpget udpget)

# Every service builds for the bare-metal ring-3 triple, not the kernel's
# custom target: these are ordinary user programs, not kernel code.
TRIPLE="x86_64-unknown-none"

artifact_for() {
    echo "$ROOT/services/$1/target/$TRIPLE/release/$1"
}

check_only=0
if [ "${1-}" = "--check" ]; then
    check_only=1
    shift
fi

if [ $# -gt 0 ]; then
    requested=("$@")
    for name in "${requested[@]}"; do
        found=0
        for known in "${SERVICES[@]}"; do
            [ "$name" = "$known" ] && found=1 && break
        done
        if [ "$found" -eq 0 ]; then
            echo "error: unknown service '$name'" >&2
            echo "known services: ${SERVICES[*]}" >&2
            exit 2
        fi
    done
else
    requested=("${SERVICES[@]}")
fi

if [ "$check_only" -eq 1 ]; then
    missing=0
    for name in "${requested[@]}"; do
        path="$(artifact_for "$name")"
        if [ -f "$path" ]; then
            printf '  present  %s\n' "$name"
        else
            printf '  MISSING  %s  (%s)\n' "$name" "$path"
            missing=$((missing + 1))
        fi
    done
    if [ "$missing" -gt 0 ]; then
        echo ""
        echo "$missing service binar$([ "$missing" -eq 1 ] && echo y || echo ies) missing;"
        echo "run ./scripts/bootstrap-worktree.sh to build them."
        exit 1
    fi
    echo ""
    echo "All embedded service binaries present."
    exit 0
fi

failed=()
for name in "${requested[@]}"; do
    dir="$ROOT/services/$name"
    if [ ! -d "$dir" ]; then
        echo "error: $dir does not exist" >&2
        failed+=("$name")
        continue
    fi

    echo "==> building service '$name'"
    # Run from inside the service directory so cargo picks up that service's
    # own .cargo/config.toml (target triple, linker script, relocation and
    # code model) rather than the kernel workspace's.
    if ( cd "$dir" && cargo build --release ); then
        artifact="$(artifact_for "$name")"
        if [ -f "$artifact" ]; then
            echo "    ok: $artifact"
        else
            # A successful cargo build that did not produce the path the
            # kernel includes means the binary name and the crate name have
            # diverged; the kernel's include_bytes! would still fail.
            echo "    error: build succeeded but $artifact was not produced" >&2
            failed+=("$name")
        fi
    else
        failed+=("$name")
    fi
done

if [ ${#failed[@]} -gt 0 ]; then
    echo "" >&2
    echo "FAILED: ${failed[*]}" >&2
    exit 1
fi

echo ""
echo "All requested services built. 'cargo build -p kernel' should now succeed."
