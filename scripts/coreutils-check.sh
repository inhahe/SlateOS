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
# Cost was the stated reason, and the stated cost was wrong. The 89 minutes
# quoted here originally was a host `clippy --all-targets` run, and this script
# does not do that: it compiles a package's library and its binaries and
# nothing else, which drops the zone's integration tests and examples -- the
# slowest part of the build and none of it where a `#[cfg(unix)]` arm lives.
# Measured 2026-09-04 against a warm shared target dir: the whole Linux half is
# **2m16s** (clippy 1m12s, test 1m04s). That is a boot-test gate's worth of
# time, not an hour's, so the premise that kept this unwired does not survive
# being measured.
#
# What remains true is that a warm cache is doing the work. The first run after
# the shared `slateos-diff-target` is invalidated pays a full Linux build, so a
# gate wants the scoping step 2 asks for -- the crates a push touches, exactly
# as pre-push gate 11 scopes itself -- rather than a blanket run.
#
# Either way this is also the thing you run by hand after touching a
# `#[cfg(unix)]` arm, and the reason it exists as a script rather than as a
# paragraph of instructions is `known-issues.md` lesson 63: a rule kept only by
# copying is a rule that will be dropped.
#
# ## The WSL output boundary, and why stdout here carries only the verdict
#
# `wsl.exe` does not share a file offset with any other writer -- not with the
# shell that launched it, not with a `cat` relaying its own pipe, and not
# between its own two streams. So when a caller merges the streams into one
# file, which is what `cmd > log 2>&1` does and what every backgrounded run in
# this tree is recorded as, whichever of the two WSL streams writes more starts
# at offset zero and overwrites the other. Nothing reports this; the bytes are
# simply gone.
#
# It is not hypothetical here. A `--only linux --no-clippy=0` run of this very
# file emitted `=== linux clippy (x86_64-unknown-linux-gnu) ===` on stdout and
# 1.2 MB of cargo manifest warnings on stderr, and in the merged capture the
# header did not exist -- so the log said only `result: clean`, with nothing to
# say which half had produced it. That is this script's own founding defect,
# one level up: a check that did not visibly happen reading as a check that
# passed.
#
# The fix is that exactly one writer may own the file. The far side therefore
# does `exec 1>&2` (see `run_linux`), collapsing everything WSL emits -- step
# headers, cargo's stdout, cargo's stderr -- onto the single stream, and this
# side keeps **stdout for the summary block and nothing else**. The verdict is
# then unreachable by any WSL handle, and
#
#     scripts/coreutils-check.sh > verdict.txt 2> log.txt
#
# gives a five-line verdict and a full log. Relaying WSL's stdout through a
# pipe was tried and does not work: the relaying process is just one more
# writer with an offset of its own, and it loses the same bytes.
#
# ## Usage
#
#     scripts/coreutils-check.sh [-p PKG]... [--dir DIR]... [--only host|linux]
#                                [--no-clippy] [--no-test] [--] [cargo args...]
#
# `--dir` names a crate by where it lives instead of what it is called, and is
# for callers that have a path and not a name -- pre-push gate 12 derives its
# scope from the files a push changed. The two may be mixed, and a crate named
# both ways is compiled once.
#
# Defaults: `-p coreutils` (only when neither -p nor --dir was given), both
# targets, clippy and test both run.
# Trailing arguments after `--` are appended to every cargo invocation, which
# is how you narrow a run to one module (`-- dirfd`) while iterating.
#
# Any package may be named, whatever shape it is: what to compile is worked out
# per package from its manifest, so a crate with only a `main.rs` is checked
# rather than rejected. A name that matches no package in the tree, and a
# package with neither a library nor a binary target, are both usage errors
# (64) -- there is nothing to check in either, and reporting nothing as clean
# is the one thing this script exists to prevent.
#
# ## Exit codes
#
#     0  every requested half ran, and passed
#     1  a requested half ran and failed
#     2  a requested half could not run at all
#    64  this script was invoked wrongly (usage error)
#
# 64 is separated from 2 for the sake of callers that are wired to tolerate a
# decline -- pre-push gate 12 passes `--may-skip`, which turns exit 2 into a
# reported skip. If a usage error also exited 2, then the day someone renames a
# flag here, that gate would read argparse-style breakage as a legitimate
# "no WSL on this host" and skip on every push, forever, and nothing would say
# so. `run-checker.sh` guards the same trap for Python checkers by sniffing for
# `usage:`; a shell script has no such banner to sniff, so it must say it in the
# exit code instead.
#
# 2 is distinct from 0 deliberately, and it is the whole point of the file: the
# defect being guarded against is a check that did not happen reading as a
# check that passed. "No WSL on this host" is an honest decline, not a pass,
# and the summary says which halves actually ran either way.
#
# ## Streams
#
#     stdout  the summary block, and nothing else
#     stderr  step headers, cargo output, decline messages
#
# So `>verdict 2>log` splits the two cleanly, and a caller that merges them
# still keeps both -- which was not true before; see "## The WSL output
# boundary" above for what merging used to destroy.

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)

pkgs=()
dirs=()
only=both
run_clippy=1
run_test=1
extra=()

while [ $# -gt 0 ]; do
  case "$1" in
    -p|--package) pkgs+=("$2"); shift 2 ;;
    # `--dir` exists for callers that know a *path* and not a package name --
    # pre-push gate 12 derives its scope from the files a push changed, and
    # `userspace/ssh-keygen/src/main.rs` says which directory changed and
    # nothing about what the crate there is called. Resolving that here rather
    # than in the caller keeps one implementation of "what package lives in
    # this directory"; a second one in the hook would be a second thing to be
    # wrong the first time a crate's name stops matching its folder.
    --dir) dirs+=("$2"); shift 2 ;;
    --only) only="$2"; shift 2 ;;
    --no-clippy) run_clippy=0; shift ;;
    --no-test) run_test=0; shift ;;
    --) shift; extra=("$@"); break ;;
    -h|--help) sed -n '/^# ## Usage/,/^# 2 is distinct/p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    # 64, not 2: see "## Exit codes". A usage error must not be spellable as a
    # decline, or a caller wired to tolerate declines tolerates its own typo.
    *) echo "coreutils-check: unknown argument: $1" >&2; exit 64 ;;
  esac
done
case "$only" in
  both|host|linux) ;;
  *) echo "coreutils-check: --only takes host, linux or both" >&2; exit 64 ;;
esac
if [ "$run_clippy" -eq 0 ] && [ "$run_test" -eq 0 ]; then
  echo "coreutils-check: --no-clippy and --no-test together ask for nothing" >&2
  exit 64
fi

# --- what to compile, decided per package -------------------------------------
#
# `--lib --bins` rather than `--all-targets`: the zone's integration tests and
# examples are the slowest part of the build and none of them is what this file
# is about. A `#[cfg(unix)]` arm lives in the library or in a `src/bin/*.rs`.
#
# But `--lib` is not a request, it is an assertion. cargo fails outright with
# "no library targets found in package 'shell'" when the named package has
# none, and that is exactly how `-p shell` died on 2026-09-04 while measuring
# step 4 of this file's tech-debt entry -- a defect in this script, not in the
# crate, which has a `main.rs` and no `lib.rs` like every other program does.
#
# `--bins` fails in the opposite direction: cargo accepts it for a package with
# no binaries and quietly builds nothing. So the two flags are not symmetric,
# and only one of them is loud when it is wrong. That asymmetry is why the
# fix cannot be "just drop --lib": a package with neither target would then be
# reported as a clean run of nothing, which is this script's founding defect
# reappearing inside the script itself. Such a package is refused by name.
#
# WHY THE MANIFEST AND NOT CARGO. cargo knows the answer authoritatively, and
# asking it costs a workspace load: measured against this tree's 2,950 members
# on 2026-09-04, `cargo read-manifest` is 8-9s per package and `cargo metadata
# --no-deps --offline` is 13.8s. Worse than the seconds is *where* they could
# be spent -- the half that has a cargo is not always the half that runs. This
# script's whole purpose is compiling the Linux half on a Windows host, and it
# declines the host half by design when there is no cargo there. Scope is
# computed once, on this side, and handed to both halves; a mechanism that
# needs a local cargo could not do that.
#
# AND THIS IS NOT A HEURISTIC. A cargo library target exists if and only if the
# manifest declares `[lib]` or the file `src/lib.rs` is present -- there is no
# third spelling, because a library whose source lives elsewhere has to say so
# with `[lib] path = ...`. The `autolib = false` / `autobins = false` edges
# make the test below over-count, never under-count, and over-counting is the
# loud direction: cargo says the target is missing and this script fails with
# it. Under-counting would be the silent one, and it cannot happen.

# The name under `[package]`, and only there. `[[bin]] name = "shell"` is a
# different claim, and a locator that accepted it would attribute one crate's
# targets to another crate that merely ships a binary of that name -- which in
# a tree whose coreutils crate builds ~180 named binaries is not a remote
# possibility.
manifest_name() {
  awk '
    /^[[:space:]]*\[/ { inpkg = ($0 ~ /^[[:space:]]*\[package\]/); next }
    inpkg && /^[[:space:]]*name[[:space:]]*=/ {
      if (match($0, /"[^"]*"/)) { print substr($0, RSTART + 1, RLENGTH - 2); exit }
    }
  ' "$1" 2>/dev/null || true
}

# Every manifest in the tree, worktree order. Untracked ones are included
# deliberately: a crate added but not yet committed is still a crate this
# script can be pointed at, and a locator that could not see it would fall
# through to "no such package" and refuse a package that is right there.
all_manifests() {
  if git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    git -C "$root" ls-files -- '*Cargo.toml' 'Cargo.toml'
    git -C "$root" ls-files --others --exclude-standard -- '*Cargo.toml' 'Cargo.toml'
  else
    (cd "$root" && find . -name target -prune -o -name Cargo.toml -print) \
      | sed 's|^\./||'
  fi
}

# The manifest path of package $1, or nothing.
#
# Three stages, and the last is what makes it correct rather than merely fast.
#
#  1. Guess. A crate almost always lives in a directory named after itself, so
#     a dozen stats answer this instantly. "Almost always" is not an invariant,
#     which is why it is only stage one.
#  2. Narrow. One `grep -l` over every manifest in the tree finds the few that
#     contain a `name = "<pkg>"` line at all -- 2,950 files in ~3s here. It is
#     one process, not one per file: 2,950 process spawns on Windows is half a
#     minute, and a locator that costs more than the build it is scoping would
#     simply be turned off.
#  3. Verify. `manifest_name` is run on the survivors, so `[[bin]] name =
#     "shell"` in some other crate cannot masquerade as a package. Stage 1's
#     guesses are verified the same way, so a wrong guess costs a stat and can
#     never produce a wrong answer.
manifest_of() {
  local want=$1 cand f
  for cand in "$want" "userspace/$want" "apps/$want" "gui/$want" "net/$want" \
              "init/$want" "services/$want" "drivers/$want" "fs/$want" \
              "toolchain/$want" "bench/$want" "pkg/$want"; do
    if [ -f "$root/$cand/Cargo.toml" ] &&
       [ "$(manifest_name "$root/$cand/Cargo.toml")" = "$want" ]; then
      echo "$root/$cand/Cargo.toml"
      return 0
    fi
  done
  while IFS= read -r f; do
    [ -n "$f" ] || continue
    if [ "$(manifest_name "$root/$f")" = "$want" ]; then
      echo "$root/$f"
      return 0
    fi
  done <<EOF
$(cd "$root" && all_manifests | tr '\n' '\0' | xargs -0 --no-run-if-empty \
    grep -lE "^[[:space:]]*name[[:space:]]*=[[:space:]]*\"$want\"[[:space:]]*\$" \
    2>/dev/null || true)
EOF
  return 1
}

# `--dir` resolved, now that `manifest_name` exists. A directory that holds no
# manifest, or a manifest that is a workspace rather than a package, is a usage
# error and not a quiet omission: the caller believed there was a crate there,
# and dropping it silently would shrink the checked set without saying so --
# which is the shape of defect this whole script was written against.
for d in ${dirs[@]+"${dirs[@]}"}; do
  case "$d" in /*|?:[/\\]*) mf_dir=$d ;; *) mf_dir="$root/$d" ;; esac
  if [ ! -f "$mf_dir/Cargo.toml" ]; then
    echo "coreutils-check: --dir '$d' has no Cargo.toml; nothing ran." >&2
    exit 64
  fi
  n=$(manifest_name "$mf_dir/Cargo.toml")
  if [ -z "$n" ]; then
    echo "coreutils-check: --dir '$d' has a Cargo.toml with no [package] name," >&2
    echo "  so it is a workspace and not a crate. Nothing ran." >&2
    exit 64
  fi
  pkgs+=("$n")
done

# The `-p coreutils` default belongs after `--dir` resolution, not before it:
# applied at parse time it would silently add coreutils to every `--dir` run.
if [ ${#pkgs[@]} -eq 0 ]; then pkgs=(coreutils); fi

# Two spellings of the same crate -- `-p stat --dir userspace/stat`, or a push
# that changed two files in one directory -- must not compile it twice.
if [ ${#pkgs[@]} -gt 1 ]; then
  mapfile -t pkgs < <(printf '%s\n' "${pkgs[@]}" | awk '!seen[$0]++')
fi

pkg_has_lib() {
  local dir
  dir=$(dirname "$1")
  if [ -f "$dir/src/lib.rs" ]; then return 0; fi
  if grep -q '^[[:space:]]*\[lib\]' "$1"; then return 0; fi
  return 1
}

pkg_has_bins() {
  local dir
  dir=$(dirname "$1")
  if [ -f "$dir/src/main.rs" ]; then return 0; fi
  if grep -q '^[[:space:]]*\[\[bin\]\]' "$1"; then return 0; fi
  # `src/bin/*.rs` and `src/bin/*/main.rs` are both binary targets to cargo.
  if compgen -G "$dir/src/bin/*.rs" >/dev/null; then return 0; fi
  if compgen -G "$dir/src/bin/*/main.rs" >/dev/null; then return 0; fi
  return 1
}

lib_pkgs=()
bin_pkgs=()
for p in "${pkgs[@]}"; do
  # Cargo package names are `[A-Za-z0-9_-]`, and this one is about to be spliced
  # into a grep pattern and a path. Checking it here means a stray quote or `*`
  # is a named usage error rather than a locator that searches for something
  # else and reports "no package named ...".
  case "$p" in
    *[!A-Za-z0-9._-]*|"")
      echo "coreutils-check: '$p' is not a cargo package name." >&2
      exit 64 ;;
  esac
  mf=$(manifest_of "$p") || mf=""
  if [ -z "$mf" ]; then
    echo "coreutils-check: no package named '$p' in this tree; nothing ran." >&2
    echo "  64 rather than 2 because a name that matches no manifest is a" >&2
    echo "  typo, and a caller wired to tolerate declines must not tolerate" >&2
    echo "  its own typo -- see '## Exit codes'." >&2
    exit 64
  fi
  if pkg_has_lib "$mf"; then
    lib_pkgs+=("$p")
  elif pkg_has_bins "$mf"; then
    bin_pkgs+=("$p")
  else
    echo "coreutils-check: package '$p' has neither a library nor a binary" >&2
    echo "  target ($mf), so there is nothing in it for this script to" >&2
    echo "  compile. Refusing rather than passing --bins, which cargo accepts" >&2
    echo "  for a package with no binaries and which would end in 'result:" >&2
    echo "  clean' having compiled nothing at all." >&2
    exit 64
  fi
done

# Groups of cargo arguments, flattened into one array with a separator, because
# bash has no array of arrays and this list has to survive being passed through
# `wsl -e bash -c` as positional arguments. There are at most two groups -- the
# packages that have a library and the packages that do not -- so the ordinary
# single-package run is still exactly one cargo invocation per step, as it was
# when the scope was a constant.
GSEP='@@'
for a in "${extra[@]+"${extra[@]}"}"; do
  if [ "$a" = "$GSEP" ]; then
    echo "coreutils-check: '$GSEP' is this script's group separator and cannot" >&2
    echo "  be passed through to cargo. Rename the argument or drop it." >&2
    exit 64
  fi
done

groups=()
add_group() {
  if [ ${#groups[@]} -gt 0 ]; then groups+=("$GSEP"); fi
  groups+=("$@")
}
if [ ${#lib_pkgs[@]} -gt 0 ]; then
  add_group --lib --bins
  for p in "${lib_pkgs[@]}"; do groups+=(-p "$p"); done
  groups+=("${extra[@]+"${extra[@]}"}")
fi
if [ ${#bin_pkgs[@]} -gt 0 ]; then
  add_group --bins
  for p in "${bin_pkgs[@]}"; do groups+=(-p "$p"); done
  groups+=("${extra[@]+"${extra[@]}"}")
fi

status=0            # 0 pass, 1 a half failed, 2 a half could not run
ran=()
skipped=()

# Step headers annotate the tool output they precede, so they go where that
# output goes -- stderr. Keeping them off stdout is what leaves the summary
# block alone there; see "## The WSL output boundary" in the header.
note() { echo "" >&2 ; echo "=== $* ===" >&2 ; }

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
  local steps=""
  if [ "$run_clippy" -eq 1 ]; then steps="$steps clippy"; fi
  if [ "$run_test" -eq 1 ]; then steps="$steps test"; fi
  # One invocation per (step, group). The trailing separator is what flushes
  # the last group, so the loop body handles a group in exactly one place.
  local step a
  local -a cur
  for step in $steps; do
    cur=()
    for a in "${groups[@]}" "$GSEP"; do
      if [ "$a" != "$GSEP" ]; then cur+=("$a"); continue; fi
      if [ ${#cur[@]} -gt 0 ]; then
        note "host $step ($host_target) ${cur[*]}"
        if ! (cd "$root" && cargo +nightly "$step" "${cur[@]}" \
                --target "$host_target"); then
          status=1
        fi
      fi
      cur=()
    done
  done
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
# Everything WSL emits goes out one stream, because two of them cannot survive
# a caller that merges them into a file: wsl.exe writes each with an offset of
# its own and the louder one silently overwrites the other. Collapsing here
# rather than asking callers to redirect is deliberate -- a rule kept only by
# remembering to redirect is a rule that will be dropped, and the bytes it
# loses are the ones that say which check ran.
exec 1>&2
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
gsep="$1"; shift
rc=0
# The remaining arguments are groups of cargo arguments separated by $gsep --
# see "what to compile, decided per package" on the near side. Splitting them
# here rather than running `wsl` once per group keeps the one-invocation
# property this heredoc was written for.
for step in $steps; do
  cur=()
  for a in "$@" "$gsep"; do
    if [ "$a" != "$gsep" ]; then cur+=("$a"); continue; fi
    if [ ${#cur[@]} -gt 0 ]; then
      echo ""
      echo "=== linux $step ($target) ${cur[*]} ==="
      # `+nightly` for the reason userspace/.cargo/config.toml states in
      # capitals: this workspace's unstable settings are silently ignored by
      # stable, and a silently-ignored setting is how you get a green run of
      # the wrong thing.
      "$cargo" +nightly "$step" "${cur[@]}" --target "$target" \
        --target-dir "$tdir" || rc=1
    fi
    cur=()
  done
done
exit $rc
WSLEOF
)
  local rc=0
  wsl -e bash -c "$script" -- "$inside" "$steps" "$linux_target" "$GSEP" \
      "${groups[@]}" || rc=$?
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

# The one thing written to stdout, and written with `echo` rather than `note`
# for exactly that reason: this is the verdict, and it is the only output of
# this script that no WSL handle can overwrite.
echo ""
echo "=== summary ==="
echo "packages: ${pkgs[*]}"
# What was actually compiled, not just what was asked for. A package that has
# no library is checked with `--bins` alone, and a verdict that did not say so
# would leave the reader to assume a lib was linted that does not exist.
if [ ${#lib_pkgs[@]} -gt 0 ]; then echo "lib+bins: ${lib_pkgs[*]}"; fi
if [ ${#bin_pkgs[@]} -gt 0 ]; then echo "bins:     ${bin_pkgs[*]}"; fi
if [ ${#ran[@]} -gt 0 ]; then echo "ran:      ${ran[*]}"; else echo "ran:      (nothing)"; fi
if [ ${#skipped[@]} -gt 0 ]; then echo "declined: ${skipped[*]}"; fi
case "$status" in
  # Named, not "both halves clean": a run narrowed with --only is a smaller
  # claim than a full one, and a summary that overstates its own scope is the
  # same defect one level up from the one this script exists to close.
  0) echo "result:   clean (${ran[*]} half checked)" ;;
  # "on stderr", not "above": the summary is the whole of stdout now, so for a
  # caller that captured the two streams separately there is nothing above it.
  1) echo "result:   FAILED -- see the log on stderr" ;;
  2) echo "result:   INCOMPLETE -- a half could not run, which is not a pass" ;;
esac
exit "$status"
