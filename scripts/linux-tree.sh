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
#   bash scripts/linux-tree.sh driver hello
#                                        # the driver process linuxdrv/hello/
#                                        # describes, into build/linuxdrv/hello.elf
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
# What this host lacks to build at all -- WSL, the toolchain, the network to
# fetch the tree the first time -- is exit 3, not 1, so that the boot test
# can tell "cannot build here" (it skips the driver processes, saying so)
# from "does not build" (it fails).
die_env() { echo "linux-tree: cannot build on this host: $*" >&2; exit 3; }

case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
        distro=${SLATEOS_WSL_DISTRO:-Ubuntu}
        # /c/visual studio projects/... is /mnt/c/visual studio projects/...
        self=$(cd "$(dirname "$0")" && pwd)/$(basename "$0")
        case $self in
            /[a-zA-Z]/*) ;;
            *) die "cannot map $self into WSL" ;;
        esac
        # WSL imports only the variables WSLENV names: the tree's location
        # is a WSL path, passed as it is.
        command -v wsl.exe >/dev/null 2>&1 || die_env "no WSL"
        wsl.exe -d "$distro" -e true >/dev/null 2>&1 ||
            die_env "no WSL distribution '$distro' (\$SLATEOS_WSL_DISTRO)"
        exec env MSYS2_ARG_CONV_EXCL='*' MSYS2_ENV_CONV_EXCL='SLATEOS_LINUX_ROOT' WSLENV="SLATEOS_LINUX_ROOT${WSLENV:+:$WSLENV}" \
            wsl.exe -d "$distro" -e bash "/mnt$self" "$@"
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
    [ -n "$dir" ] && [ -x "$dir/bin/llvm-ar" ] || die_env "no LLVM toolchain (llvm-ar) found"
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
        curl -fL --retry 3 -o "$dl.part" "$url" || die_env "could not fetch $url"
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

# What every C file of a driver process is compiled with on top of Linux's
# own flags (design-decisions 1580): the small code model, since the process
# is linked low rather than in the kernel's top 2 GiB, and the shadow headers
# forced in first. Both through KCFLAGS, which Linux's build puts last, so
# `-mcmodel=small` overrides its `-mcmodel=kernel`. `needs` uses them too, so
# the two never rebuild each other's objects.
shadow_dir() { echo "$obj/slateos-shadow"; }

# An LLVM tool by name, as llvm_setting's answer reaches it: unversioned on
# PATH ("1"), or in a directory.
tool() {
    local llvm
    llvm=$(llvm_setting)
    if [ "$llvm" = 1 ]; then echo "$1"; else echo "$llvm$1"; fi
}
driver_kcflags() { echo "-mcmodel=small -include $(shadow_dir)/shadow.h"; }

# Copy the shadow headers beside the objects -- a path without spaces, for
# KCFLAGS -- and check that each names the guard its Linux header really has:
# a guard that no longer matches lets both versions in, or neither.
install_shadow() {
    local dir file path guard real
    dir=$(shadow_dir)
    mkdir -p "$dir"
    # Only what changed: every object depends on these headers, so a fresh
    # copy's new mtime would have make rebuild them all on every run.
    for file in "$repo"/linuxdrv/shadow/*.h; do
        cmp -s "$file" "$dir/$(basename "$file")" || cp "$file" "$dir/"
    done
    for file in "$dir"/*.h; do
        read -r path guard < <(sed -n 's/^ \* SHADOWS \([^ ]*\) \([^ ]*\)$/\1 \2/p' "$file") || continue
        [ -n "${path:-}" ] || continue
        [ -f "$src/$path" ] || die "$file shadows $path, which $version does not have"
        real=$(sed -n 's/^#ifndef \([A-Za-z0-9_]*\)$/\1/p' "$src/$path" | head -1)
        [ "$real" = "$guard" ] ||
            die "$file shadows $path by guard $guard, but $version's guard is ${real:-missing}"
        path='' guard=''
    done
}

# Build the driver process `linuxdrv/<name>/` describes into
# build/linuxdrv/<name>.elf, linking the layer's Rust half, which cargo must
# have built first (linuxdrv/kit: `cargo build --release`). Both paths are
# the repository's own, not arguments: a path given from Git Bash would not
# be one in WSL, where this runs. What goes in is what the layer itself
# stands on, `linuxdrv/emul/manifest`, and then what the driver's own
# `manifest` adds: `linux <object>` for each of Linux's own objects (a path
# in the tree, built by Linux's make), `layer <file>` for each of the
# layer's C files (a path under linuxdrv/); a line the layer's already gives
# is taken once. Every function the result calls and nothing defines becomes
# a generated stop (1578 rule 4); a data symbol nothing defines is an error,
# to be written by hand.
driver() {
    local name=${1:-}
    [ -n "$name" ] || die "usage: linux-tree.sh driver <name under linuxdrv/>"
    # The workspace's target directory: linuxdrv/kit is a member.
    local kit="$repo/target/x86_64-unknown-none/release/liblxkit.a"
    local out="$repo/build/linuxdrv/$name.elf"
    mkdir -p "$(dirname "$out")"
    local layer_manifest="$repo/linuxdrv/emul/manifest" manifest="$repo/linuxdrv/$name/manifest"
    [ -f "$manifest" ] || die "$manifest: no such driver"
    [ -f "$layer_manifest" ] || die "$layer_manifest: the layer's own manifest is missing"
    [ -f "$kit" ] || die "$kit: the layer's Rust half is not built"
    prepare
    install_shadow
    local llvm work linux_objs=() layer_files=() kind path list
    local -A seen=()
    llvm=$(llvm_setting)
    for list in "$layer_manifest" "$manifest"; do
        while read -r kind path || [ -n "${kind:-}" ]; do
            case $kind in
                '' | '#'*) continue ;;
                linux | layer) ;;
                *) die "$list: '$kind' is neither linux nor layer" ;;
            esac
            [ -z "${seen["$kind:$path"]:-}" ] || continue
            seen["$kind:$path"]=1
            if [ "$kind" = linux ]; then linux_objs+=("$path"); else layer_files+=("$path"); fi
        done <"$list"
    done
    [ "${#linux_objs[@]}" -gt 0 ] || die "$layer_manifest and $manifest name no Linux object"
    # Linux's own objects, by Linux's own make.
    make -C "$src" O="$obj" ARCH=x86_64 LLVM="$llvm" KCFLAGS="$(driver_kcflags)" \
        -j"$(nproc)" "${linux_objs[@]}" >"$obj/driver-$name.log" 2>&1 ||
        { tail -30 "$obj/driver-$name.log" >&2; die "building $name's Linux objects failed"; }
    # The flags those were compiled with -- Linux's, with KCFLAGS -- for the
    # layer's own files: the saved command of the first object, without its
    # dependency file, input, output and per-object names.
    local first=${linux_objs[0]} cmdfile flags
    cmdfile="$obj/$(dirname "$first")/.$(basename "$first").cmd"
    [ -f "$cmdfile" ] || die "$cmdfile: no saved command for $first"
    flags=$(sed -n 's/^savedcmd_[^ ]* := //p' "$cmdfile" | sed 's/ ; .*//' |
        tr ' ' '\n' | grep -v -E '^(-Wp,-MMD.*|-c|-o|-DKBUILD_.*|-D__KBUILD_.*)$|\.[co]$|^$' | tr '\n' ' ')
    work="$obj/slateos-drivers/$name"
    rm -rf "$work"
    mkdir -p "$work/src"
    # The layer's sources beside the objects: a path without spaces.
    cp -r "$repo/linuxdrv/emul" "$repo/linuxdrv/$name" "$work/src/"
    local objs=() file base
    for file in "${linux_objs[@]}"; do objs+=("$obj/$file"); done
    for file in "${layer_files[@]}"; do
        # Named by its path, so that emul/pci.c and <driver>/pci.c are two.
        local out_obj base
        base=$(basename "$file" .c)
        out_obj="$work/$(echo "${file%.c}" | tr '/' '_').o"
        # -mstackrealign: Linux's flags keep the stack 8-aligned
        # (-mstack-alignment=8), and the layer's files call into the Rust
        # half, which the SysV ABI promises a 16-aligned stack.
        # -fmacro-prefix-map: __FILE__ as the repository names it, not the
        # scratch copy's path, as Linux's flags do for Linux's files.
        (cd "$obj" && eval "$flags -mstackrealign -fmacro-prefix-map='$work/src/=linuxdrv/' -DKBUILD_MODNAME='\"$name\"' -DKBUILD_BASENAME='\"$base\"' \
            -c -o '$out_obj' '$work/src/$file'") ||
            die "compiling $file failed"
        objs+=("$out_obj")
    done
    # The stops: every symbol used and defined nowhere -- not in the objects,
    # not in the Rust half, not by the linker script -- that is only ever
    # called is a function that stops the driver naming itself; anything
    # referenced otherwise is data, which a stop cannot stand in for.
    local nm dump
    nm=$(tool llvm-nm)
    dump=$(tool llvm-objdump)
    python3 - "$work/stops.c" <(
        # Strong ones only ("U"): a weak reference ("w") resolves to NULL, as
        # Linux's code expects of it, and a stop would turn its NULL check true.
        "$nm" --undefined-only "${objs[@]}" "$kit" 2>/dev/null | awk 'NF == 2 && $1 == "U" { print $2 }' | sort -u
    ) <(
        "$nm" --defined-only "${objs[@]}" "$kit" 2>/dev/null | awk 'NF == 3 { print $3 }' | sort -u
    ) <(
        "$dump" -r "${objs[@]}" "$kit" | awk '/R_X86_64/ { sub(/[-+]0x[0-9a-f]+$/, "", $3); print $2, $3 }' | sort -u
    ) "$repo/linuxdrv/link.ld" <<'PY'
import re
import sys

stops_path, undef_path, def_path, refs_path, script_path = sys.argv[1:6]
undef = {l.strip() for l in open(undef_path, encoding="utf-8")}
defined = {l.strip() for l in open(def_path, encoding="utf-8")}
script = open(script_path, encoding="utf-8").read()
# What the linker script defines: `name = ...;` at the start of a statement.
defined |= set(re.findall(r"(?m)^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=", script))
defined |= set(re.findall(r";\s*([A-Za-z_][A-Za-z0-9_]*)\s*=", script))
kinds = {}
for line in open(refs_path, encoding="utf-8"):
    kind, _, sym = line.strip().partition(" ")
    kinds.setdefault(sym, set()).add(kind)
missing = sorted(undef - defined)
data = [s for s in missing if kinds.get(s, set()) - {"R_X86_64_PLT32"}]
if data:
    sys.exit("linux-tree: data symbols nothing defines (write them by hand):\n    "
             + "\n    ".join(f"{s} ({', '.join(sorted(kinds.get(s, ())))})" for s in data))
with open(stops_path, "w", encoding="utf-8", newline="\n") as out:
    out.write("/* Generated by scripts/linux-tree.sh driver: each function a driver\n"
              " * calls that nothing defines stops the driver, naming itself\n"
              " * (design-decisions 1578 rule 4). It includes no Linux header. */\n"
              "void slx_stop(const char *what, const void *caller) __attribute__((noreturn));\n")
    for s in missing:
        out.write(f'void {s}(void) {{ slx_stop("{s}", __builtin_return_address(0)); }}\n')
print(f"linux-tree: {len(missing)} generated stop(s)")
PY
    # Called from Linux's code, whose stack is 8-aligned: realigned before
    # the call into the Rust half, as the layer's own files are.
    "$(tool clang)" --target=x86_64-linux-gnu -O2 -ffreestanding -fno-builtin -fno-pic \
        -mcmodel=small -mno-red-zone -mno-sse -mstackrealign -fno-asynchronous-unwind-tables \
        -c -o "$work/stops.o" "$work/stops.c"
    "$(tool ld.lld)" -T "$repo/linuxdrv/link.ld" --orphan-handling=error -static -nostdlib \
        -z max-page-size=0x4000 --gc-sections -o "$work/$name.elf" \
        "${objs[@]}" "$work/stops.o" "$kit" ||
        die "linking $name failed"
    cp "$work/$name.elf" "$out"
    echo "linux-tree: built $out"
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
    install_shadow
    local llvm
    llvm=$(llvm_setting)
    make -C "$src" O="$obj" ARCH=x86_64 LLVM="$llvm" KCFLAGS="$(driver_kcflags)" \
        -j"$(nproc)" "$dir/" \
        >"$obj/needs.log" 2>&1 ||
        { tail -30 "$obj/needs.log" >&2; die "building $dir failed (log: $obj/needs.log)"; }
    local nm=llvm-nm
    [ "$llvm" = 1 ] || nm="${llvm}llvm-nm"
    # Every EXPORT_SYMBOL in the tree, once per source identity: a minute's
    # grep over the whole tree, so it is kept beside the objects.
    local exports="$obj/.slateos-exports" ident
    # The scan's own version is part of the key, so a changed pattern is not
    # answered from a list the old one made.
    ident="$(source_identity) exports-v2"
    if [ ! -f "$exports.id" ] || [ "$(cat "$exports.id")" != "$ident" ]; then
        (cd "$src" && grep -rEo --include='*.c' --include='*.h' --include='*.S' \
            'EXPORT_SYMBOL[A-Z_]*\([A-Za-z0-9_]+' . || true) |
            sed -E 's/^\.\/([^:]+):EXPORT_SYMBOL[A-Z_]*\(/\1 /' |
            awk '{ print $2, $1 }' | sort -u >"$exports"
        echo "$ident" >"$exports.id"
    fi
    local objs=()
    mapfile -t objs < <(find "$obj/$dir" -name '*.o' ! -name '*.mod.o' | sort)
    [ "${#objs[@]}" -gt 0 ] || die "building $dir produced no objects (is it enabled in linux.config?)"
    python3 - "$exports" <(
        # The shadow headers' own names (slx_*) are the layer's, not Linux's.
        "$nm" --undefined-only "${objs[@]}" | awk 'NF == 2 && $2 !~ /^slx_/ { print $2 }' | sort -u
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

# The tree and its objects are one per machine, shared by every worktree:
# one build at a time, or one lane's `rm -rf` and `make` run through
# another's. A lane waits up to half an hour for another's to finish.
lock_tree() {
    mkdir -p "$root"
    exec 9>"$root/.slateos-lock"
    flock -w 1800 9 || die "another build of the Linux tree held its lock for half an hour"
}

case "${1:-}" in
    fetch | prepare | needs | driver) lock_tree ;;
esac
case "${1:-}" in
    fetch) fetch && echo "linux-tree: $src" ;;
    prepare) prepare ;;
    paths) printf 'src %s\nobj %s\n' "$src" "$obj" ;;
    needs) needs "${2:-}" ;;
    driver) driver "${2:-}" ;;
    *) die "usage: linux-tree.sh fetch | prepare | paths | needs <dir> | driver <name>" ;;
esac
