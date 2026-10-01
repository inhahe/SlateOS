# shellcheck shell=sh
#
# Unpacks Ubuntu's util-linux-extra and rfkill packages, without root, as harness references.
#
# These are the util-linux programs Ubuntu packages apart from util-linux
# itself, needed as references by the differential harnesses: `util-linux-extra` (lsirq,
# irqtop, hwclock, lsfd, fincore, fadvise, pipesz, waitpid, blkpr) and
# `rfkill`. The WSL image has neither installed, and installing needs root;
# but both are ordinary .deb files, so `apt-get download` and
# `dpkg-deb -x` unpack them into a cache without it. They are built from the
# same util-linux 2.39.3 source as the installed package, and link against
# the installed libsmartcols.
#
# Sourced by a harness AFTER it sources diff-wsl.sh, which re-execs the harness
# from the top -- into WSL, then under `timeout` -- so that anything run before
# it runs again (scripts/check-diff-preamble-order.py). The harness tells the
# preamble that the reference and the PATH directories are its own business,
# and this provides both:
#
#     DIFF_NO_REF=1
#     DIFF_NO_BINDIR=1
#     . "$(dirname "$0")/diff-wsl.sh"
#     . "$(dirname "$0")/util-linux-extra.sh"
#     ul_extra_bindir usr/bin/lsirq
#
# `ul_extra_bindir PATH` unpacks the packages unless they are cached, then puts
# our binary and the unpacked reference on the preamble's two PATH directories
# under the harness's DIFF_PROG, as the preamble would have. A reference that
# cannot be had is a skip, said out loud, as the preamble's own missing
# reference is; a link that cannot be made is a failure.

UL_EXTRA_ROOT=$HOME/.cache/slateos-ul-extra/root

ul_extra_fetch() {
  [ "$(uname -s)" = Linux ] || return 0
  [ -x "$UL_EXTRA_ROOT/usr/bin/lsirq" ] && [ -x "$UL_EXTRA_ROOT/usr/sbin/rfkill" ] && return 0
  ul_extra_dir=${UL_EXTRA_ROOT%/root}
  mkdir -p "$ul_extra_dir" || return 0
  (
    cd "$ul_extra_dir" || exit 0
    apt-get download util-linux-extra rfkill >/dev/null 2>&1 || exit 0
    for ul_extra_deb in util-linux-extra_*.deb rfkill_*.deb; do
      [ -f "$ul_extra_deb" ] && dpkg-deb -x "$ul_extra_deb" root
    done
  )
}

# shellcheck disable=SC2154  # bindir, OURS and DIFF_PROG are the preamble's
ul_extra_bindir() {
  ul_extra_fetch
  ul_extra_ref=$UL_EXTRA_ROOT/$1
  if [ ! -x "$ul_extra_ref" ]; then
    echo "$DIFF_PROG-diff: could not unpack $1 from Ubuntu's packages; skipping"
    exit 0
  fi
  mkdir -p "$bindir/ours" "$bindir/gnu" || exit 1
  ln -s "$OURS" "$bindir/ours/$DIFF_PROG" || exit 1
  ln -s "$ul_extra_ref" "$bindir/gnu/$DIFF_PROG" || exit 1
}
