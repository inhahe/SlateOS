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
# Sourced, inside WSL, by a harness BEFORE it sources diff-wsl.sh:
#
#     . "$(dirname "$0")/util-linux-extra.sh"
#     DIFF_REF=$UL_EXTRA_ROOT/usr/bin/lsirq
#     . "$(dirname "$0")/diff-wsl.sh"
#
# On the Windows host, where the harness starts before diff-wsl.sh moves it
# into WSL, this does nothing; the fetch happens on the second pass. A fetch
# that fails leaves the reference missing, and diff-wsl.sh then skips the run
# rather than pass wrongly.

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

ul_extra_fetch
