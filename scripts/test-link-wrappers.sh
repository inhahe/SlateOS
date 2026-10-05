#!/bin/bash
# Self-test for slate_make_link_wrappers in scripts/lib/worktree.sh: the
# compilers every port's link -- and CPython's and LLVM's whole builds -- go
# through to be built for SlateOS's libc rather than zig's musl.
#
#   wsl -d Ubuntu -- bash scripts/test-link-wrappers.sh
#
# Run it from WSL, with a current sysroot (toolchain/build-sysroot.ps1): it
# needs zig and libc.a, as the ports do. Each check is a rule the wrapper got
# wrong once, or that a port depends on:
#
#   * a call that compiles and links in one step -- autoconf's link test,
#     `cc -o conftest conftest.c` -- links against OUR libc.a: a function ours
#     has links, one it lacks is reported undefined. Until 2026-10-05 the
#     whole call went to zig's driver, whose musl answered instead, and
#     CPython's configure found none of close_range, sem_clockwait, getwd,
#     tmpnam_r (known-issues-resolved/
#     D-SPIKES-CPYTHON-WAS-CONFIGURED-FOR-MUSL-NOT-FOR-OUR-LIBC.md);
#   * a compile sees posix/include, so what ours has beyond musl is declared;
#   * the scratch objects of a split call are cleaned up, whether it links or
#     not;
#   * a link-only call keeps its inputs, and -o, and still refuses -shared;
#   * the output carries the SlateOS note (the kernel's loader checks it).
set -u

. "$(dirname "${BASH_SOURCE[0]}")/lib/worktree.sh" || exit 1

PASS=0
FAIL=0
ok() { PASS=$((PASS + 1)); }
bad() {
    FAIL=$((FAIL + 1))
    echo "FAIL: $*"
}

T="$(mktemp -d /tmp/test-link-wrappers.XXXXXX)" || exit 1
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/libs" "$T/tmp"
cp "$SLATE_SYSROOT/libc.a" "$SLATE_SYSROOT/libunwind.a" "$T/libs/" || exit 1
slate_make_link_wrappers "$T/bin" "$T/libs" || exit 1
cd "$T" || exit 1
# The split's scratch directories land here, where they can be counted.
export TMPDIR="$T/tmp"

# 1. One-step compile-and-link, autoconf's shape, answers for our libc.a.
for fn in close_range sem_clockwait getwd tmpnam_r; do
    printf 'char %s();\nint main(void){ return %s(); }\n' "$fn" "$fn" > conftest.c
    rm -f conftest
    if "$SLATE_LINK_CC" -o conftest -O2 conftest.c -ldl -lpthread >link.log 2>&1 && [ -x conftest ]; then
        ok
    else
        bad "$fn: ours has it, and a one-step link did not find it: $(head -2 link.log)"
    fi
done
printf 'char kqueue();\nint main(void){ return kqueue(); }\n' > conftest.c
rm -f conftest
if "$SLATE_LINK_CC" -o conftest conftest.c >link.log 2>&1; then
    bad "kqueue: ours has not got it, and a one-step link found it (musl's?)"
elif grep -q "undefined symbol: kqueue" link.log; then
    ok
else
    bad "kqueue: refused, but not as undefined: $(head -2 link.log)"
fi

# 2. Compiles see posix/include: no implicit declaration, -c or one-step.
cat > decl.c <<'C'
#define _GNU_SOURCE
#include <unistd.h>
#include <semaphore.h>
int main(void)
{
    sem_t s;
    struct timespec t = {0};
    (void)sem_clockwait(&s, 1, &t);
    return close_range(3, ~0U, 0);
}
C
if "$SLATE_LINK_CC" -Werror=implicit-function-declaration -c decl.c -o decl.o >decl.log 2>&1; then
    ok
else
    bad "-c: close_range or sem_clockwait undeclared: $(head -2 decl.log)"
fi
if "$SLATE_LINK_CC" -Werror=implicit-function-declaration -o decl decl.c >decl.log 2>&1; then
    ok
else
    bad "one-step: close_range or sem_clockwait undeclared: $(head -2 decl.log)"
fi

# 3. No scratch left by the split, after links that worked and one that did not.
left="$(find "$T/tmp" -mindepth 1 -maxdepth 1 | wc -l)"
[ "$left" = 0 ] && ok || bad "the split left $left scratch director(y/ies) in TMPDIR"

# 4. A link-only call: the object linked, -o kept, the SlateOS note there.
printf 'int main(void){ return 0; }\n' > m.c
"$SLATE_LINK_CC" -c m.c -o m.o >/dev/null 2>&1 || bad "compiling m.c"
rm -f m
if "$SLATE_LINK_CC" -o m m.o -lm -lpthread >m.log 2>&1 && [ -x m ]; then
    ok
else
    bad "link-only: $(head -2 m.log)"
fi
if readelf -n m 2>/dev/null | grep -q SlateOS; then
    ok
else
    bad "the linked program has no SlateOS note"
fi
if "$SLATE_LINK_CC" -shared -o m.so m.o >/dev/null 2>&1; then
    bad "-shared was accepted"
else
    ok
fi

echo "test-link-wrappers: $PASS passed, $FAIL failed"
[ "$FAIL" = 0 ]
