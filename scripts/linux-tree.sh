#!/usr/bin/env bash
# Fetch, verify and prepare the Linux tree the driver layer compiles against.
#
# design-decisions 1578: Linux drivers are compiled against Linux's own
# headers, from an unmodified tree of the release `linuxdrv/linux.pin` names,
# configured by `linuxdrv/linux.config` on top of `allnoconfig`.
#
#   bash scripts/linux-tree.sh prepare   # fetch if needed, configure, `make prepare`
#   bash scripts/linux-tree.sh fetch     # fetch, verify and unpack only
#   bash scripts/linux-tree.sh paths     # print the source and object directories
#   bash scripts/linux-tree.sh needs drivers/net/ethernet/intel/e1000e
#                                        # what that code uses and does not define
#
# Runs on a Linux host. From Git Bash on Windows it runs itself again in WSL
# (distribution `Ubuntu`, or $SLATEOS_WSL_DISTRO): the tree holds file names
# that differ only in case, which a Windows folder holds only with admin
# rights, so it lives in WSL's own filesystem at $SLATEOS_LINUX_ROOT (default
# ~/slateos-build/linux) -- outside the repository, with a README.txt there
# saying what it is and when it can go.
#
# Needs make, clang, ld.lld and the llvm-* tools, bc, perl, xz, curl, flex,
# bison, m4, pkg-config and libelf's headers (objtool, which x86-64 cannot
# configure away). `~/.local/opt/linux-hosttools/env.sh`, when present, is
# sourced first: Ubuntu's packages for the missing ones, unpacked without
# root.
#
# Linux's files are never edited. The source directory is unpacked once,
# verified by hash, and built out of tree (`O=`). A change Linux needed would
# be a patch under linuxdrv/patches/, applied at unpack time; there are none.
set -euo pipefail

die() { echo "linux-tree: $*" >&2; exit 1; }

case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
        distro=${SLATEOS_WSL_DISTRO:-Ubuntu}
        # /c/visual studio projects/... is /mnt/c/visual studio projects/...
        self=$(cd "$(dirname "$0")" && pwd)/$(basename "$0")
        case $self in
            /[a-zA-Z]/*) ;;
            *) die "cannot map $self into WSL" ;;
        esac
        exec env MSYS2_ARG_CONV_EXCL='*' wsl.exe -d "$distro" -e bash "/mnt$self" "$@"
        ;;
esac

script_dir=$(cd "$(dirname "$0")" && pwd)
repo=$(dirname "$script_dir")
pin="$repo/linuxdrv/linux.pin"
config="$repo/linuxdrv/linux.config"
patches="$repo/linuxdrv/patches"
root=${SLATEOS_LINUX_ROOT:-$HOME/slateos-build/linux}

# shellcheck disable=SC1091  # optional, and outside the repository
if [ -f "$HOME/.local/opt/linux-hosttools/env.sh" ]; then . "$HOME/.local/opt/linux-hosttools/env.sh"; fi

# One `<key> <value>` line of the pin; anything but exactly one is an error.
pin_field() {
    awk -v k="$1" '$1 == k { v = $2; n++ } END { if (n != 1) exit 1; print v }' "$pin"
}
version=$(pin_field version) || die "$pin: needs exactly one 'version' line"
sha=$(pin_field sha256) || die "$pin: needs exactly one 'sha256' line"
[[ $version =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?$ ]] || die "$pin: bad version '$version'"
[[ $sha =~ ^[0-9a-f]{64}$ ]] || die "$pin: bad sha256 '$sha'"

tarball="linux-$version.tar.xz"
url="https://cdn.kernel.org/pub/linux/kernel/v${version%%.*}.x/$tarball"
src="$root/linux-$version"
obj="$root/obj-$version"

# The LLVM tools Linux's build uses: unversioned names on PATH, or else the
# newest /usr/lib/llvm-N/bin (Ubuntu installs `clang` unversioned but the
# rest only there). `LLVM=<dir>/` is Linux's own spelling for a prefix.
llvm_setting() {
    if command -v llvm-ar >/dev/null && command -v clang >/dev/null; then
        echo 1
        return
    fi
    local dir
    dir=$(find /usr/lib -maxdepth 1 -name 'llvm-*' -type d 2>/dev/null | sort -V | tail -1)
    [ -n "$dir" ] && [ -x "$dir/bin/llvm-ar" ] || die "no LLVM toolchain (llvm-ar) found"
    echo "$dir/bin/"
}

# What the unpacked source must match: the tarball, and every patch.
source_identity() {
    local patch_digest=none
    if compgen -G "$patches/*.patch" >/dev/null; then
        patch_digest=$(cat "$patches"/*.patch | sha256sum | cut -d' ' -f1)
    fi
    echo "$sha $patch_digest"
}

write_readme() {
    mkdir -p "$root"
    cat >"$root/README.txt" <<EOF
What: the Linux source tree SlateOS's driver layer compiles against
(design-decisions 1578), the tarball it came from (dl/), and Linux's build
output (obj-*/: the prepared configuration and generated headers).
Made by: scripts/linux-tree.sh in the SlateOS repository, from the release
pinned in linuxdrv/linux.pin ($version on $(date -u +%Y-%m-%d)).
When it can go: any time -- the script fetches and prepares it again. Older
linux-*/ and obj-*/ directories than the pinned release are no longer used.
EOF
}

fetch() {
    write_readme
    local want marker
    want=$(source_identity)
    marker="$src/.slateos-unpacked"
    if [ -f "$marker" ] && [ "$(cat "$marker")" = "$want" ]; then
        return 0
    fi
    mkdir -p "$root/dl"
    local dl="$root/dl/$tarball"
    if ! { [ -f "$dl" ] && echo "$sha  $dl" | sha256sum -c --status; }; then
        echo "linux-tree: fetching $url"
        curl -fL --retry 3 -o "$dl.part" "$url"
        mv "$dl.part" "$dl"
    fi
    echo "$sha  $dl" | sha256sum -c --status ||
        die "$dl does not match the sha256 in $pin -- refusing to unpack it"
    echo "linux-tree: unpacking $tarball"
    rm -rf "$src.part"
    mkdir -p "$src.part"
    tar -xJf "$dl" -C "$src.part" --strip-components=1
    if compgen -G "$patches/*.patch" >/dev/null; then
        local p
        for p in "$patches"/*.patch; do
            patch -d "$src.part" -p1 --forward --batch --quiet <"$p" ||
                die "patch $p does not apply to $version"
        done
    fi
    echo "$want" >"$src.part/.slateos-unpacked"
    rm -rf "$src"
    mv "$src.part" "$src"
}

# Kconfig drops an option whose dependencies are unmet without a word, so a
# fragment line that did not take is found here rather than as a driver that
# compiled to nothing.
check_config() {
    local line opt bad=()
    while IFS= read -r line || [ -n "$line" ]; do
        case $line in
            CONFIG_*=*)
                grep -qxF -- "$line" "$obj/.config" || bad+=("$line")
                ;;
            "# CONFIG_"*" is not set")
                opt=${line#\# }
                opt=${opt%% *}
                if grep -q "^$opt=" "$obj/.config"; then bad+=("$line"); fi
                ;;
        esac
    done <"$config"
    if [ "${#bad[@]}" -gt 0 ]; then
        printf 'linux-tree: %s did not take in %s/.config:\n' "$config" "$obj" >&2
        printf '    %s\n' "${bad[@]}" >&2
        die "an option's dependencies are unmet; fix linux.config"
    fi
}

prepare() {
    fetch
    local llvm stamp want
    llvm=$(llvm_setting)
    want="$(source_identity) $(sha256sum <"$config" | cut -d' ' -f1) $llvm"
    stamp="$obj/.slateos-prepared"
    if [ -f "$stamp" ] && [ "$(cat "$stamp")" = "$want" ]; then
        echo "linux-tree: $obj is prepared"
        return 0
    fi
    rm -f "$stamp"
    mkdir -p "$obj"
    local make_args=(-C "$src" O="$obj" ARCH=x86_64 LLVM="$llvm")
    echo "linux-tree: configuring (allnoconfig + linux.config)"
    make "${make_args[@]}" KCONFIG_ALLCONFIG="$config" allnoconfig \
        >"$obj/allnoconfig.log" 2>&1 ||
        { tail -20 "$obj/allnoconfig.log" >&2; die "allnoconfig failed"; }
    check_config
    echo "linux-tree: make prepare"
    make "${make_args[@]}" -j"$(nproc)" prepare >"$obj/prepare.log" 2>&1 ||
        { tail -30 "$obj/prepare.log" >&2; die "make prepare failed (log: $obj/prepare.log)"; }
    echo "$want" >"$stamp"
    echo "linux-tree: prepared $obj"
}

# What a directory of Linux code calls that it does not define, grouped by the
# Linux file that exports each symbol: the inventory a driver's port starts
# from (design-decisions 1578 rule 4). Built by Linux's own build, so the
# objects are what the configuration makes of them.
needs() {
    local dir=${1:-}
    [ -n "$dir" ] || die "usage: linux-tree.sh needs <directory under the Linux tree>"
    dir=${dir%/}
    prepare
    [ -d "$src/$dir" ] || die "$src/$dir is not a directory"
    local llvm
    llvm=$(llvm_setting)
    make -C "$src" O="$obj" ARCH=x86_64 LLVM="$llvm" -j"$(nproc)" "$dir/" \
        >"$obj/needs.log" 2>&1 ||
        { tail -30 "$obj/needs.log" >&2; die "building $dir failed (log: $obj/needs.log)"; }
    local nm=llvm-nm
    [ "$llvm" = 1 ] || nm="${llvm}llvm-nm"
    # Every EXPORT_SYMBOL in the tree, once per source identity: a minute's
    # grep over the whole tree, so it is kept beside the objects.
    local exports="$obj/.slateos-exports" ident
    ident=$(source_identity)
    if [ ! -f "$exports.id" ] || [ "$(cat "$exports.id")" != "$ident" ]; then
        (cd "$src" && grep -rEo --include='*.c' --include='*.h' --include='*.S' \
            'EXPORT_SYMBOL(_GPL|_NS|_NS_GPL)?\([A-Za-z0-9_]+' . || true) |
            sed -E 's/^\.\/([^:]+):EXPORT_SYMBOL[A-Z_]*\(/\1 /' |
            awk '{ print $2, $1 }' | sort -u >"$exports"
        echo "$ident" >"$exports.id"
    fi
    local objs=()
    mapfile -t objs < <(find "$obj/$dir" -name '*.o' ! -name '*.mod.o' | sort)
    [ "${#objs[@]}" -gt 0 ] || die "building $dir produced no objects (is it enabled in linux.config?)"
    python3 - "$exports" <(
        "$nm" --undefined-only "${objs[@]}" | awk 'NF == 2 { print $2 }' | sort -u
    ) <(
        "$nm" --defined-only "${objs[@]}" | awk 'NF == 3 { print $3 }' | sort -u
    ) "${#objs[@]}" "$dir" <<'PY'
import collections
import sys

exports_path, undef_path, def_path, nobjs, directory = sys.argv[1:6]
where = collections.defaultdict(set)
with open(exports_path, encoding="utf-8") as fh:
    for line in fh:
        sym, path = line.split(maxsplit=1)
        where[sym].add(path.strip())
with open(def_path, encoding="utf-8") as fh:
    defined = {line.strip() for line in fh}
with open(undef_path, encoding="utf-8") as fh:
    need = sorted({line.strip() for line in fh} - defined - {""})
groups = collections.defaultdict(list)
for sym in need:
    # Other architectures' files, and tools/'s copies for user-space tools,
    # say nothing about this build. More than one candidate left (mm/vmalloc.c
    # and mm/nommu.c both export vfree) is shown as such: which one the
    # configuration compiles is not known from here.
    paths = sorted(p for p in where.get(sym, ())
                   if not p.startswith("tools/")
                   and (not p.startswith("arch/") or p.startswith("arch/x86/")))
    path = " | ".join(paths) if paths else "(no EXPORT_SYMBOL found)"
    groups[path].append(sym)
print(f"{directory}: {nobjs} object(s), {len(need)} symbol(s) used and not defined")
for path in sorted(groups, key=lambda p: (-len(groups[p]), p)):
    print(f"  {len(groups[path]):3}  {path}: {', '.join(groups[path])}")
PY
}

case "${1:-}" in
    fetch) fetch && echo "linux-tree: $src" ;;
    prepare) prepare ;;
    paths) printf 'src %s\nobj %s\n' "$src" "$obj" ;;
    needs) needs "${2:-}" ;;
    *) die "usage: linux-tree.sh fetch | prepare | paths | needs <dir>" ;;
esac
