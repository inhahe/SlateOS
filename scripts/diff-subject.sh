# Shared preamble for the `*-diff.sh` differential harnesses: build the binary
# the harness is about to measure, and hand back an absolute path to it.
#
# Sourced, not executed:
#
#     . "$(dirname "$0")/diff-subject.sh"
#     OURS=$(subject_binary coreutils tr "${OURS:-}") || exit 1
#
# ## Why this exists
#
# A harness that names `target/<triple>/debug/<name>.exe` and then just *runs*
# it is not measuring the tree — it is measuring whatever was last written to
# that path, which can be arbitrarily old and need not even come from the same
# crate. Both failure modes have happened here, and the second one cost a day:
#
#   * **Stale.** `cargo test` and `cargo clippy` do not refresh a binary. A fix
#     verified by a unit test and then "measured" by the harness was measured
#     against the *previous* build. That was found first in `printf-diff.sh`,
#     and the fix (build every run) was applied to `printf` and `seq` only.
#
#   * **The wrong program entirely.** Forty-two binary names in this workspace
#     are produced by *two* packages — `coreutils` and a superseded standalone
#     `userspace/<name>` crate — which cargo warns about ("output filename
#     collision") and then resolves by letting whichever built last win. So
#     `target/…/debug/bc.exe` was sometimes `userspace/bc` and sometimes
#     `coreutils/src/bin/bc.rs`, two different bc implementations. On
#     2026-08-21 `calc-diff.sh` reported "95 passed, 105 differed" and three
#     bugs were written up in `known-issues.md` against a bc that no one
#     intends to ship; the bc that is shipped passes all 200. See
#     `known-issues.md` → `B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES`.
#
# Naming the package as well as the binary is what closes the second hole: this
# build writes the path immediately before the harness reads it, from the crate
# the harness means.
#
# ## The `OURS` override
#
# Every harness supports `OURS=/usr/bin/tr ./scripts/tr-diff.sh` as a control —
# pointing it at the *reference* implementation should make it report almost
# nothing, which is how the harness itself is checked. A caller-supplied path is
# passed straight through: it is not ours to build, and building over it would
# defeat the check.

# The workspace root, captured *now* — while this file is being sourced — rather
# than looked up when a function is called. Two reasons it cannot be deferred:
# most harnesses `cd` into a temporary fixtures directory partway through, after
# which a relative `$0` no longer resolves; and `$BASH_SOURCE` is not available,
# because `all-diff.sh` runs each harness under `sh`. `$0` is the harness that
# sourced this file, and the source line names this file relative to it, so the
# fact that we are executing at all means `dirname "$0"` is the `scripts/`
# directory.
_diff_subject_root=$(cd "$(dirname "$0")/.." && pwd) || return 1

# subject_binary  PACKAGE BIN     [OVERRIDE]
# subject_example PACKAGE EXAMPLE [OVERRIDE]
#
# Echoes an absolute path to the binary, having built it, unless OVERRIDE is a
# non-empty caller-supplied path — in which case that is echoed unchanged.
# Returns non-zero (with nothing on stdout) if the build fails.
#
# `subject_example` is for a harness whose subject is a probe program rather
# than a shipped utility (`extfloat-diff.sh`); cargo puts those under
# `debug/examples/` and selects them with `--example`.
subject_binary()  { _subject_build --bin "$@"; }
subject_example() { _subject_build --example "$@"; }

_subject_build() {
    if [ "$#" -lt 3 ]; then
        echo "subject_binary: need PACKAGE and BIN" >&2
        return 2
    fi
    local kind=$1 pkg=$2 bin=$3 override=${4:-}
    local subdir=""
    [ "$kind" = --example ] && subdir="examples/"

    if [ -n "$override" ]; then
        printf '%s\n' "$override"
        return 0
    fi

    local target=${DIFF_TARGET:-x86_64-pc-windows-gnu}
    # The suffix follows the triple rather than the host, so that pointing
    # `DIFF_TARGET` at a Linux triple does not ask for a `.exe` that will never
    # exist.
    local suffix=""
    case $target in *windows*) suffix=".exe" ;; esac

    # `cargo build` runs at the workspace root because the target triple and the
    # linker flags come from the root `.cargo/config.toml`, and cargo reads that
    # from the invocation directory upward.
    local root=$_diff_subject_root

    # Built every run, not only when the file is missing: "is it there?" is the
    # question that lets a stale binary through, and it is the stale binary that
    # produces a *confident wrong answer* rather than an obvious failure.
    # Diagnostics go to stderr so that they cannot end up in the path this
    # function returns through stdout.
    ( cd "$root" && cargo build -p "$pkg" "$kind" "$bin" --target "$target" ) >&2 || return 1

    local path="$root/target/$target/debug/$subdir$bin$suffix"
    if [ ! -x "$path" ]; then
        echo "subject_binary: cargo built $pkg:$bin but there is no $path" >&2
        return 1
    fi
    printf '%s\n' "$path"
}
