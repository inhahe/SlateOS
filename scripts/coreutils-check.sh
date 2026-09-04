#!/usr/bin/env bash
#
# Build, lint and test a userspace package against BOTH targets that matter,
# because each one hides half of it from the other.
#
# ## The hole this closes
#
# The development host is Windows. The target family is `unix`. So the build
# everyone runs -- `cargo test -p coreutils` from the workspace root, or from
# `userspace/` -- compiles exactly one of the two arms of every `#[cfg(unix)]`
# in the zone, and the arm it compiles is the one that does not ship.
#
# That is not a theoretical gap, and it was found by accident rather than by
# looking. The `rm`/`dirfd` work passed `cargo test --target
# x86_64-pc-windows-gnu` (361 + 53 tests) and an 89-minute
# `cargo clippy --all-targets` on the same target with zero warnings. Run for
# real against `x86_64-unknown-linux-gnu`, the same tree produced two warnings
# the host build cannot emit, and reported **405** lib tests and **59** `rm`
# tests instead of 361 and 53. Forty-four lib tests and six `rm` tests existed
# that the normal workflow silently skipped -- including all three of `dirfd`'s
# directory-swap refusals, which are the tests that certify the security
# property the module was written for.
#
# They passed. The point is that nothing in the normal workflow would have said
# so if they had not, because a test that never runs is spelled exactly like a
# test that passes. See `known-issues.md` ->
# `TD-B-THE-UNIX-HALF-OF-COREUTILS-IS-NEITHER-LINTED-NOR-TESTED-BY-DEFAULT`,
# whose "proper fix" step 1 is this file.
#
# The gap runs in both directions, which is why this runs both halves rather
# than switching everyone to Linux. `cargo check --workspace --target
# x86_64-pc-windows-gnu` is the *only* thing in the tree that compiles a
# `cfg(not(unix))` arm of a file in this zone -- measured 2026-08-30, when
# `tar` had been failing it on `main` and six tests behind that failure had
# stopped running without anyone seeing a red line.
#
# ## Why WSL, and why that particular target directory
#
# There is no Linux Rust toolchain on the Windows side, and the 50 `*-diff.sh`
# harnesses already needed one, so WSL is where it lives. They build into
# `$HOME/.cache/slateos-diff-target` inside WSL and this uses the same
# directory on purpose: the Linux and Windows builds must not share a target
# dir (they invalidate each other on every alternation -- design-decisions.md
# §374), but two Linux consumers sharing one costs nothing and saves ~20 GB of
# duplicate object code. This script therefore adds no disk of its own.
#
# The Linux half runs from the **workspace root**, not from `userspace/`.
# `userspace/.cargo/config.toml` sets `build-std = [... "panic_abort"]` for the
# SlateOS target, and asking for a host target underneath that config dies with
# "the crate `panic_abort` does not have the panic strategy `abort`" -- a
# message about a mismatch this script never asked for. The root config sets no
# `build-std`, which is exactly why it was put in the zone and not the root.
#
# ## Why this is not wired into the boot test
#
# Cost, and only cost. The host `clippy -p coreutils --all-targets` run took 89
# minutes; a second full pass on the Linux side is not free, and the boot test
# already spends forty minutes on gates. Step 2 of the tech-debt entry wants
# this scoped to the crates a push touches -- exactly as pre-push gate 11
# scopes itself -- before it becomes something every push pays for. Until then
# this is the thing you run by hand after touching a `#[cfg(unix)]` arm, and
# the reason it exists as a script rather than as a paragraph of instructions
# is `known-issues.md` lesson 63: a rule kept only by copying is a rule that
# will be dropped.
#
# ## Usage
#
#     scripts/coreutils-check.sh [-p PKG]... [--only host|linux]
#                                [--no-clippy] [--no-test] [--] [cargo args...]
#
# Defaults: `-p coreutils`, both targets, clippy and test both run.
# Trailing arguments after `--` are appended to every cargo invocation, which
# is how you narrow a run to one module (`-- dirfd`) while iterating.
#
# ## Exit codes
#
#     0  every requested half ran, and passed
#     1  a requested half ran and failed
#     2  a requested half could not run at all
#
# 2 is distinct from 0 deliberately, and it is the whole point of the file: the
# defect being guarded against is a check that did not happen reading as a
# check that passed. "No WSL on this host" is an honest decline, not a pass,
# and the summary says which halves actually ran either way.

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)

pkgs=()
only=both
run_clippy=1
run_test=1
extra=()

while [ $# -gt 0 ]; do
  case "$1" in
    -p|--package) pkgs+=("$2"); shift 2 ;;
    --only) only="$2"; shift 2 ;;
    --no-clippy) run_clippy=0; shift ;;
    --no-test) run_test=0; shift ;;
    --) shift; extra=("$@"); break ;;
    -h|--help) sed -n '/^# ## Usage/,/^# 2 is distinct/p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "coreutils-check: unknown argument: $1" >&2; exit 2 ;;
  esac
done
if [ ${#pkgs[@]} -eq 0 ]; then pkgs=(coreutils); fi
case "$only" in
  both|host|linux) ;;
  *) echo "coreutils-check: --only takes host, linux or both" >&2; exit 2 ;;
esac
if [ "$run_clippy" -eq 0 ] && [ "$run_test" -eq 0 ]; then
  echo "coreutils-check: --no-clippy and --no-test together ask for nothing" >&2
  exit 2
fi

pkg_args=()
for p in "${pkgs[@]}"; do pkg_args+=(-p "$p"); done

# `--lib --bins` rather than `--all-targets`: the zone's integration tests and
# examples are the slowest part of the build and none of them is what this file
# is about. A `#[cfg(unix)]` arm lives in the library or in a `src/bin/*.rs`.
scope=(--lib --bins)

status=0            # 0 pass, 1 a half failed, 2 a half could not run
ran=()
skipped=()

note() { echo "" ; echo "=== $* ===" ; }

# --- the Windows host half ----------------------------------------------------
# This is the only build in the tree that sees a `cfg(not(unix))` arm in this
# zone, so a failure here is a real finding even though the arm never ships.
run_host() {
  local host_target=x86_64-pc-windows-gnu
  if ! command -v cargo >/dev/null 2>&1; then
    echo "coreutils-check: no cargo on the host; the host half cannot run." >&2
    skipped+=("host (no cargo)")
    status=2
    return
  fi
  if [ "$run_clippy" -eq 1 ]; then
    note "host clippy ($host_target)"
    if ! (cd "$root" && cargo +nightly clippy "${pkg_args[@]}" "${scope[@]}" \
            --target "$host_target" "${extra[@]+"${extra[@]}"}"); then
      status=1
    fi
  fi
  if [ "$run_test" -eq 1 ]; then
    note "host test ($host_target)"
    if ! (cd "$root" && cargo +nightly test "${pkg_args[@]}" "${scope[@]}" \
            --target "$host_target" "${extra[@]+"${extra[@]}"}"); then
      status=1
    fi
  fi
  ran+=("host")
}

# --- the Linux half, through WSL ----------------------------------------------
run_linux() {
  local linux_target=x86_64-unknown-linux-gnu

  if command -v wslpath >/dev/null 2>&1; then
    # Already inside WSL: nothing to translate, nothing to re-exec.
    local inside="$root"
  else
    if ! command -v wsl >/dev/null 2>&1; then
      echo "coreutils-check: no WSL on this host, so the unix half of these" >&2
      echo "  crates cannot be compiled here at all. That is a decline, not a" >&2
      echo "  pass: every #[cfg(unix)] arm went unchecked." >&2
      skipped+=("linux (no WSL)")
      status=2
      return
    fi
    # MSYS rewrites anything that looks like a path on its way to `wsl`.
    export MSYS2_ARG_CONV_EXCL='*'
    local win="$root"
    if command -v cygpath >/dev/null 2>&1; then win=$(cygpath -m "$root"); fi
    local inside
    if ! inside=$(wsl wslpath -u "$win" 2>/dev/null); then
      echo "coreutils-check: could not map $win into WSL; the unix half did" >&2
      echo "  not run." >&2
      skipped+=("linux (unmappable path)")
      status=2
      return
    fi
  fi

  # The far side builds into the harnesses' shared cache, so this adds no disk
  # of its own; it computes the path itself because it is a separate shell on
  # the other side of a boundary this one cannot expand `$HOME` across.
  local steps=""
  if [ "$run_clippy" -eq 1 ]; then steps="$steps clippy"; fi
  if [ "$run_test" -eq 1 ]; then steps="$steps test"; fi

  # One `wsl` invocation for the whole half. Two would pay the distro's startup
  # twice and, worse, could disagree about which toolchain answered.
  local script
  script=$(cat <<'WSLEOF'
set -eu
cd "$1"; shift
tdir="$HOME/.cache/slateos-diff-target"
cargo="$HOME/.cargo/bin/cargo"
if [ ! -x "$cargo" ]; then
  echo "coreutils-check: no cargo inside WSL ($cargo)." >&2
  echo "  Install one with rustup; without it the unix half is unchecked." >&2
  exit 2
fi
steps="$1"; shift
target="$1"; shift
rc=0
for step in $steps; do
  echo ""
  echo "=== linux $step ($target) ==="
  # `+nightly` for the reason userspace/.cargo/config.toml states in capitals:
  # this workspace's unstable settings are silently ignored by stable, and a
  # silently-ignored setting is how you get a green run of the wrong thing.
  "$cargo" +nightly "$step" "$@" --target "$target" --target-dir "$tdir" || rc=1
done
exit $rc
WSLEOF
)
  local rc=0
  wsl -e bash -c "$script" -- "$inside" "$steps" "$linux_target" \
      "${pkg_args[@]}" "${scope[@]}" "${extra[@]+"${extra[@]}"}" || rc=$?
  case "$rc" in
    0) ran+=("linux") ;;
    2) skipped+=("linux (no cargo in WSL)"); status=2 ;;
    *) ran+=("linux"); status=1 ;;
  esac
}

case "$only" in
  host)  run_host ;;
  linux) run_linux ;;
  both)  run_host; run_linux ;;
esac

note "summary"
echo "packages: ${pkgs[*]}"
if [ ${#ran[@]} -gt 0 ]; then echo "ran:      ${ran[*]}"; else echo "ran:      (nothing)"; fi
if [ ${#skipped[@]} -gt 0 ]; then echo "declined: ${skipped[*]}"; fi
case "$status" in
  # Named, not "both halves clean": a run narrowed with --only is a smaller
  # claim than a full one, and a summary that overstates its own scope is the
  # same defect one level up from the one this script exists to close.
  0) echo "result:   clean (${ran[*]} half checked)" ;;
  1) echo "result:   FAILED -- see the output above" ;;
  2) echo "result:   INCOMPLETE -- a half could not run, which is not a pass" ;;
esac
exit "$status"
