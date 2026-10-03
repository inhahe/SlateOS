# shellcheck shell=sh
#
# Builds procps-ng 4.0.4's `w` from the release, for a harness whose reference
# must be the program itself rather than the one a distribution installs.
#
# ## Why a built reference
#
# `w-diff.sh` compares against procps' `w` configured as SlateOS's port is:
# `--enable-w-from` (the FROM column, `W_SHOWFROM`) and no logind
# (`--without-systemd --without-elogind`, so the sessions come from utmp, as
# they do on SlateOS). The installed `/usr/bin/w` on Ubuntu 24.04 is neither
# -- it asks logind -- and, given a utmp with a FROM column to print, it
# crashes with SIGSEGV, so it cannot be the reference even for the cases it
# shares. The release is fetched once into a cache, checked against its
# SHA-256, configured, and only `src/w` is built (a minute, once).
#
# Sourced by a harness BEFORE it sources diff-wsl.sh, which builds as it is
# sourced -- so the harness itself runs nothing before the preamble, as
# `check-diff-preamble-order.py` requires:
#
#     . "$(dirname "$0")/procps-ref.sh"
#     DIFF_REF=$PROCPS_REF_W
#
# The preamble re-execs the harness from the top -- into WSL, then under its
# time bound -- so this is sourced more than once, and the build must be a
# no-op the second time: it is, by the stamp it checks. On the Windows host it
# does nothing at all; the build happens on the pass inside WSL. A fetch,
# check or build that fails leaves `$PROCPS_REF_W` missing, and diff-wsl.sh
# then finds no reference and says so rather than compare against something
# else.

PROCPS_REF_VERSION=4.0.4
PROCPS_REF_CACHE=$HOME/.cache/slateos-procps-ref
# The release tarball as Debian mirrors it (`procps_4.0.4.orig.tar.xz` is the
# upstream `procps-ng-4.0.4.tar.xz`, byte for byte -- this is its SHA-256).
PROCPS_REF_URL=https://deb.debian.org/debian/pool/main/p/procps/procps_4.0.4.orig.tar.xz
PROCPS_REF_SHA256=22870d6feb2478adb617ce4f09a787addaf2d260c5a8aa7b17d889a962c5e42e
PROCPS_REF_SRC=$PROCPS_REF_CACHE/procps-ng-$PROCPS_REF_VERSION-wfrom
# `--disable-shared` makes `src/w` the program itself rather than libtool's
# wrapper script, so it can be run from anywhere.
PROCPS_REF_CONFIGURE='--quiet --disable-nls --without-systemd --without-elogind --without-ncurses --disable-kill --enable-w-from --disable-shared'
PROCPS_REF_W=$PROCPS_REF_SRC/src/w

procps_ref_build() {
  [ "$(uname -s)" = Linux ] || return 0
  # The stamp records the configuration, so a change to it rebuilds.
  if [ -x "$PROCPS_REF_W" ] \
     && [ "$(cat "$PROCPS_REF_SRC/.slateos-built" 2>/dev/null)" = "$PROCPS_REF_CONFIGURE" ]; then
    return 0
  fi
  mkdir -p "$PROCPS_REF_CACHE" || return 0
  procps_ref_tar=$PROCPS_REF_CACHE/procps-ng-$PROCPS_REF_VERSION.tar.xz
  if ! [ -f "$procps_ref_tar" ]; then
    printf 'procps-ref.sh: fetching procps-ng %s\n' "$PROCPS_REF_VERSION" >&2
    curl -fsSL -o "$procps_ref_tar.part" "$PROCPS_REF_URL" || { rm -f "$procps_ref_tar.part"; return 0; }
    mv "$procps_ref_tar.part" "$procps_ref_tar"
  fi
  if [ "$(sha256sum "$procps_ref_tar" | cut -d' ' -f1)" != "$PROCPS_REF_SHA256" ]; then
    printf 'procps-ref.sh: %s does not match its SHA-256; removed\n' "$procps_ref_tar" >&2
    rm -f "$procps_ref_tar"
    return 0
  fi
  printf 'procps-ref.sh: building procps-ng %s src/w (once)\n' "$PROCPS_REF_VERSION" >&2
  rm -rf "$PROCPS_REF_SRC" "$PROCPS_REF_CACHE/procps-ng-$PROCPS_REF_VERSION"
  tar -C "$PROCPS_REF_CACHE" -xJf "$procps_ref_tar" || return 0
  mv "$PROCPS_REF_CACHE/procps-ng-$PROCPS_REF_VERSION" "$PROCPS_REF_SRC" || return 0
  # shellcheck disable=SC2086 # the configure flags are words on purpose
  if ! ( cd "$PROCPS_REF_SRC" && ./configure $PROCPS_REF_CONFIGURE >/dev/null \
         && make -s -j4 src/w >/dev/null 2>&1 ); then
    printf 'procps-ref.sh: building procps-ng %s failed\n' "$PROCPS_REF_VERSION" >&2
    rm -f "$PROCPS_REF_W"
    return 0
  fi
  printf '%s' "$PROCPS_REF_CONFIGURE" > "$PROCPS_REF_SRC/.slateos-built"
}

procps_ref_build
