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
# | `DIFF_GNU_SOURCE`| (none) | a coreutils version (`9.4`) to fetch, build and compare against, *instead of* the installed binary. See "Why a built reference" below |
# | `DIFF_GNU_DIR`   | (none) | an already-built `coreutils-N/src` to use instead of building one. The escape hatch for `DIFF_GNU_SOURCE`; ignored without it |
# | `DIFF_GNU_CACHE` | `$HOME/.cache/slateos-diff-gnu` | where the tarball is downloaded and unpacked |
# | `DIFF_GNU_VERIFY_WITH` | the first `DIFF_BINS` entry | which binary in the built tree is asked for its `--version`. For the one utility that cannot answer; see "Why a built reference" below |
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
# | `gnu_dir`    | the built reference's `src` directory, or empty without `DIFF_GNU_SOURCE` |
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
# ## Why a built reference: the installed binary is evidence, not authority
#
# `DIFF_GNU_SOURCE=9.4` makes a harness compare against coreutils 9.4 *built
# from the GNU tarball*, rather than against `/usr/bin/<prog>`. It is opt-in
# per harness, because most of them are still on the installed binary and
# converting one may surface real differences that want fixing one at a time.
#
# The installed binary is not GNU. WSL's `coreutils` is Ubuntu's
# `9.4-3ubuntu6.1`, and Debian/Ubuntu carry behavioural patches on top of the
# release. Two are known to reach these harnesses:
#
# * **`df`/`du`**: `devtmpfs` and `squashfs` are added to gnulib's dummy-file
#   system list, so the reference omits the `/dev` row that upstream prints.
#   Found 2026-08-29; `design-decisions.md` §700 has the whole elimination.
# * **`cp -n`**: Debian 9.4-3 "revert cp -n behavior to debian 12 & prior"
#   (Debian #1058752) plus "add deprecation/compatibility warning for above".
#   Upstream's `-n` is `I_ALWAYS_NO` -- `cp: not replacing 'b'`, exit 1. The
#   installed one is `I_ALWAYS_SKIP` -- silent, exit 0 -- and prints
#   `cp: warning: behavior of -n is non-portable and may change in future; use
#   --update=none instead`, a string that appears nowhere in the 9.4, 9.5, 9.6
#   or master source trees. Found 2026-08-30.
#
# The changelog names four more patches whose blast radius includes a harness
# here -- `uname -i -p` (no harness yet), `tail` on sysfs, `split`'s
# CVE-2024-0684 fix, `ls -l` on NFS -- so the two above are a lower bound, not
# a list.
#
# §700 decided the policy on the first one: **our source follows upstream, and
# the harness removes the divergence from the comparison rather than from the
# subject.** It also named the trigger for revisiting *how*: "if a second
# distribution patch is ever found, [building a pristine reference] becomes the
# right answer and should be revisited for all the harnesses at once." `cp -n`
# is that second patch, so this is the mechanism it asked for.
#
# The cost §700 rejected it on has already been paid, twice over: `ls-diff.sh`
# had been building coreutils 9.5 from source since §366 for an unrelated
# reason, so the fetch-configure-make-cache-verify sequence existed and worked
# -- it was simply written inside one harness where the other 49 could not
# reach it. This is that block, moved here and generalised. `ls-diff.sh` is now
# a caller of it rather than the sole owner, which also retires the "two copies
# of one judgement" hazard the `DIFF_NO_BINDIR` section below warns about.
#
# Three rules the block keeps from `ls-diff.sh`, all of which matter:
#
# * **Skip rather than fall back.** If the tarball cannot be fetched or built,
#   the harness exits 0 saying so. Quietly comparing against the installed
#   binary instead would be the exact failure the knob exists to prevent, and
#   it would look green.
# * **Verify the version.** A reference of the wrong version fails cases that
#   are right and passes cases that are wrong, in one run. `--version` is
#   checked against `DIFF_GNU_SOURCE` and a mismatch is fatal, not a skip.
#
#   One utility cannot be asked. `test --version` prints nothing and exits 0 --
#   measured, on both the built 9.4 and the installed binary -- because `test`
#   has no options at all: `--version` is an ordinary one-argument expression,
#   and a non-empty string is true. So the check reads "not coreutils 9.4 (it
#   says: )" for a tree that is in fact exactly right. `DIFF_GNU_VERIFY_WITH`
#   is the answer: the whole package is built once from one tarball into one
#   directory, so *any* sibling in it attests the tree's version, and
#   `test-diff.sh` sets `DIFF_GNU_VERIFY_WITH=cat` to borrow one.
#
#   Note what this is not: a way to switch the check off. It stays a real
#   runtime check on a real binary, and stays fatal. The rejected alternative
#   was to skip verification whenever `--version` produced nothing
#   recognisable -- but "nothing recognisable" is also precisely what the wrong
#   binary would produce, so that would disarm the guard for every harness in
#   order to accommodate one.
# * **The whole package is built, once.** Not `make src/cp`: automake makes
#   `BUILT_SOURCES` for the default target only, and gnulib's replacement
#   headers are built sources, so naming a target directly fails in a way that
#   reads like a broken host. That mistake is why `ls-diff.sh` had been
#   skipping every run. The block below has the detail.
#
# ## `DIFF_NO_BINDIR` is for two situations, and neither is "I have a family"
#
# It was three until `DIFF_GNU_SOURCE` above absorbed the middle one. The
# retired entry read: "the reference is not known yet when this file runs --
# `ls-diff.sh` builds GNU coreutils 9.5 from source, so the symlinks cannot be
# made until that build has finished." That is no longer true of any harness,
# because the build now happens *here*, before the symlinks are made.
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
: "${DIFF_GNU_SOURCE:=}"
# The witness defaults to the *first binary the harness builds*, not to
# `DIFF_PROG`: a family harness names itself for the property it tests --
# `write-error`, `digest`, `interleave` -- and there is no `src/write-error` in
# the coreutils tree to ask. `DIFF_BINS` has already been defaulted to
# `DIFF_PROG` above, so a single-binary harness gets exactly what it would have
# either way.
if [ -z "${DIFF_GNU_VERIFY_WITH:-}" ]; then
  for diff_w in $DIFF_BINS; do DIFF_GNU_VERIFY_WITH=$diff_w; break; done
  : "${DIFF_GNU_VERIFY_WITH:=$DIFF_PROG}"
fi
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
  # `DIFF_GNU_DIR` and `DIFF_GNU_CACHE` are carried unconditionally, unlike
  # `DIFF_GNU_SOURCE`: the version is written *in* the harness and so is set
  # again on the far side, while these two are the operator's overrides and
  # exist only in the environment this side of the boundary.
  for diff_v in OURS VERBOSE DIFF_GNU_DIR DIFF_GNU_CACHE $DIFF_FORWARD; do
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

# A pristine GNU, fetched and built, for the harnesses that ask for one. The
# header section "Why a built reference" says why they do; this is the how.
#
# ## `make`, and not `make src/cp`
#
# The obvious economy -- build only the binaries this harness names -- does not
# work, and its failure is silent enough to be worth spelling out. Automake
# guarantees `BUILT_SOURCES` are made before the *default* target only:
#
#     all: $(BUILT_SOURCES)
#             $(MAKE) $(AM_MAKEFLAGS) all-am
#
# Naming a target directly skips that prerequisite. In coreutils the built
# sources are gnulib's replacement headers -- `lib/fcntl.h`, `lib/stdckdint.h`,
# `lib/wchar.h` and forty more -- so `make src/ls` compiles gnulib against the
# *system* headers it was meant to be shielded from, and dies on `O_BINARY
# undeclared`, `stdckdint.h: No such file or directory`, `O_SEARCH undeclared`,
# `unknown type name 'wint_t'`. Nothing in that output says "you named the
# wrong target"; it reads like a host missing its C library.
#
# This is not hypothetical either. `ls-diff.sh` had `make -s -jN src/ls` from
# §366 onward, so on this machine it had been **skipping every run** -- exiting
# 0 with "a C compiler and make are needed", on a host that has both. In the
# aggregate sweep that shows up as the harness's last line, which is the
# parenthetical hint rather than a count, and is easy to read past. So `ls` was
# uncertified for as long as that line existed. See `known-issues.md`,
# `B-LS-DIFF-HAS-BEEN-SKIPPING`.
#
# A whole build is also not the cost it looks like: about 90 seconds at `-j8`,
# once per version per machine, after which every other harness on that version
# pays nothing at all.
#
# ## A failed build is wiped, not kept
#
# Also measured: a tree left half-built by the broken `make src/ls` above does
# not recover when the right `make` is run over it. The stale objects were
# compiled against the wrong headers, and the link fails with `undefined
# reference to rpl_mbrtoc32` -- a third error message that names neither cause.
# Every failure path below therefore removes the tree, so the next run starts
# from the tarball rather than inheriting a wreck.

## The reference's own dependencies, which are not free
#
# A coreutils built by the block below on a host without libattr has `USE_XATTR`
# undefined, which is not a small thing: `copy_attr`'s entire body becomes
# `return true`, so the reference `cp` and `mv` silently carry no extended
# attributes at all, and `cp --preserve=xattr` refuses to run. Without libacl,
# `USE_ACL` goes the same way and `copy_acl` degrades to a plain `chmod`, so the
# reference loses access-control lists — which ours carries, since on this kernel
# an ACL *is* the extended attribute `system.posix_acl_access`.
#
# Either way a harness that writes a case about the thing compares our correct
# behaviour against a reference that cannot do it, and reports a difference *in
# us* — the one direction that is worst, because the natural reading of a red
# case is that the subject is wrong. That is not hypothetical: the first five
# extended-attribute cases ever written here came back exactly like that.
#
# The host has neither development package and no way to install one (`sudo`
# wants a password this script does not have), so both are **built from source
# into the same cache**, exactly as coreutils itself is and for the same reason.
# They are small C libraries; the two builds together take a few seconds. See
# `design-decisions.md` 744 for why this is preferred to an `xfail` that
# promises never to fail.
#
# The system is tried first and the source build is the fallback, so a host that
# does have the development packages pays nothing.
#
# Failure is never fatal at any step. A host with no network, or a library that
# will not build, gets the old behaviour — a reference without that support —
# and the harnesses that care say so in their own headers rather than here.
diff_dep_cppflags=
diff_dep_ldflags=
diff_xattr_ref=no
diff_acl_ref=no
diff_attr_version=2.5.2
diff_acl_version=2.3.2

# One prefix for both, and not one each: `acl` links against `attr`, so its own
# `configure` has to be able to find the `attr` this script may have just built.
# Sharing the prefix makes that the same `-I`/`-L` pair the reference gets, with
# nothing to keep in step.
diff_dep_cache=${DIFF_GNU_CACHE:-$HOME/.cache/slateos-diff-gnu}
diff_dep_prefix=$diff_dep_cache/deps-prefix
# `-Wl,-rpath` and not `LD_LIBRARY_PATH`: the reference binaries are run through
# symlinks in a throwaway `PATH` directory, by two different harnesses, in an
# environment the case controls. A library path baked into the binary cannot be
# lost by any of that, and cannot leak into the *subject's* environment either —
# which matters, because our binaries must not accidentally acquire a library
# the target does not have.
diff_dep_cpp=-I$diff_dep_prefix/include
diff_dep_ld="-L$diff_dep_prefix/lib -Wl,-rpath,$diff_dep_prefix/lib"

# Does a C program link with these flags? `diff_dep_links <cppflags> <ldflags>
# <libs>`, with the program itself on standard input.
#
# Its own scratch directory, and not `$DIFF_TMP`: these probes run while the
# reference is still being *located*, which is a few hundred lines before the
# harness's own temporary tree is made. Reaching forward to a variable that does
# not exist yet is how the first version of this failed, under `set -u`, with
# `DIFF_TMP: unbound variable` and no other output.
diff_dep_links() {
  command -v cc >/dev/null 2>&1 || return 1
  diff_dep_dir=$(mktemp -d) || return 1
  cat > "$diff_dep_dir/probe.c"
  # shellcheck disable=SC2086
  cc $1 -o "$diff_dep_dir/probe" "$diff_dep_dir/probe.c" $2 $3 >/dev/null 2>&1
  diff_dep_rc=$?
  rm -rf "$diff_dep_dir"
  return $diff_dep_rc
}

# Fetch, configure, build and install one dependency into the shared prefix.
# `diff_dep_build <name> <version> <url-directory>`. Silent, and never fatal: a
# failure leaves the tree removed so the next run starts from the tarball rather
# than inheriting a wreck, which is the same rule the coreutils build follows
# and for the same measured reason.
#
# The tree is removed on *success* too, which is the opposite of what the
# coreutils build does and is right for a different reason: coreutils is used
# from where it was built, so its tree is the product, while these two are used
# from the prefix they installed into and the tree is spoil. The tarball is
# kept, so nothing has to be re-fetched; what is thrown away is only the object
# files. What was actually achieved is read back from `config.h` further down,
# never inferred from this function's exit status.
diff_dep_build() {
  diff_dep_src=$diff_dep_cache/$1-$2
  diff_dep_tar=$1-$2.tar.gz
  if [ ! -f "$diff_dep_cache/$diff_dep_tar" ]; then
    diff_dep_url=$3/$diff_dep_tar
    mkdir -p "$diff_dep_cache" || return 1
    # `.part` and a rename, so an interrupted fetch cannot be mistaken for a
    # complete tarball on the next run.
    if command -v curl >/dev/null 2>&1; then
      curl -fsSL -o "$diff_dep_cache/$diff_dep_tar.part" "$diff_dep_url" \
        && mv "$diff_dep_cache/$diff_dep_tar.part" "$diff_dep_cache/$diff_dep_tar"
    elif command -v wget >/dev/null 2>&1; then
      wget -q -O "$diff_dep_cache/$diff_dep_tar.part" "$diff_dep_url" \
        && mv "$diff_dep_cache/$diff_dep_tar.part" "$diff_dep_cache/$diff_dep_tar"
    fi
  fi
  [ -f "$diff_dep_cache/$diff_dep_tar" ] || return 1
  rm -rf "$diff_dep_src"
  ( cd "$diff_dep_cache" \
    && tar xf "$diff_dep_tar" \
    && cd "$1-$2" \
    && ./configure --quiet --prefix="$diff_dep_prefix" \
         --disable-nls --disable-static \
         CPPFLAGS="$diff_dep_cpp" LDFLAGS="$diff_dep_ld" \
    && make -s -j"$(nproc 2>/dev/null || echo 4)" \
    && make -s install ) >/dev/null 2>&1 \
    || { rm -rf "$diff_dep_src"; return 1; }
  rm -rf "$diff_dep_src"
  return 0
}

# coreutils' own `m4/xattr.m4` probe, reduced to the part that decides.
diff_attr_usable() {
  diff_dep_links "$1" "$2" -lattr <<'C_PROBE'
#include <attr/error_context.h>
#include <attr/libattr.h>
static int perms(const char *n, struct error_context *c)
{ return attr_copy_action(n, c) == ATTR_ACTION_PERMISSIONS; }
int main(void) { return attr_copy_fd("/", 0, "/", 0, perms, 0); }
C_PROBE
}

# And `m4/acl.m4`'s. `acl_extended_file` is in it deliberately: it is the Linux
# entry point gnulib uses for the cheap "does this file have an ACL at all?"
# test, and a `sys/acl.h` without it is a different library wearing the name.
diff_acl_usable() {
  diff_dep_links "$1" "$2" -lacl <<'C_PROBE'
#include <sys/acl.h>
int main(void)
{
  acl_t a = acl_get_file("/", ACL_TYPE_ACCESS);
  if (a) acl_free(a);
  return acl_extended_file("/") < 0;
}
C_PROBE
}

if [ -n "$DIFF_GNU_SOURCE" ] && [ -z "${DIFF_GNU_DIR:-}" ]; then
  # attr first, and not merely for tidiness: `acl` will not configure without
  # it, so a host missing both has to get them in this order.
  if diff_attr_usable '' ''; then
    diff_xattr_ref=yes
  else
    diff_attr_usable "$diff_dep_cpp" "$diff_dep_ld" \
      || diff_dep_build attr "$diff_attr_version" \
           https://download.savannah.nongnu.org/releases/attr
    if diff_attr_usable "$diff_dep_cpp" "$diff_dep_ld"; then
      diff_dep_cppflags=$diff_dep_cpp
      diff_dep_ldflags=$diff_dep_ld
      diff_xattr_ref=yes
    fi
  fi

  if diff_acl_usable '' ''; then
    diff_acl_ref=yes
  else
    diff_acl_usable "$diff_dep_cpp" "$diff_dep_ld" \
      || diff_dep_build acl "$diff_acl_version" \
           https://download.savannah.nongnu.org/releases/acl
    if diff_acl_usable "$diff_dep_cpp" "$diff_dep_ld"; then
      diff_dep_cppflags=$diff_dep_cpp
      diff_dep_ldflags=$diff_dep_ld
      diff_acl_ref=yes
    fi
  fi
fi

gnu_dir=${DIFF_GNU_DIR:-}
if [ -n "$DIFF_GNU_SOURCE" ] && [ -z "$gnu_dir" ]; then
  diff_gnu_cache=${DIFF_GNU_CACHE:-$HOME/.cache/slateos-diff-gnu}
  diff_gnu_src=$diff_gnu_cache/coreutils-$DIFF_GNU_SOURCE
  diff_gnu_tar=coreutils-$DIFF_GNU_SOURCE.tar.xz
  diff_gnu_hint="  (or set DIFF_GNU_DIR=/path/to/coreutils-$DIFF_GNU_SOURCE/src)"
  # The marker, not `src/$DIFF_PROG`: it is written only after a whole `make`
  # returned 0, so it distinguishes "built" from "a binary happens to exist",
  # which is exactly the distinction the half-built tree above destroys.
  #
  # It carries the *flags the tree was configured with* and not merely the fact
  # of a build, because the tree can now be built two ways. A cache holding a
  # coreutils built before libattr was available would otherwise be reused
  # forever, and the harnesses would go on comparing against a `cp` whose
  # `copy_attr` is `return true` — with nothing to say so, since the tree looks
  # built and is.
  #
  # The flags and not the *outcome*: a marker recording "xattr=no" on a host
  # where the probe says yes but coreutils' own configure disagrees would be
  # rebuilt on every single run, ninety seconds at a time, forever. Inputs
  # change once; outcomes can disagree with intent indefinitely. What was
  # actually achieved is read back from `config.h` below, where it costs a
  # `grep` rather than a build.
  diff_gnu_want="dep-flags=$diff_dep_cppflags|$diff_dep_ldflags"
  if [ "$(cat "$diff_gnu_src/.slateos-built" 2>/dev/null)" != "$diff_gnu_want" ]; then
    mkdir -p "$diff_gnu_cache" || exit 1
    if [ ! -f "$diff_gnu_cache/$diff_gnu_tar" ]; then
      diff_gnu_url=https://ftp.gnu.org/gnu/coreutils/$diff_gnu_tar
      # Downloaded to `.part` and renamed, so an interrupted fetch cannot be
      # mistaken for a complete tarball on the next run.
      if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o "$diff_gnu_cache/$diff_gnu_tar.part" "$diff_gnu_url" \
          && mv "$diff_gnu_cache/$diff_gnu_tar.part" "$diff_gnu_cache/$diff_gnu_tar"
      elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$diff_gnu_cache/$diff_gnu_tar.part" "$diff_gnu_url" \
          && mv "$diff_gnu_cache/$diff_gnu_tar.part" "$diff_gnu_cache/$diff_gnu_tar"
      fi
    fi
    if [ ! -f "$diff_gnu_cache/$diff_gnu_tar" ]; then
      echo "$DIFF_PROG-diff: could not fetch $diff_gnu_tar; SKIPPED"
      echo "  (put it in $diff_gnu_cache/)"
      echo "$diff_gnu_hint"
      exit 0
    fi
    rm -rf "$diff_gnu_src"
    ( cd "$diff_gnu_cache" \
      && tar xf "$diff_gnu_tar" \
      && cd "coreutils-$DIFF_GNU_SOURCE" \
      && ./configure --quiet --disable-nls \
           CPPFLAGS="$diff_dep_cppflags" LDFLAGS="$diff_dep_ldflags" \
      && make -s -j"$(nproc 2>/dev/null || echo 4)" ) >&2 || {
      rm -rf "$diff_gnu_src"
      echo "$DIFF_PROG-diff: coreutils $DIFF_GNU_SOURCE did not build; SKIPPED"
      echo "  (a C compiler, make and ~90s are needed)"
      echo "$diff_gnu_hint"
      exit 0
    }
    printf '%s\n' "$diff_gnu_want" > "$diff_gnu_src/.slateos-built"
  fi
  gnu_dir=$diff_gnu_src/src
fi
# What the reference can actually do, read from the tree rather than inferred
# from what it was asked to do. `gl_FUNC_XATTR` can decide `use_xattr=no` and
# only *warn*, so intent is not evidence: a tree configured with the flags can
# still hold `/* #undef USE_XATTR */`, and that is precisely the reference that
# silently drops every attribute. Harnesses that write cases about extended
# attributes must test this, not `diff_xattr_ref` above.
#
# Read relative to `$gnu_dir` and not to `$diff_gnu_src`, so that a reference
# supplied by hand through `DIFF_GNU_DIR` is judged by the same evidence. If it
# is a bare directory of binaries with no `lib/config.h` beside it, the answer
# is "no" -- which costs a skipped section and never a wrong verdict.
if [ -z "$gnu_dir" ] \
   || ! grep -q '^#define USE_XATTR 1' "$gnu_dir/../lib/config.h" 2>/dev/null
then
  diff_xattr_ref=no
fi
# The same reading for ACLs, and for the same reason: `gl_FUNC_ACL` also only
# warns, and a reference with `USE_ACL` undefined compiles `copy_acl` down to a
# plain `chmod`. That reference does not *fail* an ACL case -- it quietly
# carries the mode and drops the entries, so the case goes red against a
# correct `cp`. Separate from the xattr answer because the two libraries are
# separate builds and either can be present without the other.
if [ -z "$gnu_dir" ] \
   || ! grep -q '^#define USE_ACL 1' "$gnu_dir/../lib/config.h" 2>/dev/null
then
  diff_acl_ref=no
fi
# The name the harnesses read; `diff_xattr_ref` is this file's working variable
# and is not part of the interface. The suppression below is not a nuisance
# silencer: this file is *sourced*, so its only reader is in another file, and
# the checker works one file at a time and cannot see that use. It is left
# unexported deliberately, matching `DIFF_XATTR` and `DIFF_TMP`: the case
# scripts are run as children and have no business inheriting harness state.
#
# (A comment line beginning with the checker's own name is parsed as a
# directive, which is why that sentence is worded around it.)
# shellcheck disable=SC2034
DIFF_XATTR_REF=$diff_xattr_ref
# shellcheck disable=SC2034
DIFF_ACL_REF=$diff_acl_ref

# A reference of the wrong version is worse than none: it fails cases that are
# right and passes cases that are wrong, in the same run. Fatal rather than a
# skip, because unlike a missing compiler this cannot be an accident of the
# host -- something has handed us the wrong binary, and running on would report
# the mismatch as differences in our own program.
#
# What is verified is the *tree*, not each binary in it: `$gnu_dir` is one
# `make` of one tarball, so one sibling's `--version` attests all of them, and
# a family harness need not fork five processes to hear the same answer five
# times. That is also what makes `DIFF_GNU_VERIFY_WITH` sound -- see the
# "Verify the version" bullet in the header for the one utility that needs it.
diff_gnu_verified=
diff_gnu_verify() {
  [ -n "$DIFF_GNU_SOURCE" ] || return 0
  [ -z "$diff_gnu_verified" ] || return 0
  diff_gnu_verified=1
  diff_gnu_witness=$gnu_dir/$DIFF_GNU_VERIFY_WITH
  if [ ! -x "$diff_gnu_witness" ]; then
    echo "$DIFF_PROG-diff: cannot check the reference's version: no $DIFF_GNU_VERIFY_WITH at $diff_gnu_witness" >&2
    exit 1
  fi
  case $("$diff_gnu_witness" --version 2>/dev/null | head -1) in
    *" $DIFF_GNU_SOURCE") return 0 ;;
  esac
  echo "$DIFF_PROG-diff: $diff_gnu_witness is not coreutils $DIFF_GNU_SOURCE" >&2
  echo "  (it says: $("$diff_gnu_witness" --version 2>/dev/null | head -1))" >&2
  exit 1
}

gnu_real=
if [ -z "${DIFF_NO_REF:-}" ]; then
  if [ -n "$DIFF_GNU_SOURCE" ]; then
    # No fallback to `PATH` here, on purpose: a harness that asked for 9.4 and
    # silently got Ubuntu's is the one outcome this knob exists to prevent, and
    # it would look green.
    gnu_real=$gnu_dir/$DIFF_PROG
    if [ ! -x "$gnu_real" ]; then
      echo "$DIFF_PROG-diff: coreutils $DIFF_GNU_SOURCE has no $DIFF_PROG at $gnu_real" >&2
      exit 1
    fi
    diff_gnu_verify
  elif [ -n "$DIFF_REF" ]; then
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
        if [ -n "$DIFF_GNU_SOURCE" ]; then
          # The built tree is the only candidate, for the reason at
          # `gnu_real` above -- except that a *family* member missing from it
          # is a skip rather than fatal, matching the rule for the rest of
          # this arm.
          [ -x "$gnu_dir/$diff_b" ] && {
            diff_gnu=$gnu_dir/$diff_b
            diff_gnu_verify
          }
        else
          for diff_cand in "/usr/bin/$diff_b" "/bin/$diff_b"; do
            [ -x "$diff_cand" ] && { diff_gnu=$diff_cand; break; }
          done
        fi
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

# --- 8. extended attributes ---------------------------------------------------
# An extended attribute is a small named blob a filesystem stores beside a file
# — a SELinux label, a `user.mime_type`, a backup tool's bookkeeping. `cp -a`
# and `mv` both carry them, and neither harness could see them until this was
# written: `B-DIFF-HARNESSES-CANNOT-SEE-EXTENDED-ATTRIBUTES` was filed after a
# fix for a *dropped* attribute landed with the harness reporting an identical
# 341/0/11 before and after.
#
# ## Why Python and not `getfattr`
#
# `getfattr` is the obvious tool and is what the issue asked for. It is part of
# the `attr` package, which is not installed here and cannot be installed
# without a password this script does not have. Python is installed, is already
# a dependency of half the repository's tooling, and is in three ways the better
# instrument for this particular job:
#
# * **`os.listxattr(..., follow_symlinks=False)` reads a symlink's own
#   attributes.** `getfattr` needs `-h` for that and it is easy to leave off,
#   which would silently compare the *target's* attributes twice.
# * **The value is bytes, and stays bytes.** `getfattr -d` picks an encoding per
#   value (`text` or base64) by guessing, so an attribute whose value happens to
#   contain a quote is printed differently from one that does not. Here the rule
#   is fixed and stated in one place: printable ASCII as itself, anything else
#   as hex.
# * **The elision is one `if`** rather than a `-m` regex whose syntax differs
#   between implementations.
#
# If `attr` is ever installed this can be reconsidered, but there is nothing to
# gain by it: nothing about the comparison wants a subprocess per file.
#
# ## What is elided, and why only that
#
# `security.selinux` only. It is set by the *system's* policy from the path a
# file is created at, so the two sides get different labels for the same reason
# they get different inode numbers, and on a machine with SELinux disabled it is
# absent from both. Everything else is compared, including the rest of
# `security.*`: `security.capability` in particular is a real thing for these
# two programs to lose, since `chown` clears it and the copy has to notice.
DIFF_XATTR=
for diff_xa in python3 python; do
  command -v "$diff_xa" >/dev/null 2>&1 || continue
  # Present is not the same as working: a Python built without `os.setxattr`,
  # or a filesystem mounted `nouser_xattr`, would make every case report "no
  # attributes" rather than failing, which is the silent-blind-spot outcome the
  # issue was filed about. Probe by actually setting one.
  if "$diff_xa" - "$DIFF_TMP" >/dev/null 2>&1 <<'PY'
import os, sys, tempfile
fd, p = tempfile.mkstemp(dir=sys.argv[1])
os.close(fd)
try:
    os.setxattr(p, "user.diffprobe", b"1")
    assert os.getxattr(p, "user.diffprobe") == b"1"
finally:
    os.unlink(p)
PY
  then DIFF_XATTR=$diff_xa; break; fi
done

# One tree's extended attributes as sorted text: `<prefix><path> <name> <value>`,
# one line per attribute, nothing at all when the tree has none.
#
# Errors are swallowed per path rather than per run, which matches what
# `snapshot` does with `find`'s: a case that leaves an unreadable directory
# leaves one on both sides, so the blind spot is symmetric.
diff_xattrs_in() {
  [ -n "$DIFF_XATTR" ] || return 0
  [ -d "$2" ] || return 0
  "$DIFF_XATTR" - "$1" "$2" 2>/dev/null <<'PY'
import os, sys

prefix, root = sys.argv[1], sys.argv[2]
ELIDE = {"security.selinux"}


def show(value):
    """Printable ASCII as itself, anything else as hex — fixed, not guessed."""
    if value and all(32 <= b < 127 for b in value):
        return value.decode("ascii")
    return "0x" + value.hex()


lines = []
for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
    # A symlink to a directory lands in `dirnames` and is not descended into,
    # so listing both lists reaches every entry exactly once.
    for name in dirnames + filenames:
        path = os.path.join(dirpath, name)
        rel = os.path.relpath(path, root)
        try:
            names = os.listxattr(path, follow_symlinks=False)
        except OSError:
            continue
        for attr in names:
            if attr in ELIDE:
                continue
            try:
                value = os.getxattr(path, attr, follow_symlinks=False)
            except OSError:
                continue
            lines.append("%s%s %s %s" % (prefix, rel, attr, show(value)))
sys.stdout.write("".join(line + "\n" for line in sorted(lines)))
PY
  return 0
}

# Set one, for a fixture. `diff_setxattr <path> <name> <value>`, on the link
# itself rather than its target — a fixture that wants the target can name it.
# Silent when there is no Python: the case still runs and still compares
# everything else, and the missing comparison is announced once in the header
# rather than once per case.
diff_setxattr() {
  [ -n "$DIFF_XATTR" ] || return 0
  "$DIFF_XATTR" - "$1" "$2" "$3" <<'PY'
import os, sys
os.setxattr(sys.argv[1], sys.argv[2], sys.argv[3].encode(), follow_symlinks=False)
PY
}

## Access-control lists, which need a fixture and no comparison
#
# An ACL is a per-file permission list finer than the nine mode bits — "user
# alice may write this, group build may read it" — and `cp -p` is supposed to
# carry it. On Linux it is *stored* as the extended attribute
# `system.posix_acl_access`, which means `diff_xattrs_in` above already sees it
# and already compares it byte for byte. There is deliberately no
# `diff_getfacl`: adding one would be a second, weaker view of a thing already
# compared exactly, and two views that can disagree is worse than one that
# cannot.
#
# What is still needed is a way to *make* one, and that does need a tool.
# Writing the raw attribute by hand is possible — the format is a 4-byte version
# followed by `{tag, perm, id}` triples — but a fixture that hand-assembles the
# on-disk encoding is a fixture that can be wrong in a way no case would catch,
# and it would not be checked against the kernel's own validation the way
# `setfacl` is.
#
# The system's `setfacl` is preferred; the one built beside libacl (section
# above) is the fallback, and on this host it is the only one. Empty when
# neither works, in which case the ACL cases skip and say so — the same shape as
# `DIFF_XATTR`, and for the same reason.
DIFF_SETFACL=
for diff_sf in setfacl "$diff_dep_prefix/bin/setfacl"; do
  case $diff_sf in
    setfacl) command -v setfacl >/dev/null 2>&1 || continue ;;
    *) [ -x "$diff_sf" ] || continue ;;
  esac
  # Present is not working: a filesystem mounted `noacl` accepts nothing, and a
  # `setfacl` that cannot find its libacl exits before touching the file. Probe
  # by setting one and confirming the attribute it is stored as appears — via
  # the same reader the cases will use, so a probe that passes guarantees a case
  # can see the result.
  diff_sf_dir=$(mktemp -d) || continue
  : > "$diff_sf_dir/probe"
  if "$diff_sf" -m u:0:rwx "$diff_sf_dir/probe" >/dev/null 2>&1 \
     && [ -n "$DIFF_XATTR" ] \
     && "$DIFF_XATTR" - "$diff_sf_dir/probe" >/dev/null 2>&1 <<'PY'
import os, sys
assert "system.posix_acl_access" in os.listxattr(sys.argv[1])
PY
  then
    rm -rf "$diff_sf_dir"
    DIFF_SETFACL=$diff_sf
    break
  fi
  rm -rf "$diff_sf_dir"
done

# Set one, for a fixture. `diff_setfacl <path> <spec>`, where the spec is
# whatever `setfacl -m` takes (`u:0:rwx`, `g:0:r-x`, `d:u:0:rwx` for a
# directory's default ACL). Silent when there is no usable tool, matching
# `diff_setxattr`: the case still runs, and the gap is announced once in the
# harness header rather than once per case.
diff_setfacl() {
  [ -n "$DIFF_SETFACL" ] || return 0
  "$DIFF_SETFACL" -m "$2" "$1" >/dev/null 2>&1
  return 0
}
