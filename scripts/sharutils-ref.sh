# shellcheck shell=sh
#
# Unpacks Ubuntu's sharutils package, without root, as the uuencode/uudecode harness reference.
#
# The WSL image does not install sharutils, and installing needs root; but the
# package is an ordinary .deb, so `apt-get download` and `dpkg-deb -x` unpack
# it into a cache without it. Ubuntu 24.04's is sharutils 4.15.2-9, whose four
# Debian patches touch unshar, gnulib's stdio internals, a gcc-10 build fix in
# the generated headers and the configure macros -- none of the behaviour of
# uuencode, uudecode or their libopts -- so it stands for upstream 4.15.2.
#
# Sourced by a harness AFTER it sources diff-wsl.sh, which re-execs the harness
# from the top -- into WSL, then under `timeout` -- so that anything run before
# it runs again (scripts/check-diff-preamble-order.py):
#
#     DIFF_NO_REF=1
#     DIFF_NO_BINDIR=1
#     . "$(dirname "$0")/diff-wsl.sh"
#     . "$(dirname "$0")/sharutils-ref.sh"
#     ref=$(sharutils_ref uuencode) || exit 0
#
# `sharutils_ref NAME` unpacks the package unless it is cached and prints the
# unpacked `usr/bin/NAME`. A reference that cannot be had is a skip, said out
# loud, as the preamble's own missing reference is: the function says so and
# fails, and the harness exits 0 -- from itself, since an `exit` in here would
# only leave the command substitution.

SHARUTILS_ROOT=$HOME/.cache/slateos-sharutils/root

sharutils_fetch() {
  [ "$(uname -s)" = Linux ] || return 0
  [ -x "$SHARUTILS_ROOT/usr/bin/uuencode" ] && [ -x "$SHARUTILS_ROOT/usr/bin/uudecode" ] && return 0
  sharutils_dir=${SHARUTILS_ROOT%/root}
  mkdir -p "$sharutils_dir" || return 0
  (
    cd "$sharutils_dir" || exit 0
    apt-get download sharutils >/dev/null 2>&1 || exit 0
    for sharutils_deb in sharutils_*.deb; do
      [ -f "$sharutils_deb" ] && dpkg-deb -x "$sharutils_deb" root
    done
  )
}

# shellcheck disable=SC2154  # DIFF_PROG is the preamble's
sharutils_ref() {
  sharutils_fetch
  if [ ! -x "$SHARUTILS_ROOT/usr/bin/$1" ]; then
    echo "$DIFF_PROG-diff: could not unpack $1 from Ubuntu's sharutils package; skipping" >&2
    return 1
  fi
  printf '%s\n' "$SHARUTILS_ROOT/usr/bin/$1"
}
