# shellcheck shell=bash
# What a CMake build's configuration depends on, as one line. Sourced by run.sh and slatelink.sh.
#
# run.sh records it beside the build it configures ($SLATE_WORK/cmake-spike/
# configured.txt); slatelink.sh compares it with the tree's before it relinks,
# and hands over to run.sh when they differ. CMake's configure answers its
# check_function_exists and check_symbol_exists probes by linking against our
# libc.a, and its header probes from posix/include and zig's musl headers, so
# the objects encode both: a function libc.a gained, or a changed header,
# leaves objects that link cleanly against the new libc.a and were configured
# without it. Only a fresh configure catches up, and only this says when one
# is needed. The recipe, the CMake version and the zig (whose musl and libc++
# headers the build read) are in it too.
#
# Sourced after scripts/lib/worktree.sh, whose SLATE_* names it reads.

# Print the fingerprint of a configuration made against the archive `$1` (a
# libc.a) and the header tree `$2` (posix/include). Of the archive only the
# names a C program can refer to count: defined global symbols that are C
# identifiers, without Rust's mangled names (`_R...`, `_ZN...`), whose hashes
# change with every build of the same code. zig counts by its version, not
# its path, so the caller must have run slate_ensure_zig, which settles
# which zig $SLATE_ZIG is. Fails if any of them cannot be read.
cmake_configure_fingerprint() {
    local libc="$1" include="$2" symbols headers recipe zig nm
    [ -f "$libc" ] && [ -d "$include" ] || return 1
    zig="$("$SLATE_ZIG" version 2>/dev/null)" && [ -n "$zig" ] || return 1
    # binutils' nm, which WSL's Ubuntu has and the Windows side does not.
    nm="$(command -v nm)" || { echo "configured.sh: no nm to list libc.a's names" >&2; return 1; }
    symbols="$("$nm" -g --defined-only --format=just-symbols "$libc" 2>/dev/null \
        | grep -E '^[A-Za-z_][A-Za-z0-9_]*$' | grep -vE '^(_R|_ZN)' \
        | sort -u | sha256sum | cut -c1-64)" || return 1
    headers="$(cd "$include" && find . -type f -print0 | sort -z \
        | xargs -0 sha256sum | sha256sum | cut -c1-64)" || return 1
    recipe="$(sha256sum <"$SLATE_ROOT/scripts/cmake-spike/run.sh" | cut -c1-64)" || return 1
    echo "cmake=$SLATE_CMAKE_VERSION zig=$zig libc=$symbols posix/include=$headers run.sh=$recipe"
}
