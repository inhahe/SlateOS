# shellcheck shell=sh
# shellcheck disable=SC3043  # `local`; see "Which dialect" below.
#
# Shared preamble for a differential harness that runs both sides inside WSL.
#
# Sourced, not run. It does the six things every such harness was doing for
# itself, identically, in about fifty lines apiece:
#
#   1. re-exec itself inside WSL if it is not already there
#   2. find the repository root as WSL sees it
#   3. find the GNU reference, and skip the run rather than pass wrongly
#   4. build our binary for `x86_64-unknown-linux-gnu`
#   5. fix the locale
#   6. put both binaries behind one name, so `argv[0]` matches on both sides
#
# ## Why any of it
#
# Two reasons, and both are load-bearing rather than convenience.
#
# The reference has to be glibc's. MSYS2 is a Cygwin derivative and its getopt
# is not glibc's -- `unknown option -- x` against `invalid option -- 'x'` --
# so a harness that compares against MSYS2 certifies wording no GNU/Linux
# system prints. `sort-diff.sh` did that for eight cases and passed the whole
# time (known-issues.md ->
# TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE).
#
# The subject has to be a Linux binary. Some of what these utilities do exists
# only there: `coreutils::stdfd`, which is what makes `prog >&-` behave, is
# `#[cfg(target_os = "linux")]` because the two runtime lies it undoes are
# undone with `.init_array` and raw `write(2)`. A Windows build cannot exercise
# a line of it, so a Windows-hosted harness cannot catch a regression in it.
#
# The build lands in `$HOME/.cache/slateos-diff-target` inside WSL, shared
# between harnesses and kept out of the repository's `target/` so the Linux and
# Windows builds do not invalidate each other (design-decisions.md §374).
#
# ## Which dialect
#
# `sh`, not `bash`. A sourced file has no shebang of its own, so shellcheck
# cannot infer one and checks nothing at all until told -- and what it should
# be told is the *most restrictive* shell that sources this, not the most
# common one. 45 of the 46 harnesses are bash; `osh-diff.sh` is `#!/bin/sh`,
# and declaring bash here would pass a bashism that breaks it, which is the
# failure a dialect declaration exists to prevent.
#
# One harness got that wrong in the other direction and the check found it:
# `ls-diff.sh` said `#!/bin/sh` while using process substitution, so `dash -n`
# rejected the whole file -- it could not have run under the shell it named.
# Its shebang now says bash, which is what it always was.
#
# The one POSIX rule this then breaks on purpose is `local`, which is not in
# the standard. `/bin/sh` in the WSL images these harnesses run under is dash,
# which implements it, as does bash. The alternative is to drop the three uses
# and let those variables leak into whichever harness sourced the file: the
# `diff_` prefix they carry makes a collision unlikely, but "unlikely" is a
# weaker guarantee than a scope, and this file is read by every harness that
# sources it. So the extension is kept and SC3043 is disabled at the top,
# rather than the scoping being given up to satisfy a shell nobody runs.
#
# ## Why the subject is built, every run
#
# A harness that names a path under the target directory and then merely *runs*
# it is not measuring the tree -- it is measuring whatever was last written to
# that path, which can be arbitrarily old and need not even come from the same
# crate. Both failure modes have happened here, and the second cost a day:
#
#   * **Stale.** `cargo test` and `cargo clippy` do not refresh a binary. A fix
#     verified by a unit test and then "measured" by the harness was measured
#     against the *previous* build. That was found in `printf-diff.sh`, and for
#     a while the fix -- build every run -- was applied to `printf` and `seq`
#     only.
#
#   * **The wrong program entirely.** Forty-two binary names in this workspace
#     are produced by *two* packages -- `coreutils` and a superseded standalone
#     `userspace/<name>` crate -- which cargo warns about ("output filename
#     collision") and then resolves by letting whichever built last win. So
#     `debug/bc` was sometimes `userspace/bc` and sometimes
#     `coreutils/src/bin/bc.rs`, two different implementations of bc. On
#     2026-08-21 `calc-diff.sh` reported "95 passed, 105 differed" and three
#     bugs were written up in `known-issues.md` against a bc that nobody
#     intends to ship; the bc that is shipped passes all 200. See
#     `known-issues.md` -> `B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES`.
#
# Naming the package (`DIFF_PKG`) as well as the binary is what closes the
# second hole. Building immediately before the harness reads the path closes
# the first -- and it is done every run, not only when the file is missing,
# because "is it there?" is exactly the question that lets a stale binary
# through, and a stale binary yields a *confident wrong answer* rather than an
# obvious failure.
#
# (This section used to live in `scripts/diff-subject.sh`, the host-side
# ancestor of this file. It was deleted once every harness had moved here --
# see design-decisions.md §382 -- and the reasoning was moved rather than lost.)
#
# ## Using it
#
# Set the knobs, then source it, before anything else in the harness:
#
#     DIFF_PROG='cat'
#     . "$(dirname "$0")/diff-wsl.sh"
#
# **Quote the value**, as above. It is a string and never a command to run,
# but shellcheck cannot tell a deliberate bare command *name* from a forgotten
# `$(...)`, so an unquoted `DIFF_PROG=cat` is SC2209 at severity `warning` --
# the severity `boot-test.sh`'s `check_shellcheck` gates at. This line is the
# one every new harness is copied from, so it was 37 of the 44 findings that
# stood between the tree and that gate (A->B request
# `a-b-shellcheck-floor-the-remaining-findings-are-all-yours.md`). All 50
# harnesses now quote it; keep it that way and the count stays at zero.
#
# (Careful when editing this header: a comment line whose *first word* is
# `shellcheck` is parsed as a directive, not prose. Getting that wrong here
# does not fail this file's own check -- it emits SC1073 and then every one of
# the 50 harnesses reports SC1094 "parsing of sourced file failed" and loses
# the `-x` suppressions, which turned 44 findings into 227. Keep such a word
# off the start of a line.)
#
# The harness's own arguments survive the re-exec, so a harness may parse `$@`
# after sourcing this as if it had never left the host.
#
# | knob | default | meaning |
# |---|---|---|
# | `DIFF_PROG`      | *required* | the utility's name: used for messages, for finding the reference, and as the one name both binaries are reached by |
# | `DIFF_PKG`       | `coreutils` | the cargo package(s) to build from; more than one for a harness whose subjects do not share a crate |
# | `DIFF_BINS`      | `$DIFF_PROG`, or empty if `DIFF_EXAMPLES` is set | the `--bin` names to build; more than one for a harness that compares a family |
# | `DIFF_EXAMPLES`  | (none) | `--example` names to build, for a harness whose subject is a test instrument rather than a shipped utility. `extfloat-probe` is one: it exposes a *library* to a C reference, and a `src/bin/*.rs` would be installed into the image |
# | `DIFF_FORWARD`   | (none) | extra environment variable names to carry across the re-exec, beyond `OURS` and `VERBOSE` |
# | `DIFF_REF`       | (none) | candidate paths for the reference, tried in order, instead of looking on `PATH`. `echo` needs this: `command -v echo` finds the shell builtin, which is not what is being compared. Single-binary harnesses only |
# | `DIFF_NEED`      | (none) | other commands that must exist inside WSL, or the run is skipped rather than run without them |
# | `DIFF_NO_REF`    | (unset) | do not look for a reference; the harness finds its own |
# | `DIFF_NO_BINDIR` | (unset) | do not build the `PATH` directories; the harness makes its own. See below — this is almost never what a harness wants |
#
# Afterwards it has set:
#
# | name | |
# |---|---|
# | `root`       | the repository root |
# | `target_dir` | the shared Linux target directory |
# | `OURS`       | our binary, absolute (a single `DIFF_BINS`, or a single `DIFF_EXAMPLES` and no `DIFF_BINS`) |
# | `gnu_real`   | the reference binary, absolute (single `DIFF_BINS`, unless `DIFF_NO_REF`) |
# | `DIFF_TMP`   | a scratch directory, removed on exit |
# | `bindir`     | `$DIFF_TMP/bin`, holding `ours/NAME` and `gnu/NAME` for each of `DIFF_BINS` |
# | `DIFF_SKIPPED` | the `DIFF_BINS` entries with no reference on this host (multi-binary only) |
# | `diff_ours`  | `diff_ours NAME` -> the path of another built binary |
# | `diff_ours_example` | the same for a `DIFF_EXAMPLES` name |
#
# A harness's own fixtures belong under `$DIFF_TMP`, so that the one `EXIT`
# trap set here cleans up everything. Setting a second `trap ... EXIT` would
# replace this one, not add to it, and leak the scratch directory every run;
# extend `diff_cleanup` instead if there is more to do.
#
# `OURS=/usr/bin/<prog>` overrides the build with the reference itself, which
# is how a harness is checked for still being able to tell the two apart: it
# should then report every xfail as an XPASS and nothing else. For a *family*
# harness `OURS` names the directory instead — `/usr/bin` — since there is no
# single subject for it to name.
#
# ## `DIFF_NO_BINDIR` is for three situations, and none is "I have a family"
#
# Three harnesses set it and then rebuilt this file's multi-binary `$bindir`
# by hand, name for name (`interleave-diff.sh`, `digest-diff.sh`,
# `write-error-diff.sh`, all converted 2026-08-25). Every one of the three had
# drifted from the copy here in the same direction: it took `OURS` as a
# directory without checking it *is* one, so a mistyped `OURS` skipped every
# name in silence rather than saying so. Two copies of one judgement is one
# copy that is wrong, and the wrong one is the one nobody rereads.
#
# The situations the knob is actually for:
#
# * **The subject has no same-named counterpart to symlink beside.**
#   `extfloat-diff.sh`: its subject is a `--example`, and its reference is a C
#   program it compiles itself.
# * **The reference is not known yet when this file runs.** `ls-diff.sh` builds
#   GNU coreutils 9.5 from source, because the 9.4 WSL ships lays out columns
#   differently; the symlinks cannot be made until that build has finished.
# * **The two sides must keep their own distinct names.** `osh-diff.sh`
#   compares `osh` against `bash`, and its harness strips each shell's *own*
#   name from that shell's diagnostics before comparing them, so that it
#   compares the message rather than the program. It can only do that if it
#   knows which name to strip; a `$bindir` that called both sides `osh` would
#   defeat the normalisation it was meant to serve.
#
# Anything else — including "my harness compares a family" — wants `DIFF_BINS`
# with more than one name in it and no `DIFF_NO_BINDIR` at all.

if [ -z "${DIFF_PROG:-}" ]; then
  echo "diff-wsl.sh: DIFF_PROG is not set" >&2
  exit 1
fi
: "${DIFF_PKG:=coreutils}"
: "${DIFF_EXAMPLES:=}"
# A harness whose subject is an example need build no binary at all, so
# `DIFF_BINS` only falls back to the utility's name when there is nothing else
# to build. Defaulting it unconditionally would ask cargo for a `--bin
# extfloat` that does not exist.
if [ -n "$DIFF_EXAMPLES" ]; then
  : "${DIFF_BINS:=}"
else
  : "${DIFF_BINS:=$DIFF_PROG}"
fi
: "${DIFF_FORWARD:=}"
: "${DIFF_REF:=}"
: "${DIFF_NEED:=}"

# MSYS would rewrite an argument that looks like a path on its way to `wsl`.
export MSYS2_ARG_CONV_EXCL='*'

# --- 1. get ourselves into WSL ------------------------------------------------
# `$0` may be an MSYS path (`/d/visual studio projects/...`) or a Windows one.
# `wslpath` translates whatever it is; it exists only inside WSL, which is also
# how we tell we are already there.
if ! command -v wslpath >/dev/null 2>&1; then
  if ! command -v wsl >/dev/null 2>&1; then
    echo "$DIFF_PROG-diff: no WSL on this host; skipping (ours is a unix-only binary)"
    exit 0
  fi
  diff_here=$(cd "$(dirname "$0")" && pwd)
  # `wsl wslpath` converts a *Windows* path; MSYS's own `/d/...` form is not
  # one, so hand over the mixed form cygpath produces, which WSL understands.
  if command -v cygpath >/dev/null 2>&1; then diff_here=$(cygpath -m "$diff_here"); fi
  diff_inside=$(wsl wslpath -u "$diff_here" 2>/dev/null) || {
    echo "$DIFF_PROG-diff: could not map $diff_here into WSL; skipping"
    exit 0
  }
  # The command line for `wsl -e` is built up in the positional parameters,
  # because this file is sourced by a `sh` that may have no arrays. It has to
  # end up as
  #
  #     env VAR=... VAR=... bash /path/to/harness ARG ARG ...
  #
  # and it starts out holding the harness's own arguments, so those are counted,
  # the environment and the command are appended after them, and then exactly
  # that many are rotated from the front to the back. A harness that takes
  # options -- `--cases`, `--flip`, `--keep` -- would otherwise lose them at the
  # WSL boundary and silently run its defaults.
  diff_argc=$#
  for diff_v in OURS VERBOSE $DIFF_FORWARD; do
    eval "set -- \"\$@\" \"$diff_v=\${$diff_v:-}\""
  done
  set -- "$@" bash "$diff_inside/$(basename "$0")"
  while [ "$diff_argc" -gt 0 ]; do
    set -- "$@" "$1"
    shift
    diff_argc=$((diff_argc - 1))
  done
  exec wsl -e env "$@"
fi

root=$(cd "$(dirname "$0")/.." && pwd) || exit 1

# --- 5. the locale ------------------------------------------------------------
# Fixed under UTF-8, as everywhere since §351: getopt renders an unknown or
# ambiguous option with directional single quotes under a UTF-8 locale and
# ASCII apostrophes under `C`, so the whole option-error family would disagree
# for a reason unrelated to the program being tested. `strerror` is
# locale-dependent too, which is why even a program with no text of its own
# needs this. `C.UTF-8` is present on every glibc build; a named territory
# locale is not.
export LC_ALL=C.UTF-8

# --- 3. the reference ---------------------------------------------------------
for diff_cmd in $DIFF_NEED; do
  if ! command -v "$diff_cmd" >/dev/null 2>&1; then
    echo "$DIFF_PROG-diff: no '$diff_cmd' inside WSL; skipping"
    echo "  the cases that need it would otherwise be green and meaningless."
    exit 0
  fi
done

gnu_real=
if [ -z "${DIFF_NO_REF:-}" ]; then
  if [ -n "$DIFF_REF" ]; then
    for diff_cand in $DIFF_REF; do
      [ -x "$diff_cand" ] && { gnu_real=$diff_cand; break; }
    done
  else
    gnu_real=$(command -v "$DIFF_PROG" 2>/dev/null) || gnu_real=
  fi
  if [ -z "$gnu_real" ]; then
    echo "$DIFF_PROG-diff: no GNU $DIFF_PROG inside WSL; skipping"
    exit 0
  fi
fi

# --- 4. the subject -----------------------------------------------------------
target_dir=$HOME/.cache/slateos-diff-target

# The path of one of the binaries built above.
diff_ours() {
  printf '%s/x86_64-unknown-linux-gnu/debug/%s' "$target_dir" "$1"
}

# The same, for a `--example`, which cargo puts one directory deeper.
diff_ours_example() {
  printf '%s/x86_64-unknown-linux-gnu/debug/examples/%s' "$target_dir" "$1"
}

# Did the build above actually rebuild what changed?
#
# `cargo build` exiting 0 is not that promise. On 2026-08-24 this target
# directory reached a state where cargo judged the `coreutils` *library* fresh
# while its artifacts told a different story: every binary that called a
# function added to `coreutils::stdfd` that morning failed to compile with
# `cannot find function `close_stdout` in module `stdfd``, and `cargo clean -p
# coreutils` was the whole fix.
#
# This comment used to add that the directory "had a `debug/` full of finished
# binaries and no `deps/` at all, so something had removed the intermediates"
# -- a disk that filled, or a kill during a write. That inference was wrong and
# is withdrawn (2026-08-25): cargo 1.100.0-nightly does not create `deps/` at
# all. It puts every unit under `debug/build/<pkg>/<hash>/out/`, as a clean
# build of a hello-world confirms. The missing directory was therefore evidence
# of nothing, and what actually let cargo call a stale library fresh is still
# unexplained. Which is the argument for the check below, not against it: it
# fires on the symptom, and the symptom is all anyone gets.
#
# A compile error is the *lucky* shape of that bug. The unlucky shape is a
# harness whose subject compiles against a stale library and passes, certifying
# a binary nobody built. "Why the subject is built, every run" above argues
# that a harness must not merely run whatever path it was given; this is the
# same argument one level down, because a build that silently did nothing is a
# path that was merely run.
#
# The check is the invariant a successful `cargo build` establishes: cargo's
# freshness for a path dependency is mtime-based, so any source file newer than
# the binary is a file the build should have reacted to and did not. One
# `cargo clean -p` and one retry, then refuse -- running anyway is how the
# false green happens.
#
# Scanned: `userspace/` (this package and every path dependency it has) plus
# the crates it reaches outside it. `target` directories are pruned, since a
# build's own `*.rs` output is always newer than the binary and always
# irrelevant.
diff_fresh_roots() {
  for diff_r in "$root/userspace" "$root/sha2" "$root/tzrules"; do
    [ -d "$diff_r" ] && printf '%s\n' "$diff_r"
  done
}

# The first source file newer than $1, or nothing.
diff_newer_than() {
  # shellcheck disable=SC2046
  find $(diff_fresh_roots) -name target -prune -o \
       -name '*.rs' -newer "$1" -print -quit 2>/dev/null
}

# The manifest for package $1, or nothing.
#
# One glob level, not a recursive search: `userspace/` alone holds several
# thousand package directories, and walking it costs minutes -- far more than
# the check it exists to serve. A package somewhere this does not reach is
# *reported*, never skipped; see `diff_lib_artifact`.
diff_manifest() {
  for diff_m in "$root"/*/"$1"/Cargo.toml "$root/$1/Cargo.toml"; do
    if [ -f "$diff_m" ]; then
      printf '%s\n' "$diff_m"
      return 0
    fi
  done
  return 1
}

# The library target name for the package manifested at $1, or nothing if that
# package has no library at all.
#
# These are cargo's own two rules, in cargo's order: an explicit `[lib] name`
# wins, and failing that a package has a library if and only if `src/lib.rs`
# exists, named after the package with dashes turned into underscores.
#
# Read from the manifest rather than taken as a knob, because a knob would be a
# second copy of something the tree already states -- and `oils`, whose library
# is named `osh`, is the standing proof that the copy nobody rereads is the one
# that goes wrong. Guessing the library name from the package name is precisely
# the bug this replaces.
diff_lib_name() {
  diff_explicit=$(awk '
    /^[ \t]*\[/ { diff_in = ($0 ~ /^[ \t]*\[lib\]/); next }
    diff_in && /^[ \t]*name[ \t]*=/ {
      sub(/^[^=]*=[ \t]*/, ""); sub(/[ \t]*(#.*)?$/, ""); gsub(/["'\'']/, "")
      print; exit
    }
  ' "$1")
  if [ -n "$diff_explicit" ]; then
    printf '%s\n' "$diff_explicit"
    return 0
  fi
  [ -f "${1%/Cargo.toml}/src/lib.rs" ] || return 0
  basename "${1%/Cargo.toml}" | tr - _
}

# The newest library artifact of package $1.
#
# Prints the path and returns 0; prints nothing and returns 0 when the package
# has no library; returns 1 when it has one and the build produced no artifact,
# and 2 when the package could not be found at all.
#
# ## Why this searches instead of naming a directory
#
# It named one until 2026-08-25: `debug/deps/lib<pkg>-*.rlib`, which was right
# when it was written and is now right nowhere. Cargo 1.100.0-nightly moved
# intermediates to `debug/build/<pkg>/<hash>/out/`, and `deps/` no longer
# exists; a clean build of a hello-world produces no such directory. Both of
# this check's inputs were therefore wrong at once -- the wrong folder and, for
# `oils`, the wrong filename -- so it matched nothing, found nothing to
# complain about, and passed. A check that cannot fail is not a check.
#
# So it asks where the artifact *is* rather than asserting where it should be.
# A layout the next toolchain invents costs nothing here as long as the file
# keeps its name, and if it ever stops being found the answer is a refusal to
# run, not a silent pass.
diff_lib_artifact() {
  diff_manifest_path=$(diff_manifest "$1") || return 2
  diff_libname=$(diff_lib_name "$diff_manifest_path")
  [ -z "$diff_libname" ] && return 0

  diff_found=$(find "$target_dir/x86_64-unknown-linux-gnu/debug" \
      -name "lib${diff_libname}-*.rlib" -printf '%T@ %p\n' 2>/dev/null \
    | sort -rn | head -1)
  [ -z "$diff_found" ] && return 1
  printf '%s\n' "${diff_found#* }"
}

# Print `SUBJECT|COMPLAINT` and succeed if $1 is stale; fail silently if not.
#
# The right-hand side is a whole predicate rather than just the offending
# filename, because there are now four ways to be stale and only one of them is
# "something is newer than this". `diff_assert_fresh` prints it verbatim.
diff_stale_one() {
  local diff_late
  if [ ! -f "$1" ]; then
    printf '%s|was not produced by the build at all\n' "$1"
    return 0
  fi
  diff_late=$(diff_newer_than "$1")
  [ -z "$diff_late" ] && return 1
  printf '%s|is older than %s\n' "$1" "$diff_late"
  return 0
}

# `SUBJECT|COMPLAINT` for the first stale artifact, or nothing.
# An artifact the build did not produce at all counts as stale.
#
# ## The library is checked first, and that is the point
#
# The obvious check -- "is any source newer than the binaries" -- passed on
# 2026-08-24 against a build whose *library* was three commits stale. Cargo had
# relinked every binary, so each one's mtime was newer than every source file
# and the check was satisfied, while the `coreutils` lib unit was replayed from
# cache: `stdfd`'s new `fflush (stdout)` before a diagnostic was simply not in
# them, and `interleave-diff.sh` reported sixteen differences against a fix
# that was in the tree and correct. `cargo clean -p coreutils` was the whole
# cure, and afterwards the same harness passed twenty-one for twenty-one.
#
# The tell was a replayed `dead_code` warning naming a function that had been
# unused only in the *previous* edit of the file -- a cached lib announcing
# itself. A per-binary mtime check cannot see that, because a binary's mtime
# says when it was linked and nothing about how old the code inside it is. So
# the artifact that actually holds the shared code is checked on its own.
diff_first_stale() {
  local diff_b diff_bin diff_late diff_lib diff_p diff_rc
  for diff_p in $DIFF_PKG; do
    diff_lib=$(diff_lib_artifact "$diff_p")
    diff_rc=$?
    if [ "$diff_rc" = 2 ]; then
      printf '%s|%s\n' "the package \`$diff_p\`" \
        "has no Cargo.toml anywhere under $root -- is DIFF_PKG right?"
      return 0
    fi
    if [ "$diff_rc" != 0 ]; then
      printf '%s|%s\n' "\`$diff_p\`'s library" \
        "is declared in its Cargo.toml, but the build produced no .rlib for it"
      return 0
    fi
    # Empty: the package has no library, so there is nothing here to be stale.
    [ -z "$diff_lib" ] && continue
    diff_late=$(diff_newer_than "$diff_lib")
    if [ -n "$diff_late" ]; then
      printf '%s|%s\n' "$diff_lib" "is older than $diff_late"
      return 0
    fi
  done
  for diff_b in $DIFF_BINS; do
    diff_bin=$(diff_ours "$diff_b")
    diff_stale_one "$diff_bin" && return 0
  done
  for diff_b in $DIFF_EXAMPLES; do
    diff_bin=$(diff_ours_example "$diff_b")
    diff_stale_one "$diff_bin" && return 0
  done
  return 0
}

diff_assert_fresh() {
  local diff_stale
  diff_stale=$(diff_first_stale)
  [ -z "$diff_stale" ] && return 0

  echo "$DIFF_PROG-diff: ${diff_stale%%|*}" >&2
  echo "  ${diff_stale#*|} -- the build cache is stale. Cleaning." >&2
  for diff_p in $DIFF_PKG; do
    ( cd "$root" && cargo clean -p "$diff_p" \
        --target x86_64-unknown-linux-gnu --target-dir "$target_dir" ) >&2
  done
  # shellcheck disable=SC2086
  ( cd "$root" && cargo build $diff_args \
      --target x86_64-unknown-linux-gnu --target-dir "$target_dir" ) >&2 || return 1

  diff_stale=$(diff_first_stale)
  [ -z "$diff_stale" ] && return 0
  echo "$DIFF_PROG-diff: ${diff_stale%%|*}" >&2
  echo "  STILL ${diff_stale#*|}, after a clean rebuild." >&2
  echo "  Refusing to run: the comparison would be against a binary nobody built." >&2
  return 1
}

OURS=${OURS:-}
if [ -z "$OURS" ]; then
  # shellcheck disable=SC1091
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  if ! command -v cargo >/dev/null 2>&1; then
    echo "$DIFF_PROG-diff: no cargo inside WSL; skipping"
    echo "  install one with:  wsl -e sh -c 'curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly --profile minimal'"
    exit 0
  fi
  # Built every run, for the reason the header gives under "Why the subject is
  # built, every run": a harness that merely *runs* a path measures whatever was
  # written there last, which need not be current and need not even be this
  # crate. One `cargo build` for the whole family rather than one per binary, so
  # the output does not read as though something were rebuilt between two halves
  # of a run.
  diff_args=
  for diff_p in $DIFF_PKG; do diff_args="$diff_args -p $diff_p"; done
  for diff_b in $DIFF_BINS; do diff_args="$diff_args --bin $diff_b"; done
  for diff_b in $DIFF_EXAMPLES; do diff_args="$diff_args --example $diff_b"; done
  # shellcheck disable=SC2086
  ( cd "$root" && cargo build $diff_args \
      --target x86_64-unknown-linux-gnu --target-dir "$target_dir" ) >&2 || exit 1
  diff_assert_fresh || exit 1
  # One subject gets named in `OURS`; a family does not, and its harness picks
  # what it needs with `diff_ours` / `diff_ours_example`.
  case $DIFF_BINS in
    ''|*' '*) ;;
    *) OURS=$(diff_ours "$DIFF_BINS") ;;
  esac
  if [ -z "$OURS" ] && [ -z "$DIFF_BINS" ]; then
    case $DIFF_EXAMPLES in
      *' '*) ;;
      *) OURS=$(diff_ours_example "$DIFF_EXAMPLES") ;;
    esac
  fi
fi
if [ -n "$OURS" ]; then
  if [ ! -x "$OURS" ]; then
    echo "$DIFF_PROG-diff: $OURS is not executable" >&2
    exit 1
  fi
  # Absolute, because the symlinks below are followed from a different
  # directory than the one the harness was started in.
  case $OURS in
    /*) ;;
    *) OURS=$(cd "$(dirname "$OURS")" && pwd)/$(basename "$OURS") ;;
  esac
fi

# --- 6. one scratch directory, and one name for both sides --------------------
DIFF_TMP=$(mktemp -d)

# Extend this rather than setting a second `EXIT` trap, which would replace
# this one. The `chmod` is for harnesses whose fixtures include an unreadable
# directory: `rm -rf` cannot descend into one.
diff_cleanup() {
  chmod -R u+rwx "$DIFF_TMP" 2>/dev/null
  rm -rf "$DIFF_TMP"
}
trap diff_cleanup EXIT

# --- 7. running one side without the shell's own commentary -------------------
# Run "$@" with its stderr going wherever the caller redirected *this
# function's* stderr, and with the shell's own job-status commentary going
# nowhere.
#
# A harness calls its `run_side` as
#
#     run_side ours "$@" 2>"$o_err"
#
# and a redirection on a *function call* redirects the shell's stderr for the
# duration of the call, not merely the child's. So when the child dies of a
# signal, bash's announcement of it -- `Aborted (core dumped)`, carrying the pid
# and the literal text of the command line -- lands in the very file the harness
# is about to compare byte for byte. Two sides that both abort then "differ",
# by pid, on every run forever.
#
# `od -w0` is exactly that: GNU 9.4 reaches `abort()` there, so the two sides
# were only ever both signalled under `OURS=/usr/bin/od`, where every case is
# the same binary and nothing should differ at all. That is the run that found
# this, and it is the argument for making that run part of the routine.
#
# fd 4 carries the caller's stderr past the shell's own, so the child still
# writes where the harness expects and only bash's messages are dropped.
# Nothing else writes to that stream: the command word is a symlink this file
# has already resolved and checked, so there is no `command not found` to lose.
diff_run() { { "$@" 2>&4; } 4>&2 2>/dev/null; }

bindir=$DIFF_TMP/bin
DIFF_SKIPPED=
if [ -z "${DIFF_NO_BINDIR:-}" ]; then
  # Each binary is reached through a symlink named after the utility, in a
  # directory that is the whole of `PATH` for that one invocation, so `argv[0]`
  # is the bare word on both sides and the `prog: ` prefix on every diagnostic
  # matches.
  mkdir -p "$bindir/ours" "$bindir/gnu"
  case $DIFF_BINS in
    '')
      # Only reachable from a harness whose subject is an example, since that
      # is the one case `DIFF_BINS` is allowed to be empty -- and such a
      # subject has no same-named reference to be symlinked beside.
      echo "diff-wsl.sh: DIFF_BINS is empty, so there is nothing to put on PATH" >&2
      echo "  (set DIFF_NO_BINDIR=1: an example has no counterpart in /usr/bin)" >&2
      exit 1
      ;;
    *' '*|*'	'*|*'
'*)
      # A family, or a harness whose subjects live in different crates. Each
      # name gets its own pair, and its own reference found by that name --
      # `DIFF_REF` names one path and so cannot describe more than one binary.
      #
      # A name with no reference on this host is *skipped*, not fatal: a family
      # harness is still worth running over the rest, and `DIFF_SKIPPED` says
      # out loud which ones did not run. That is the opposite of the
      # single-binary rule below, where no reference means there is nothing
      # left for the harness to do at all.
      #
      # The reference is looked for on the *filesystem*, not with `command -v`,
      # and that is not a stylistic preference: `command -v echo` -- and
      # `printf`, and `true` -- answers with the shell's own builtin, which is
      # not the program being compared and has neither its options nor its
      # diagnostics. `write-error-diff.sh` carries all three in `DIFF_BINS`.
      for diff_b in $DIFF_BINS; do
        diff_gnu=
        for diff_cand in "/usr/bin/$diff_b" "/bin/$diff_b"; do
          [ -x "$diff_cand" ] && { diff_gnu=$diff_cand; break; }
        done
        # `OURS` names a *directory* for a multi-binary harness, since there is
        # no single subject for it to name.
        if [ -n "$OURS" ] && [ -d "$OURS" ]; then
          diff_bin=$OURS/$diff_b
        else
          diff_bin=$(diff_ours "$diff_b")
        fi
        if [ -z "$diff_gnu" ] || [ ! -x "$diff_bin" ]; then
          DIFF_SKIPPED="$DIFF_SKIPPED $diff_b"
          continue
        fi
        ln -s "$diff_bin" "$bindir/ours/$diff_b"
        ln -s "$diff_gnu" "$bindir/gnu/$diff_b"
      done
      ;;
    *)
      ln -s "$OURS" "$bindir/ours/$DIFF_PROG"
      ln -s "$gnu_real" "$bindir/gnu/$DIFF_PROG"
      ;;
  esac
fi
