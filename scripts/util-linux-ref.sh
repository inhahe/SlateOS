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
# Sourced by a harness BEFORE it sources diff-wsl.sh, as procps-ref.sh is,
# for the same reasons -- the preamble re-execs the harness, so the build must
# be a no-op the second time (it is, by the stamp it checks), and on the
# Windows host this does nothing at all:
#
#     . "$(dirname "$0")/util-linux-ref.sh"
#     DIFF_REF=$UL_REF_CAL
#
# A fetch, check or build that fails leaves the program missing, and
# diff-wsl.sh then finds no reference and says so rather than compare against
# something else.

UL_REF_VERSION=2.39.3
UL_REF_CACHE=$HOME/.cache/slateos-ul-ref
UL_REF_URL=https://mirrors.edge.kernel.org/pub/linux/utils/util-linux/v2.39/util-linux-$UL_REF_VERSION.tar.xz
# kernel.org's sha256sums.asc for the release, checked against it.
UL_REF_SHA256=7b6605e48d1a49f43cc4b4cfc59f313d0dd5402fa40b96810bd572e167dfed0f
UL_REF_SRC=$UL_REF_CACHE/util-linux-$UL_REF_VERSION
# Every program off but the ones named; the rest of configure's choices are
# its defaults -- libtinfo when it is there, colours on by default. Static, so
# `cal` is the program rather than libtool's wrapper script.
UL_REF_CONFIGURE='--quiet --disable-all-programs --enable-cal --disable-nls --disable-shared --without-systemd'
UL_REF_CAL=$UL_REF_SRC/cal

ul_ref_build() {
  [ "$(uname -s)" = Linux ] || return 0
  # The stamp records the configuration, so a change to it rebuilds.
  if [ -x "$UL_REF_CAL" ] \
     && [ "$(cat "$UL_REF_SRC/.slateos-built" 2>/dev/null)" = "$UL_REF_CONFIGURE" ]; then
    return 0
  fi
  mkdir -p "$UL_REF_CACHE" || return 0
  ul_ref_tar=$UL_REF_CACHE/util-linux-$UL_REF_VERSION.tar.xz
  if ! [ -f "$ul_ref_tar" ]; then
    printf 'util-linux-ref.sh: fetching util-linux %s\n' "$UL_REF_VERSION" >&2
    curl -fsSL -o "$ul_ref_tar.part" "$UL_REF_URL" || { rm -f "$ul_ref_tar.part"; return 0; }
    mv "$ul_ref_tar.part" "$ul_ref_tar"
  fi
  if [ "$(sha256sum "$ul_ref_tar" | cut -d' ' -f1)" != "$UL_REF_SHA256" ]; then
    printf 'util-linux-ref.sh: %s does not match its SHA-256; removed\n' "$ul_ref_tar" >&2
    rm -f "$ul_ref_tar"
    return 0
  fi
  printf 'util-linux-ref.sh: building util-linux %s cal (once)\n' "$UL_REF_VERSION" >&2
  rm -rf "$UL_REF_SRC"
  tar -C "$UL_REF_CACHE" -xJf "$ul_ref_tar" || return 0
  # shellcheck disable=SC2086 # the configure flags are words on purpose
  if ! ( cd "$UL_REF_SRC" && ./configure $UL_REF_CONFIGURE >/dev/null \
         && make -s -j4 cal >/dev/null 2>&1 ); then
    printf 'util-linux-ref.sh: building util-linux %s failed\n' "$UL_REF_VERSION" >&2
    rm -f "$UL_REF_CAL"
    return 0
  fi
  # A build without libtinfo is the very reference this replaced: refuse it.
  if ! grep -q '^#define HAVE_LIBTINFO 1' "$UL_REF_SRC/config.h"; then
    printf 'util-linux-ref.sh: configure did not find libtinfo (install libncurses-dev)\n' >&2
    rm -f "$UL_REF_CAL"
    return 0
  fi
  printf '%s' "$UL_REF_CONFIGURE" > "$UL_REF_SRC/.slateos-built"
}

ul_ref_build
