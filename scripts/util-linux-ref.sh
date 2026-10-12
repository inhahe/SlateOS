# shellcheck shell=sh
#
# Builds util-linux 2.39.3's `cal` from the release, linked as Ubuntu links.
#
# For harnesses whose reference is a util-linux program Ubuntu does not
# install.
#
# ## Why a built reference
#
# Ubuntu's `/usr/bin/cal` is BSD's (`ncal`); util-linux's `cal` is in no
# package. `cal-diff.sh` used to compare against a `/usr/local/bin/cal` built
# by hand in August 2026, configured before libncurses-dev was installed --
# so without libtinfo, and with `colors_terminal_is_ready` (lib/colors.c)
# answering "no" for every terminal: that `cal` coloured nothing on a terminal
# unless told `--color=always`. Every util-linux program Ubuntu does ship --
# `hexdump`, `lscpu`, `dmesg` -- is linked against libtinfo and asks terminfo
# whether the terminal has colours. This builds `cal` the way they are built:
# the release, configured with its defaults, which find libtinfo and turn
# colours on by default. Nothing else is built (a minute, once).
#
# The release is util-linux-source.sh's -- fetched, checked against
# kernel.org's SHA-256 and unpacked once into its cache -- and is built out
# of tree, in a directory of this file's own, so the shared source stays as
# the harnesses that read its tests/ expect it.
#
# Sourced by a harness BEFORE it sources diff-wsl.sh, as util-linux-source.sh
# and procps-ref.sh are, for the same reasons -- the preamble re-execs the
# harness, so the build must be a no-op the second time (it is, by the stamp
# it checks), and on the Windows host this does nothing at all:
#
#     . "$(dirname "$0")/util-linux-ref.sh"
#     DIFF_REF=$UL_REF_CAL
#
# A fetch, check or build that fails leaves the program missing, and
# diff-wsl.sh then finds no reference and says so rather than compare against
# something else. So does a build that did not find libtinfo: that is the
# reference this replaced.

# shellcheck source=util-linux-source.sh
. "$(dirname "$0")/util-linux-source.sh"

UL_REF_BUILD=$HOME/.cache/slateos-ul-ref/build-2.39.3
# Every program off but the ones named; the rest of configure's choices are
# its defaults -- libtinfo when it is there, colours on by default. Static, so
# `cal` is the program rather than libtool's wrapper script.
UL_REF_CONFIGURE='--quiet --disable-all-programs --enable-cal --disable-nls --disable-shared --without-systemd'
UL_REF_CAL=$UL_REF_BUILD/cal

ul_ref_build() {
  [ "$(uname -s)" = Linux ] || return 0
  # The stamp records the configuration, so a change to it rebuilds.
  if [ -x "$UL_REF_CAL" ] \
     && [ "$(cat "$UL_REF_BUILD/.slateos-built" 2>/dev/null)" = "$UL_REF_CONFIGURE" ]; then
    return 0
  fi
  # util-linux-source.sh could not fetch or check the release.
  [ -f "$UL_SRC/.unpacked" ] || return 0
  printf 'util-linux-ref.sh: building util-linux 2.39.3 cal (once)\n' >&2
  rm -rf "$UL_REF_BUILD"
  mkdir -p "$UL_REF_BUILD" || return 0
  # shellcheck disable=SC2086 # the configure flags are words on purpose
  if ! ( cd "$UL_REF_BUILD" && "$UL_SRC/configure" $UL_REF_CONFIGURE >/dev/null \
         && make -s -j4 cal >/dev/null 2>&1 ); then
    printf 'util-linux-ref.sh: building util-linux 2.39.3 failed\n' >&2
    rm -f "$UL_REF_CAL"
    return 0
  fi
  if ! grep -q '^#define HAVE_LIBTINFO 1' "$UL_REF_BUILD/config.h"; then
    printf 'util-linux-ref.sh: configure did not find libtinfo (install libncurses-dev)\n' >&2
    rm -f "$UL_REF_CAL"
    return 0
  fi
  printf '%s' "$UL_REF_CONFIGURE" > "$UL_REF_BUILD/.slateos-built"
}

ul_ref_build
