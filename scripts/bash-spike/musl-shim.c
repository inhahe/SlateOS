/*
 * For the musl copy of bash that scripts/bash-spike/cross2.sh links to run on
 * Linux -- and ONLY for it: the SlateOS link (cross2.sh's own, and
 * slatelink.sh's) never sees this file.
 *
 * bash is configured against SlateOS's libc, which has arc4random (glibc
 * 2.36's interface), so its objects call it for $SRANDOM. zig's musl has not
 * got it. This supplies it from getrandom, which musl has, so that the same
 * objects run under WSL for a measurement of this build's bash.
 *
 * As CPython's scripts/cpython-spike/control-shim.c does for its control
 * interpreter.
 */
#include <errno.h>
#include <stdint.h>
#include <stdlib.h>
#include <sys/random.h>
#include <sys/types.h>

uint32_t arc4random(void)
{
    uint32_t value = 0;
    /* glibc's arc4random cannot fail; a getrandom that keeps failing for any
     * reason but an interrupted call has nothing to fall back on here. */
    for (;;) {
        ssize_t got = getrandom(&value, sizeof value, 0);
        if (got == (ssize_t)sizeof value)
            return value;
        if (got < 0 && errno != EINTR)
            abort();
    }
}
