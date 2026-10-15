# shellcheck shell=sh
#
# Builds procps-ng 4.0.4's `w`, `ps`, `pgrep`, `pkill`, `pidwait`, `vmstat`,
# `sysctl` and `top` from the release, for harnesses whose reference must be
# the program itself rather than the one a distribution installs.
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
# SHA-256, configured, and only the programs below are built (a minute,
# once).
#
# `ps-diff.sh` compares against the same build's `ps`. Two of the flags are
# for it: no logind means the `unit`, `seat`, `machine` and other `sd_*`
# columns print `?`, as they must on SlateOS, which has no logind; and
# `--disable-numa` means the `numa` column is -1 rather than whatever a
# `libnuma` that happens to be installed in WSL says -- SlateOS has none, and
# without the flag the reference would `dlopen` it.
#
# `pgrep-diff.sh` compares against the same build's `pgrep`, `pkill` and
# `pidwait` -- three executables from one source, built as upstream builds
# them (`pidwait` is there because glibc has `pidfd_open`, so `configure`
# turns on `ENABLE_PIDWAIT`).
#
# `vmstat-diff.sh` compares against the same build's `vmstat`: the release's
# own, rather than the distribution's patched one, since the port says it
# transcribes 4.0.4. None of the configure flags reaches it. `sysctl-diff.sh`
# compares against the same build's `sysctl`, for the same reason.
#
# Sourced by a harness BEFORE it sources diff-wsl.sh, which builds as it is
# sourced -- so the harness itself runs nothing before the preamble, as
# `check-diff-preamble-order.py` requires:
#
#     . "$(dirname "$0")/procps-ref.sh"
#     DIFF_REF=$PROCPS_REF_W        # or $PROCPS_REF_PS, or the pgrep trio
#
# The preamble re-execs the harness from the top -- into WSL, then under its
# time bound -- so this is sourced more than once, and the build must be a
# no-op the second time: it is, by the stamp it checks. On the Windows host it
# does nothing at all; the build happens on the pass inside WSL. A fetch,
# check or build that fails leaves the programs missing, and diff-wsl.sh then
# finds no reference and says so rather than compare against something else.

PROCPS_REF_VERSION=4.0.4
PROCPS_REF_CACHE=$HOME/.cache/slateos-procps-ref
# The release tarball as Debian mirrors it (`procps_4.0.4.orig.tar.xz` is the
# upstream `procps-ng-4.0.4.tar.xz`, byte for byte -- this is its SHA-256).
PROCPS_REF_URL=https://deb.debian.org/debian/pool/main/p/procps/procps_4.0.4.orig.tar.xz
PROCPS_REF_SHA256=22870d6feb2478adb617ce4f09a787addaf2d260c5a8aa7b17d889a962c5e42e
PROCPS_REF_SRC=$PROCPS_REF_CACHE/procps-ng-$PROCPS_REF_VERSION-wfrom
# `--disable-shared` makes `src/w` and `src/ps/pscommand` the programs
# themselves rather than libtool's wrapper scripts, so they can be run from
# anywhere.
PROCPS_REF_CONFIGURE='--quiet --disable-nls --without-systemd --without-elogind --without-ncurses --disable-kill --disable-numa --enable-w-from --disable-shared'
PROCPS_REF_W=$PROCPS_REF_SRC/src/w
PROCPS_REF_PS=$PROCPS_REF_SRC/src/ps/pscommand
PROCPS_REF_PGREP=$PROCPS_REF_SRC/src/pgrep
PROCPS_REF_PKILL=$PROCPS_REF_SRC/src/pkill
PROCPS_REF_PIDWAIT=$PROCPS_REF_SRC/src/pidwait
PROCPS_REF_VMSTAT=$PROCPS_REF_SRC/src/vmstat
PROCPS_REF_SYSCTL=$PROCPS_REF_SRC/src/sysctl

# `top-diff.sh` compares against `top` configured as Ubuntu configures it --
# `--disable-modern-top`, the original defaults, and `--enable-watch8bit`, so
# it links ncursesw -- and, like the tree above, with no logind and no NUMA,
# which SlateOS has neither of. `top` cannot be built `--without-ncurses`, so
# it is a second tree, with its own configuration and stamp -- built only for
# a harness that asks for it before sourcing this, with `PROCPS_REF_TREES=top`
# (the default is `main`, the tree above), so that every other procps harness
# is not made to build a program it never runs.
PROCPS_REF_TOP_SRC=$PROCPS_REF_CACHE/procps-ng-$PROCPS_REF_VERSION-top
PROCPS_REF_TOP_CONFIGURE='--quiet --disable-nls --without-systemd --without-elogind --disable-kill --disable-numa --disable-modern-top --enable-watch8bit --disable-shared'
PROCPS_REF_TOP=$PROCPS_REF_TOP_SRC/src/top/top

# procps_ref_tree DIR CONFIGURE TARGET...: the release unpacked into DIR,
# configured with CONFIGURE, and the make targets TARGET (paths under DIR)
# built -- unless DIR's stamp says that configuration is already built and
# every target is there. A configuration that failed to build is not tried
# again on every run of every harness: DIR.failed records it, and says so,
# until it is deleted. Each tree unpacks in a directory of its own, so two
# harnesses building different trees at once do not share one.
procps_ref_tree() {
  procps_ref_dir=$1 procps_ref_conf=$2
  shift 2
  procps_ref_have=yes
  for procps_ref_t in "$@"; do
    [ -x "$procps_ref_dir/$procps_ref_t" ] || procps_ref_have=
  done
  # The stamp records the configuration, so a change to it rebuilds.
  if [ -n "$procps_ref_have" ] \
     && [ "$(cat "$procps_ref_dir/.slateos-built" 2>/dev/null)" = "$procps_ref_conf" ]; then
    return 0
  fi
  if [ "$(cat "$procps_ref_dir.failed" 2>/dev/null)" = "$procps_ref_conf" ]; then
    printf 'procps-ref.sh: %s failed to build before; delete %s to try again\n' \
      "$*" "$procps_ref_dir.failed" >&2
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
  printf 'procps-ref.sh: building procps-ng %s: %s (once)\n' "$PROCPS_REF_VERSION" "$*" >&2
  rm -rf "$procps_ref_dir" "$procps_ref_dir.unpack"
  mkdir -p "$procps_ref_dir.unpack" || return 0
  tar -C "$procps_ref_dir.unpack" -xJf "$procps_ref_tar" || return 0
  mv "$procps_ref_dir.unpack/procps-ng-$PROCPS_REF_VERSION" "$procps_ref_dir" || return 0
  rm -rf "$procps_ref_dir.unpack"
  # shellcheck disable=SC2086 # the configure flags are words on purpose
  if ! ( cd "$procps_ref_dir" && ./configure $procps_ref_conf >/dev/null \
         && make -s -j4 "$@" >/dev/null 2>&1 ); then
    printf 'procps-ref.sh: building procps-ng %s failed: %s\n' "$PROCPS_REF_VERSION" "$*" >&2
    for procps_ref_t in "$@"; do rm -f "$procps_ref_dir/$procps_ref_t"; done
    printf '%s' "$procps_ref_conf" > "$procps_ref_dir.failed"
    return 0
  fi
  rm -f "$procps_ref_dir.failed"
  printf '%s' "$procps_ref_conf" > "$procps_ref_dir/.slateos-built"
}

procps_ref_build() {
  [ "$(uname -s)" = Linux ] || return 0
  for procps_ref_which in ${PROCPS_REF_TREES:-main}; do
    case $procps_ref_which in
      main) procps_ref_tree "$PROCPS_REF_SRC" "$PROCPS_REF_CONFIGURE" src/w \
              src/ps/pscommand src/pgrep src/pkill src/pidwait src/vmstat src/sysctl ;;
      top) procps_ref_tree "$PROCPS_REF_TOP_SRC" "$PROCPS_REF_TOP_CONFIGURE" src/top/top ;;
      *) printf 'procps-ref.sh: no tree called %s\n' "$procps_ref_which" >&2 ;;
    esac
  done
}

procps_ref_build
