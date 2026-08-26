#!/bin/sh
# osh-diff.sh — run the shell corpus against *glibc* bash, inside WSL.
#
# `scripts/osh-bash-diff.py` is the harness; this file is the ten lines that
# put it in front of the right two shells. Run on the Windows host it compares
# a `x86_64-pc-windows-gnu` osh against Git-for-Windows bash — a Cygwin port,
# linking `msys-2.0.dll` rather than glibc, with Cygwin's getopt wording, signal
# table, locale handling and process model. SlateOS is a GNU/Linux-shaped
# target, so that reference is wrong in the direction that matters: every
# behaviour it certified was learned from a port nobody runs. See
# `known-issues.md` → `TD-B-THE-SHELL-HARNESS-STILL-MEASURES-AGAINST-MSYS-BASH`
# for the four entries in that file which exist only because of it.
#
# Sourcing `diff-wsl.sh` is the whole fix. It re-execs into WSL, builds `osh`
# for `x86_64-unknown-linux-gnu` into the shared target directory, refuses to
# run against a build cache that silently did nothing, and pins the locale —
# the same four steps, from the same copy, as the twenty-five coreutils
# harnesses beside it.
#
#     sh scripts/osh-diff.sh                  # the whole corpus
#     sh scripts/osh-diff.sh -k trap          # cases matching "trap"
#     sh scripts/osh-diff.sh --timeout-scale 5
#
# Arguments are passed through to the Python untouched, so every flag it
# documents works here.
#
# ## Why `DIFF_NO_BINDIR`
#
# Not for either of the two reasons `diff-wsl.sh`'s header gives, and not for
# the wrong one it warns about — this harness rebuilds nothing. `$bindir` puts
# both sides on `PATH` under *one* name so that `argv[0]`, and therefore the
# `prog: ` prefix on every diagnostic, matches. Here the two sides are `osh`
# and `bash` and must keep their own names: `osh-bash-diff.py` strips each
# shell's own name from its diagnostics before comparing (`normalise_stderr`),
# which is how it compares the *message* rather than the program, and it can
# only do that if it knows which name to strip. A shim that called both `osh`
# would defeat the very normalisation it was meant to help.
#
# ## The control
#
# `OURS` names a single binary, as it does in every single-subject harness
# here, and `diff-wsl.sh` skips the build when it is set. So
#
#     OURS=/usr/bin/bash sh scripts/osh-diff.sh
#
# runs the corpus with bash on both sides. Every case must match; anything that
# does not is a case that depends on something other than the shell (the clock,
# the pid, the working directory), and anything reported as a stale
# `# EXPECT-DIFF` waiver is a waiver that was never about osh at all.
set -u

DIFF_PROG=osh
DIFF_PKG=oils
DIFF_BINS=osh
# `command -v bash` would answer for the *host's* bash on a host that has one
# earlier in `PATH`; naming the paths says which file is meant.
DIFF_REF="/usr/bin/bash /bin/bash"
DIFF_NO_BINDIR=1
DIFF_NEED="python3"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

# Not `exec`: that would replace this shell and with it the `EXIT` trap
# `diff-wsl.sh` set, leaking `$DIFF_TMP` on every run.
#
# `--osh` also turns off the Python's own staleness check, which is the right
# outcome rather than a loss: that check compares mtimes and can only say the
# binary looks older than a source file, where `diff_assert_fresh` above has
# already built the thing and then verified the *library* artifact the build
# produced. It is the stronger of the two and it has already run.
python3 "$(dirname "$0")/osh-bash-diff.py" \
  --osh "$OURS" --bash "$gnu_real" "$@"
