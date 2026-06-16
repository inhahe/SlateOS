#!/usr/bin/env bash
#
# Build a real ext4 root-filesystem image for the SlateOS Linux-ABI layer.
#
# This is the rootfs that lets the kernel run *prebuilt, dynamically-linked*
# glibc Linux binaries (Path Z / roadmap.md line 5089).  It stages a real glibc
# tree (`ld-linux-x86-64.so.2` + `libc.so.6`) plus a tiny dynamic test binary,
# then packs them into an ext4 image whose feature set is restricted to exactly
# what the kernel's native ext4 driver understands
# (`kernel/src/fs/ext4/ondisk.rs::SUPPORTED_INCOMPAT/SUPPORTED_RO_COMPAT`).
#
# Per design-decisions.md §25 the libc is **glibc** and the rootfs is **ext4**
# (no musl stepping-stone).  The FAT test image (disk.img, scripts/create-disk.py)
# is unaffected — it stays for the FAT driver self-test; this is a *second* disk.
#
# REQUIREMENTS: run inside a Linux environment with glibc + e2fsprogs + gcc.
# On the Windows dev box that means WSL:
#
#     wsl -d Ubuntu -- bash "scripts/create-ext4-rootfs.sh"
#
# Output: rootfs.ext4 at the repo root (git-ignored via *.ext4).
#
# The image is intentionally MINIMAL and built with a conservative feature set:
#   - no journal       (^has_journal)   — the rootfs is mounted read-only, so no
#                                          recovery is needed; avoids INCOMPAT_RECOVER
#   - no metadata_csum (^metadata_csum)  — first-light bring-up avoids any csum
#                                          mismatch rejecting the mount; the driver
#                                          supports csums but we don't need them
#   - no resize_inode / orphan_file      — unused for a static rootfs; orphan_file
#                                          is newer than the driver's known set
# Everything left on is in the driver's supported set: extent, 64bit, flex_bg,
# filetype, sparse_super, large_file, huge_file, dir_nlink, extra_isize, ext_attr.

set -euo pipefail

# --- locate the repo root (this script lives in <root>/scripts) --------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
OUT_IMG="${1:-$ROOT_DIR/rootfs.ext4}"
IMG_SIZE="${IMG_SIZE:-48M}"

# --- standard Ubuntu/Debian glibc locations ----------------------------------
LD_SO="/lib64/ld-linux-x86-64.so.2"          # PT_INTERP of every x86-64 glibc exe
LIBC="/lib/x86_64-linux-gnu/libc.so.6"        # the C library itself
LIBC_DIR="/lib/x86_64-linux-gnu"

echo "[rootfs] repo root : $ROOT_DIR"
echo "[rootfs] output    : $OUT_IMG ($IMG_SIZE)"

# --- sanity: required tools + glibc artifacts present ------------------------
for tool in mke2fs gcc cp; do
    command -v "$tool" >/dev/null 2>&1 || { echo "[rootfs] ERROR: '$tool' not found (run inside WSL/Linux)"; exit 1; }
done
for f in "$LD_SO" "$LIBC"; do
    [ -e "$f" ] || { echo "[rootfs] ERROR: missing glibc artifact: $f"; exit 1; }
done

# --- build the staging tree --------------------------------------------------
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

mkdir -p "$STAGE/lib64" "$STAGE$LIBC_DIR" "$STAGE/bin"

# Dereference the ld.so symlink so the rootfs holds the real ELF, mounted at the
# exact interpreter path the test binary names in its PT_INTERP.
cp -L "$LD_SO" "$STAGE/lib64/ld-linux-x86-64.so.2"
cp -L "$LIBC"  "$STAGE$LIBC_DIR/libc.so.6"

# --- the test binary: full glibc dynamic startup, exit(42) -------------------
# A trivial `main` that returns 42 exercises the ENTIRE real-glibc dynamic path:
# ld.so maps libc.so.6, processes relocations, sets up TLS, runs __libc_start_main,
# calls main, and exits 42.  If the SlateOS child process exits 42, real dynamic
# glibc execution works end-to-end.  RUNPATH guarantees libc.so.6 is found without
# an ld.so.cache (none is staged).
CSRC="$STAGE/hello.c"
cat > "$CSRC" <<'EOF'
/* SlateOS Path-Z real-glibc dynamic smoke test. */
int main(void) {
    return 42;
}
EOF
gcc -O2 -o "$STAGE/bin/hello" "$CSRC" -Wl,-rpath,"$LIBC_DIR" -Wl,--enable-new-dtags
rm -f "$CSRC"

# --- the stdio test binary: full glibc stdio output path ---------------------
# `printf` to stdout exercises the part of glibc that `hello` does NOT: stdio
# stream setup, the fstat(1) call glibc uses to choose buffering, the
# vfprintf/%d formatting machinery, and the exit-time flush that finally issues
# the write(2)/writev(2) to fd 1.  The SlateOS self-test wires fd 1 to a file,
# runs this binary, then reads the file back and asserts the exact bytes — so
# this proves the real-glibc *output* path, the gate for any program that
# produces output.  It returns 7 so the exit-code channel independently
# confirms a clean run.
CSRC2="$STAGE/stdio.c"
cat > "$CSRC2" <<'EOF'
/* SlateOS Path-Z real-glibc stdio (output) test. */
#include <stdio.h>
int main(void) {
    printf("SLATE_GLIBC_STDIO_OK %d\n", 1234);
    return 7;
}
EOF
gcc -O2 -o "$STAGE/bin/stdio" "$CSRC2" -Wl,-rpath,"$LIBC_DIR" -Wl,--enable-new-dtags
rm -f "$CSRC2"

# --- the "full" test binary: argv + getenv + stdin + heavy malloc/free --------
# This binary exercises every glibc input/runtime path the first two do not:
#   - argv delivery   : sums the lengths of all argv[] strings (proves the
#                       kernel built the stack's argv vector glibc reads).
#   - environment     : getenv("SLATE_TAG") proves envp delivery + glibc's
#                       environ scan.
#   - stdin           : one fgets() from stdin proves the glibc *input* path
#                       (fstat(0) buffering choice + read(2) on a regular file).
#   - heap stress     : 64 rounds mixing small (brk arena) and large (>128 KiB,
#                       mmap-backed) allocations, touching every page, then
#                       freeing — stresses brk growth and the mmap heap path
#                       under genuine glibc allocator behaviour.  A crash, OOM,
#                       or corruption returns a non-11 exit, failing the test.
# Output is fully deterministic from the fixed argv/env/stdin the SlateOS
# self-test supplies, so the test asserts the exact bytes.  Returns 11.
CSRC3="$STAGE/full.c"
cat > "$CSRC3" <<'EOF'
/* SlateOS Path-Z real-glibc argv/env/stdin/heap test. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
    long argsum = 0;
    for (int i = 0; i < argc; i++) argsum += (long)strlen(argv[i]);

    const char *tag = getenv("SLATE_TAG");
    if (!tag) tag = "none";

    char line[128];
    if (!fgets(line, sizeof line, stdin)) line[0] = '\0';
    size_t L = strlen(line);
    if (L && line[L - 1] == '\n') line[--L] = '\0';

    unsigned long acc = 0;
    for (int round = 0; round < 64; round++) {
        size_t n = (round % 8 == 0) ? (256u * 1024u)
                                    : (size_t)(1024 + round * 48);
        unsigned char *p = malloc(n);
        if (!p) return 2;
        for (size_t j = 0; j < n; j += 256) {
            p[j] = (unsigned char)(j + round);
            acc += p[j];
        }
        free(p);
    }
    if (acc == 0) return 3;

    printf("SLATE_GLIBC_FULL_OK tag=%s argc=%d argsum=%ld in=%s\n",
           tag, argc, argsum, line);
    return 11;
}
EOF
gcc -O2 -o "$STAGE/bin/full" "$CSRC3" -Wl,-rpath,"$LIBC_DIR" -Wl,--enable-new-dtags
rm -f "$CSRC3"

# --- the "pthread" test binary: clone + futex + TLS via real glibc ------------
# This is the integration coverage thread_clone.rs's self-test explicitly cannot
# provide ("the integration path is covered by booting a real Linux binary that
# calls pthread_create").  It spawns 4 worker threads, each of which increments
# a shared counter NITERS times under a single pthread_mutex (so the result is
# deterministic regardless of scheduling), then joins all four and sums their
# return values.  This exercises:
#   - clone(CLONE_VM|CLONE_THREAD|CLONE_SETTLS|...) thread creation;
#   - per-thread TLS setup (errno + the mutex live in/through TLS);
#   - the futex fast path (uncontended adaptive-mutex CAS in userspace) AND the
#     contended path (futex_wait/futex_wake syscalls under lock contention);
#   - pthread_join, which blocks on the child-tid futex the kernel wakes when a
#     thread exits.
# counter == 4*NITERS and joinsum == 1+2+3+4 are deterministic, so the SlateOS
# self-test redirects fd 1 to a file and asserts the exact output.  Returns 13.
# glibc >= 2.34 folds pthread into libc.so.6, so no extra library is staged.
CSRC4="$STAGE/pthread.c"
cat > "$CSRC4" <<'EOF'
/* SlateOS Path-Z real-glibc pthread (clone + futex + TLS) test. */
#include <stdio.h>
#include <pthread.h>

#define NTHREADS 4
#define NITERS   10000

static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static long counter = 0;

static void *worker(void *arg) {
    long id = (long)arg;
    for (int i = 0; i < NITERS; i++) {
        pthread_mutex_lock(&lock);
        counter += 1;
        pthread_mutex_unlock(&lock);
    }
    return (void *)(id + 1);
}

int main(void) {
    pthread_t t[NTHREADS];
    for (long i = 0; i < NTHREADS; i++) {
        if (pthread_create(&t[i], NULL, worker, (void *)i) != 0) return 2;
    }
    long joinsum = 0;
    for (int i = 0; i < NTHREADS; i++) {
        void *ret = NULL;
        if (pthread_join(t[i], &ret) != 0) return 3;
        joinsum += (long)ret;
    }
    printf("SLATE_GLIBC_PTHREAD_OK counter=%ld joinsum=%ld\n", counter, joinsum);
    return 13;
}
EOF
gcc -O2 -pthread -o "$STAGE/bin/pthread" "$CSRC4" -Wl,-rpath,"$LIBC_DIR" -Wl,--enable-new-dtags
rm -f "$CSRC4"
echo "[rootfs] pthread binary DT_NEEDED:"
readelf -d "$STAGE/bin/pthread" 2>/dev/null | grep -E 'NEEDED|RUNPATH' | sed 's/^/  /'

# --- the "signal" test binary: real glibc SA_SIGINFO handler round-trip --------
# This is the integration coverage the kernel's own signal-shim self-tests
# cannot provide: they exercise the pending/blocked/disposition bookkeeping in
# isolation but never build a real Linux `rt_sigframe` and enter an unmodified
# glibc handler.  This binary installs a `SA_SIGINFO` handler for SIGUSR1 via
# `sigaction(2)` (glibc fills in `sa_restorer` = `__restore_rt` automatically),
# `raise(3)`s SIGUSR1 (glibc routes that through `tgkill(2)`), and the handler
# reads `info->si_signo`/`si_code` and sets a flag.  This exercises, end to end:
#   - `rt_sigaction` install (handler + SA_SIGINFO + sa_restorer + sa_mask);
#   - signal posting via raise/tgkill;
#   - the kernel's Linux-shape `rt_sigframe` delivery: handler entered with
#     rdi=signo, rsi=&siginfo, rdx=&ucontext, rsp at pretcode=sa_restorer;
#   - the handler correctly reading a byte-exact `siginfo_t`;
#   - the return path: handler `ret`s into glibc's `__restore_rt`, which calls
#     `rt_sigreturn`, restoring the pre-signal context so `main` resumes.
# Output is deterministic: SIGUSR1 = 10 on x86_64, and the kernel currently
# stamps si_code = SI_USER (0) for caught signals (sender-faithful si_code is
# future work), so the line is fixed.  Returns 17 (2 = sigaction failed,
# 3 = handler never ran, 4 = wrong signo).
CSRC5="$STAGE/signal.c"
cat > "$CSRC5" <<'EOF'
/* SlateOS Path-Z real-glibc signal (SA_SIGINFO handler + rt_sigreturn) test. */
#include <stdio.h>
#include <signal.h>
#include <string.h>

static volatile sig_atomic_t got = 0;
static volatile int got_signo = -1;
static volatile int got_code = -1;

static void handler(int signo, siginfo_t *info, void *ucv) {
    got_signo = signo;
    got_code = info ? info->si_code : -99;
    got = 1;
    (void)ucv;
}

int main(void) {
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = handler;
    sa.sa_flags = SA_SIGINFO;
    sigemptyset(&sa.sa_mask);
    if (sigaction(SIGUSR1, &sa, NULL) != 0) return 2;

    raise(SIGUSR1);            /* glibc: tgkill(getpid(), gettid(), SIGUSR1) */

    if (!got) return 3;        /* handler never ran -> delivery broken */
    if (got_signo != SIGUSR1) return 4;

    printf("SLATE_GLIBC_SIGNAL_OK signo=%d code=%d\n", got_signo, got_code);
    return 17;
}
EOF
gcc -O2 -o "$STAGE/bin/signal" "$CSRC5" -Wl,-rpath,"$LIBC_DIR" -Wl,--enable-new-dtags
rm -f "$CSRC5"

echo "[rootfs] staged tree:"
( cd "$STAGE" && find lib64 lib bin -type f -printf '  %-44p %10s bytes\n' )

# --- pack into a driver-compatible ext4 image --------------------------------
# -b 4096 : the driver reads/writes at 4 KiB ext4-block granularity.
# -F      : overwrite a non-block-device file without prompting.
# -d      : populate from the staging directory (no root / no loop mount needed).
rm -f "$OUT_IMG"
mke2fs -q -F -t ext4 -b 4096 \
    -O '^has_journal,^metadata_csum,^resize_inode,^orphan_file' \
    -L SLATEOS_ROOT \
    -d "$STAGE" \
    "$OUT_IMG" "$IMG_SIZE"

echo "[rootfs] created $OUT_IMG"
echo "[rootfs] feature set:"
dumpe2fs -h "$OUT_IMG" 2>/dev/null | grep -E 'Filesystem features|Block size|Inode count|Free blocks' | sed 's/^/  /'
echo "[rootfs] contents (debugfs):"
debugfs -R 'ls -l /' "$OUT_IMG" 2>/dev/null | sed 's/^/  /'
debugfs -R 'ls -l /bin' "$OUT_IMG" 2>/dev/null | sed 's/^/  /'
echo "[rootfs] DONE."
