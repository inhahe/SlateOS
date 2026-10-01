# shellcheck shell=sh
#
# Fetches util-linux 2.39.3's source and test data, for harnesses that need them.
#
# That is the release tarball, with the test data Ubuntu 24.04 adds to it,
# for the harnesses that need upstream's own test data rather than its
# programs.
#
# util-linux ships snapshots of real machines' /sys and /proc for its tests
# (`tests/ts/lscpu/dumps/*.tar.gz`: eighteen of them, from x86 laptops to
# s390 LPARs), and a program that reads a --sysroot can be compared against
# upstream on every one of them. Those snapshots are only in the source, not
# in any package, so this downloads the release tarball from kernel.org into
# a cache, checks it against the SHA-256 kernel.org publishes for it, and
# unpacks it once.
#
# The references these harnesses run are Ubuntu's builds, and Ubuntu patches
# lscpu with later upstream commits; two of its patches add to `tests/` (a
# nineteenth snapshot, a hybrid ARM machine with four CPU types, and an
# expected output that commit changed). Those two are fetched from
# Launchpad, checked against the SHA-256 recorded here, and applied with
# `git apply`, which is what can apply the snapshot's binary diff.
#
# Sourced, inside WSL, by a harness BEFORE it sources diff-wsl.sh:
#
#     . "$(dirname "$0")/util-linux-source.sh"
#     ... "$UL_SRC/tests/ts/lscpu/dumps" ...
#
# On the Windows host, where the harness starts before diff-wsl.sh moves it
# into WSL, this does nothing; the fetch happens on the second pass. A fetch
# or a check that fails leaves `$UL_SRC` without the file the harness looks
# for, and the harness then skips rather than pass wrongly. A test patch that
# cannot be had leaves only the eighteen snapshots.

UL_SRC_CACHE=$HOME/.cache/slateos-ul-src
UL_SRC=$UL_SRC_CACHE/util-linux-2.39.3
# kernel.org's sha256sums.asc for v2.39.
UL_SRC_SHA256=7b6605e48d1a49f43cc4b4cfc59f313d0dd5402fa40b96810bd572e167dfed0f
UL_SRC_URL=https://mirrors.edge.kernel.org/pub/linux/utils/util-linux/v2.39/util-linux-2.39.3.tar.xz
# Ubuntu noble's debian/patches/ubuntu/, as util-linux 2.39.3-9ubuntu6.6.
UL_SRC_PATCH_URL='https://git.launchpad.net/ubuntu/+source/util-linux/plain/debian/patches/ubuntu'
UL_SRC_PATCHES='
lp-2111723-0003-tests-update-lscpu-vmware_fpe-output.patch 2749f609e7dba35771ba6bd6a19fdf68e0c954d9c2df622f29cf8eb38afbd30a
lp-2111723-0004-tests-add-dump-from-ARM-with-A510-A710-A715-X3.patch 0a62a3e6f777f46e03686c866a5c5f5c23a7a7350b3e1881d6d45d559a3ee7b9
'

ul_src_fetch() {
  [ "$(uname -s)" = Linux ] || return 0
  [ -f "$UL_SRC/.unpacked" ] && return 0
  mkdir -p "$UL_SRC_CACHE" || return 0
  ul_src_tar=$UL_SRC_CACHE/util-linux-2.39.3.tar.xz
  if ! [ -f "$ul_src_tar" ]; then
    curl -fsSL -o "$ul_src_tar.part" "$UL_SRC_URL" || { rm -f "$ul_src_tar.part"; return 0; }
    mv "$ul_src_tar.part" "$ul_src_tar"
  fi
  if [ "$(sha256sum "$ul_src_tar" | cut -d' ' -f1)" != "$UL_SRC_SHA256" ]; then
    printf 'util-linux-source.sh: %s does not match its published SHA-256; removed\n' "$ul_src_tar" >&2
    rm -f "$ul_src_tar"
    return 0
  fi
  rm -rf "$UL_SRC"
  tar -C "$UL_SRC_CACHE" -xJf "$ul_src_tar" || return 0
  printf '%s\n' "$UL_SRC_PATCHES" | while read -r ul_src_patch ul_src_sum; do
    [ -n "$ul_src_patch" ] || continue
    ul_src_file=$UL_SRC_CACHE/$ul_src_patch
    if ! [ -f "$ul_src_file" ]; then
      if ! curl -fsSL -o "$ul_src_file.part" \
          "$UL_SRC_PATCH_URL/$ul_src_patch?h=applied/ubuntu/noble-security"; then
        rm -f "$ul_src_file.part"
        continue
      fi
      mv "$ul_src_file.part" "$ul_src_file"
    fi
    if [ "$(sha256sum "$ul_src_file" | cut -d' ' -f1)" != "$ul_src_sum" ]; then
      printf 'util-linux-source.sh: %s does not match its SHA-256; not applied\n' "$ul_src_patch" >&2
      rm -f "$ul_src_file"
      continue
    fi
    # `nowarn`: the snapshot's binary diff has lines git would otherwise
    # complain about, and print.
    (cd "$UL_SRC" && git apply --whitespace=nowarn "$ul_src_file") \
      || printf 'util-linux-source.sh: %s did not apply\n' "$ul_src_patch" >&2
  done
  : > "$UL_SRC/.unpacked"
}

ul_src_fetch
