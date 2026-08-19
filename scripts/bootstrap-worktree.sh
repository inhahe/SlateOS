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
#     ./scripts/bootstrap-worktree.sh --check --need=limine,rootfs
#                                                  # ...only these classes
#
# Safe to re-run: cargo no-ops when a service is already up to date, so this
# is cheap enough to run before any kernel build if you are unsure.
#
# `--check` exit status, which `scripts/boot-test.sh` reads:
#
#     0  everything present
#     1  something *blocking* is missing — the kernel cannot build, or the
#        boot test cannot stage (a service binary, or limine)
#     2  usage error (unknown service name, or the kernel's embed list could
#        not be derived — see embedded_artifact_paths)
#     3  only rootfs.ext4 is missing — the boot test still runs and still
#        passes, but silently tests ~58 rungs fewer
#
# 1 and 3 are split because conflating them forces a caller to choose between
# refusing a run that would have been useful and accepting a green result that
# quietly measured less than it claims. Both are wrong; neither is necessary.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"

# The services the kernel embeds are DERIVED from the `include_bytes!` calls
# in kernel/src, not listed here.
#
# A literal list would be a second copy of a fact the kernel already states,
# and the two drift in one direction only: a service added to the kernel and
# not added here goes missing from every fresh worktree, while this script —
# whose entire job is to say what is missing — reports everything present.
# That is the worst shape a check can have, and deriving the list makes it
# impossible. See known-issues.md
# A-A-FRESH-CHECKOUT-CANNOT-BOOT-TEST-AND-NEITHER-FAILURE-NAMES-THE-MISSING-STEP.
#
# The pattern matches the artifact path *inside* the macro, so it yields the
# target triple and the binary name as well as the service directory. Nothing
# about the layout is assumed beyond `services/<dir>/target/<triple>/release/
# <bin>`, which is what the kernel literally writes — a service that one day
# builds for a different triple needs no change here.
embedded_artifact_paths() {
    grep -rh --include='*.rs' 'include_bytes!' "$ROOT/kernel/src" 2>/dev/null \
        | grep -o 'services/[A-Za-z0-9_.-]\+/target/[A-Za-z0-9_.-]\+/release/[A-Za-z0-9_.-]\+' \
        | sort -u
}

declare -A ARTIFACT=()
SERVICES=()
while IFS= read -r _rel; do
    [ -n "$_rel" ] || continue
    _name="${_rel#services/}"
    _name="${_name%%/*}"
    if [ -z "${ARTIFACT[$_name]+set}" ]; then
        SERVICES+=("$_name")
        ARTIFACT["$_name"]="$_rel"
    elif [ "${ARTIFACT[$_name]}" != "$_rel" ]; then
        # One service directory, two different embedded artifacts. Keeping the
        # first would provision one and silently leave the other missing, which
        # is exactly the failure this derivation exists to prevent — so refuse
        # rather than pick.
        echo "error: service '$_name' is embedded from two different paths:" >&2
        echo "         ${ARTIFACT[$_name]}" >&2
        echo "         $_rel" >&2
        echo "       One cargo invocation cannot produce both; teach this" >&2
        echo "       script how before continuing." >&2
        exit 2
    fi
done < <(embedded_artifact_paths)

if [ ${#SERVICES[@]} -eq 0 ]; then
    # An empty scan must never read as a clean bill of health. Either the
    # kernel stopped embedding services (delete the derivation) or the pattern
    # no longer matches how they are written (fix it) — both need a human, and
    # neither is "nothing is missing".
    echo "error: found no include_bytes! service artifacts under kernel/src." >&2
    echo "       Refusing to report 'all prerequisites present' from a scan" >&2
    echo "       that found nothing to look for. See embedded_artifact_paths." >&2
    exit 2
fi

artifact_for() {
    echo "$ROOT/${ARTIFACT[$1]}"
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

# Which classes of prerequisite `--check` reports on.  All three by default.
#
# This exists because "missing" and "missing *and needed*" are different
# questions, and only the caller knows which it is asking.  `boot-test.sh
# --no-build --no-stage` boots an already-staged image: it touches neither the
# embedded service binaries nor `limine/`, so refusing that run because they are
# absent would be a false refusal — the run is valid and would have worked.  A
# gate that blocks correct runs gets disabled, and then it is not a gate.
#
# Provisioning mode has no use for this (it provisions everything or the
# named services), so `--need` outside `--check` is an error rather than a
# silently-ignored flag.
need_services=1
need_limine=1
need_rootfs=1
_need_given=0

while [ $# -gt 0 ]; do
    case "$1" in
        --check) check_only=1; shift ;;
        --need=*)
            # The first --need clears the defaults; further ones add to it.
            if [ "$_need_given" -eq 0 ]; then
                need_services=0; need_limine=0; need_rootfs=0; _need_given=1
            fi
            IFS=',' read -r -a _classes <<< "${1#--need=}"
            for _c in ${_classes[@]+"${_classes[@]}"}; do
                case "$_c" in
                    services) need_services=1 ;;
                    limine)   need_limine=1 ;;
                    rootfs)   need_rootfs=1 ;;
                    "")       ;;
                    *)
                        echo "error: unknown --need class '$_c'" >&2
                        echo "known classes: services, limine, rootfs" >&2
                        exit 2
                        ;;
                esac
            done
            shift
            ;;
        --) shift; break ;;
        -*)
            echo "error: unknown option '$1'" >&2
            echo "usage: bootstrap-worktree.sh [--check [--need=<classes>]] [service...]" >&2
            exit 2
            ;;
        *) break ;;
    esac
done

if [ "$_need_given" -eq 1 ] && [ "$check_only" -eq 0 ]; then
    echo "error: --need only applies to --check; provisioning always does" >&2
    echo "       everything, or the services you name." >&2
    exit 2
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
    # Two severities, reported and exited separately, because they are not the
    # same kind of problem and a caller that automates this needs to tell them
    # apart:
    #
    #   blocking  — the build or the boot cannot happen at all (services,
    #               limine).  Loud, immediate, unambiguous.
    #   degrading — the run still goes green while quietly testing less
    #               (rootfs.ext4 → ~58 rungs SKIP).  Far more dangerous to
    #               conflate with "fine", and far too weak a reason to refuse
    #               a boot test that would otherwise be useful.
    #
    # Exit: 0 all present, 1 something blocking, 3 only degrading, 2 usage.
    blocking=0
    degrading=0
    if [ "$need_limine" -eq 1 ]; then
        if [ -f "$LIMINE_MARKER" ]; then
            printf '  present  limine bootloader\n'
        else
            printf '  MISSING  limine bootloader  (%s)\n' "$LIMINE_MARKER"
            blocking=$((blocking + 1))
        fi
    fi
    if [ "$need_rootfs" -eq 1 ]; then
        if looks_like_ext4 "$ROOTFS_IMG"; then
            printf '  present  rootfs.ext4 (Path-Z glibc tests)\n'
        else
            printf '  DEGRADED rootfs.ext4  (%s) — ~58 tests will silently SKIP\n' "$ROOTFS_IMG"
            degrading=$((degrading + 1))
        fi
    fi
    if [ "$need_services" -eq 1 ]; then
        for name in "${requested[@]}"; do
            path="$(artifact_for "$name")"
            if [ -f "$path" ]; then
                printf '  present  %s\n' "$name"
            else
                printf '  MISSING  %s  (%s)\n' "$name" "$path"
                blocking=$((blocking + 1))
            fi
        done
    fi
    if [ "$blocking" -gt 0 ]; then
        echo ""
        echo "$blocking prerequisite$([ "$blocking" -eq 1 ] || echo s) missing."
        echo "The kernel cannot build, or the boot test cannot stage, until"
        echo "provisioned:"
        echo "    ./scripts/bootstrap-worktree.sh"
        exit 1
    fi
    if [ "$degrading" -gt 0 ]; then
        echo ""
        echo "Everything needed to build and boot is present, but rootfs.ext4 is not,"
        echo "so a boot test will report PASSED while skipping every REAL-glibc"
        echo "Path-Z rung.  Provision it before trusting a green run:"
        echo "    ./scripts/bootstrap-worktree.sh"
        exit 3
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
