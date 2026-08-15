#!/usr/bin/env bash
# bootstrap-worktree.sh — make a fresh worktree or clone able to build and boot.
#
# Provisions the two classes of artifact that are git-ignored and therefore
# absent from a new checkout, but that the kernel build and the boot test
# both hard-require:
#
#   1. the six ring-3 service binaries the kernel embeds (see below);
#   2. the `limine/` bootloader binaries the boot test stages into the ESP;
#   3. `rootfs.ext4`, the Path-Z glibc rootfs the boot test attaches as vdb.
#
# Item 3 does not break the build — it silently *shrinks the test suite*.
# Without it the boot test still reports PASSED while skipping ~58 rungs,
# including every REAL-glibc Path-Z test (dynamic execution, stdio, pthread,
# signal, fault). A lane that only ever ran in a fresh worktree would
# therefore never exercise the highest-value tests in the tree and would not
# be told so by the exit code. Provision it before trusting a green boot.
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
# THE BOOTLOADER
#
# scripts/boot-test.sh stages `limine/BOOTX64.EFI` into the EFI system
# partition. `/limine/` is git-ignored (it is an upstream binary release, not
# our source), so a fresh worktree fails the boot test at the staging step
# with a bare `cp: cannot stat .../limine/BOOTX64.EFI` — after a full kernel
# build has already succeeded, which makes it look like a boot regression
# rather than a missing prerequisite. We fetch the same shallow v8.x-binary
# clone that scripts/setup-toolchain.sh documents. If a sibling worktree
# already has one, we copy from it instead of re-downloading.
#
# USAGE
#
#     ./scripts/bootstrap-worktree.sh              # everything
#     ./scripts/bootstrap-worktree.sh netstack     # build just one service
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

# The one file boot-test.sh actually stages; its presence stands in for a
# usable limine checkout.
LIMINE_MARKER="$ROOT/limine/BOOTX64.EFI"
LIMINE_REPO="https://github.com/limine-bootloader/limine.git"
LIMINE_BRANCH="v8.x-binary"

# Fetch the limine binary release, preferring a copy from a sibling worktree
# so that setting up the second and third lanes needs no network at all.
provision_limine() {
    if [ -f "$LIMINE_MARKER" ]; then
        echo "==> limine already present"
        return 0
    fi

    # A sibling checkout of the same project (…/os, …/os-lane-a, …) very
    # likely already has it. Copying is faster than cloning and, more
    # importantly, pins every lane to the identical bootloader build — a
    # version skew between lanes would make boot results incomparable.
    local parent sibling
    parent="$(dirname "$ROOT")"
    for sibling in "$parent"/*/; do
        sibling="${sibling%/}"
        [ "$sibling" = "$ROOT" ] && continue
        if [ -f "$sibling/limine/BOOTX64.EFI" ]; then
            echo "==> copying limine from sibling worktree $(basename "$sibling")"
            cp -r "$sibling/limine" "$ROOT/limine"
            [ -f "$LIMINE_MARKER" ] && return 0
            echo "    warning: copy did not produce $LIMINE_MARKER; falling back to clone" >&2
            rm -rf "$ROOT/limine"
            break
        fi
    done

    echo "==> cloning limine ($LIMINE_BRANCH)"
    if ! git clone "$LIMINE_REPO" --branch="$LIMINE_BRANCH" --depth=1 "$ROOT/limine"; then
        echo "    error: could not clone limine from $LIMINE_REPO" >&2
        return 1
    fi
    if [ ! -f "$LIMINE_MARKER" ]; then
        echo "    error: clone succeeded but $LIMINE_MARKER is missing" >&2
        return 1
    fi
    return 0
}

# The Path-Z glibc rootfs the boot test attaches as a second virtio-blk disk
# and the kernel probes onto /mnt.
ROOTFS_IMG="$ROOT/rootfs.ext4"

# True if the file carries an ext4 superblock magic (0xEF53, little-endian at
# byte offset 0x438). Guards against copying a partially-written image out of
# a sibling worktree while another lane is regenerating it — a truncated
# 256 MiB image would otherwise fail as a mysterious mount error at boot.
looks_like_ext4() {
    local f="$1" magic
    [ -f "$f" ] || return 1
    magic="$(od -An -tx1 -j 1080 -N 2 "$f" 2>/dev/null | tr -d ' \n')"
    [ "$magic" = "53ef" ]
}

provision_rootfs() {
    if looks_like_ext4 "$ROOTFS_IMG"; then
        echo "==> rootfs.ext4 already present"
        return 0
    fi
    if [ -f "$ROOTFS_IMG" ]; then
        echo "==> rootfs.ext4 present but has no ext4 superblock — replacing"
        rm -f "$ROOTFS_IMG"
    fi

    local parent sibling
    parent="$(dirname "$ROOT")"
    for sibling in "$parent"/*/; do
        sibling="${sibling%/}"
        [ "$sibling" = "$ROOT" ] && continue
        if looks_like_ext4 "$sibling/rootfs.ext4"; then
            echo "==> copying rootfs.ext4 from sibling worktree $(basename "$sibling") (256 MiB)"
            cp "$sibling/rootfs.ext4" "$ROOTFS_IMG"
            if looks_like_ext4 "$ROOTFS_IMG"; then
                return 0
            fi
            echo "    error: copied image has no ext4 superblock; removing" >&2
            rm -f "$ROOTFS_IMG"
            return 1
        fi
    done

    # Building it needs a Linux userland (mke2fs/debugfs) plus a glibc
    # cross-build, so we cannot do it from this shell. Say exactly what to
    # run rather than failing with a bare "missing".
    echo "    rootfs.ext4 not found, and no sibling worktree has one." >&2
    echo "    Build it with:  wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh" >&2
    echo "    Without it the boot test still reports PASSED but silently skips" >&2
    echo "    ~58 rungs, including every REAL-glibc Path-Z test." >&2
    return 1
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
    if [ -f "$LIMINE_MARKER" ]; then
        printf '  present  limine bootloader\n'
    else
        printf '  MISSING  limine bootloader  (%s)\n' "$LIMINE_MARKER"
        missing=$((missing + 1))
    fi
    if looks_like_ext4 "$ROOTFS_IMG"; then
        printf '  present  rootfs.ext4 (Path-Z glibc tests)\n'
    else
        printf '  MISSING  rootfs.ext4  (%s) — ~58 tests will silently SKIP\n' "$ROOTFS_IMG"
        missing=$((missing + 1))
    fi
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
        echo "$missing prerequisite$([ "$missing" -eq 1 ] || echo s) missing;"
        echo "run ./scripts/bootstrap-worktree.sh to provision them."
        exit 1
    fi
    echo ""
    echo "All build and boot prerequisites present."
    exit 0
fi

failed=()

# The bootloader is only needed when provisioning the whole worktree; a
# targeted rebuild of one service should not reach for the network.
if [ $# -eq 0 ]; then
    provision_limine || failed+=("limine")
    provision_rootfs || failed+=("rootfs.ext4")
fi

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
if [ $# -eq 0 ]; then
    echo "Worktree provisioned. 'cargo build -p kernel' and"
    echo "'./scripts/boot-test.sh' should now both succeed."
else
    echo "All requested services built."
fi
