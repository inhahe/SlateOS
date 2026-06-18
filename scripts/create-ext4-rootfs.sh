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
# Output is deterministic: SIGUSR1 = 10 on x86_64.  Because glibc routes
# raise(3) through tgkill(2), Linux (and now SlateOS) delivers a thread-directed
# siginfo: si_code = SI_TKILL (-6) and si_pid = the caller's pid.  The handler
# verifies both (sender-faithful siginfo, known-issues.md TD29) and prints
# `self=1` when si_pid == getpid().  Returns 17 (2 = sigaction failed,
# 3 = handler never ran, 4 = wrong signo, 5 = wrong si_code, 6 = wrong si_pid).
CSRC5="$STAGE/signal.c"
cat > "$CSRC5" <<'EOF'
/* SlateOS Path-Z real-glibc signal (SA_SIGINFO handler + rt_sigreturn) test. */
#include <stdio.h>
#include <signal.h>
#include <string.h>
#include <unistd.h>

/* SI_TKILL is glibc-internal in some header configurations; pin the ABI value. */
#ifndef SI_TKILL
#define SI_TKILL (-6)
#endif

static volatile sig_atomic_t got = 0;
static volatile int got_signo = -1;
static volatile int got_code = -1;
static volatile int got_pid = -1;

static void handler(int signo, siginfo_t *info, void *ucv) {
    got_signo = signo;
    got_code = info ? info->si_code : -99;
    got_pid = info ? (int)info->si_pid : -99;
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

    if (!got) return 3;            /* handler never ran -> delivery broken */
    if (got_signo != SIGUSR1) return 4;
    if (got_code != SI_TKILL) return 5;        /* sender-faithful si_code */
    if (got_pid != (int)getpid()) return 6;    /* sender-faithful si_pid  */

    printf("SLATE_GLIBC_SIGNAL_OK signo=%d code=%d self=%d\n",
           got_signo, got_code, got_pid == (int)getpid());
    return 17;
}
EOF
gcc -O2 -o "$STAGE/bin/signal" "$CSRC5" -Wl,-rpath,"$LIBC_DIR" -Wl,--enable-new-dtags
rm -f "$CSRC5"

# --- the "fault" test binary: synchronous CPU fault -> Linux SIGSEGV -----------
# The "signal" binary above exercises *asynchronous* signal delivery (raise ->
# tgkill -> rt_sigframe).  This one exercises the *synchronous* path: a real
# CPU page fault (#PF) on an unmapped address must be turned into a Linux
# SIGSEGV delivered to an unmodified glibc SA_SIGINFO handler, with a faithful
# `siginfo_t`:
#   - si_signo = SIGSEGV (11);
#   - si_code  = SEGV_MAPERR (1)  [address not mapped, present bit clear];
#   - si_addr  = the exact faulting address (= CR2 = 0xDEAD000).
# 0xDEAD000 is a low, guaranteed-unmapped address: the PIE base is ~0x5555...,
# ld.so/libc map ~0x7000..., and the stack is ~0x7fff..., so the kernel's
# demand-fault / stack-growth resolver will never satisfy it -> unrecoverable
# user fault -> SIGSEGV.  Because returning from the handler would re-execute
# the faulting store and fault again, the handler uses sigsetjmp/siglongjmp to
# recover to a safe point instead of relying on rt_sigreturn resuming past the
# instruction.  This validates, end to end:
#   - the page-fault ISR building a Linux rt_sigframe from the *interrupt*
#     register context (not a syscall frame);
#   - fault-specific si_code classification (present bit -> MAPERR vs ACCERR);
#   - si_addr carrying CR2;
#   - the handler reading a byte-exact siginfo_t and longjmp'ing out cleanly.
# Output is deterministic.  Returns 19 on success (2 = sigaction failed,
# 3 = handler never ran, 4 = wrong signo, 5 = wrong si_code, 6 = wrong si_addr).
CSRC6="$STAGE/fault.c"
cat > "$CSRC6" <<'EOF'
/* SlateOS Path-Z real-glibc synchronous-fault (#PF -> SIGSEGV) test. */
#include <stdio.h>
#include <signal.h>
#include <string.h>
#include <unistd.h>
#include <setjmp.h>

/* SEGV_MAPERR is glibc-internal in some header configurations; pin the ABI value. */
#ifndef SEGV_MAPERR
#define SEGV_MAPERR 1
#endif

#define FAULT_ADDR 0xDEAD000UL

static volatile sig_atomic_t got = 0;
static volatile int got_signo = -1;
static volatile int got_code = -1;
static volatile unsigned long got_addr = 0;
static sigjmp_buf recover;

static void handler(int signo, siginfo_t *info, void *ucv) {
    got_signo = signo;
    got_code = info ? info->si_code : -99;
    got_addr = info ? (unsigned long)info->si_addr : 0;
    got = 1;
    (void)ucv;
    siglongjmp(recover, 1);    /* can't resume past the faulting store */
}

int main(void) {
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = handler;
    sa.sa_flags = SA_SIGINFO;
    sigemptyset(&sa.sa_mask);
    if (sigaction(SIGSEGV, &sa, NULL) != 0) return 2;

    if (sigsetjmp(recover, 1) == 0) {
        volatile unsigned char *p = (volatile unsigned char *)FAULT_ADDR;
        *p = 0x42;             /* triggers #PF on an unmapped page */
    }

    if (!got) return 3;            /* handler never ran -> delivery broken */
    if (got_signo != SIGSEGV) return 4;
    if (got_code != SEGV_MAPERR) return 5;     /* fault-specific si_code   */
    if (got_addr != FAULT_ADDR) return 6;      /* faithful si_addr (= CR2) */

    printf("SLATE_GLIBC_FAULT_OK signo=%d code=%d addr=0x%lx\n",
           got_signo, got_code, got_addr);
    return 19;
}
EOF
gcc -O2 -o "$STAGE/bin/fault" "$CSRC6" -Wl,-rpath,"$LIBC_DIR" -Wl,--enable-new-dtags
rm -f "$CSRC6"

# --- the "sigqueue" test binary: queued signal with an si_value payload -------
# The "signal" binary exercises a plain raise()->tgkill (SI_TKILL, no payload).
# This one exercises the *queued* path: sigqueue(3) attaches a data word that
# the kernel must carry byte-exact into the delivered siginfo_t and hand to an
# unmodified glibc SA_SIGINFO handler as info->si_value. It validates the full
# rt_sigqueueinfo round-trip:
#   - si_code  = SI_QUEUE (-1)            [queued, not kill/tkill];
#   - si_pid   = getpid()                 [sender-faithful identity];
#   - si_value.sival_int = 0x12345678     [the attached payload].
# glibc routes sigqueue(getpid(), sig, val) through rt_sigqueueinfo(2). The
# handler reads info->si_value.sival_int and resumes via rt_sigreturn (no
# longjmp needed -- a queued signal does not re-fault). Output is
# deterministic. Returns 23 on success (2 = sigaction failed, 3 = handler
# never ran, 4 = wrong signo, 5 = wrong si_code, 6 = wrong si_value,
# 7 = wrong si_pid).
CSRC7="$STAGE/sigqueue.c"
cat > "$CSRC7" <<'EOF'
/* SlateOS Path-Z real-glibc queued-signal (sigqueue + si_value) test. */
#include <stdio.h>
#include <signal.h>
#include <string.h>
#include <unistd.h>

/* SI_QUEUE is glibc-internal in some header configurations; pin the ABI value. */
#ifndef SI_QUEUE
#define SI_QUEUE (-1)
#endif

#define PAYLOAD 0x12345678

static volatile sig_atomic_t got = 0;
static volatile int got_signo = -1;
static volatile int got_code = -1;
static volatile int got_value = -1;
static volatile int got_pid = -1;

static void handler(int signo, siginfo_t *info, void *ucv) {
    got_signo = signo;
    got_code = info ? info->si_code : -99;
    got_value = info ? info->si_value.sival_int : -99;
    got_pid = info ? (int)info->si_pid : -99;
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

    union sigval sv;
    sv.sival_int = PAYLOAD;
    if (sigqueue(getpid(), SIGUSR1, sv) != 0) return 2;  /* -> rt_sigqueueinfo */

    if (!got) return 3;            /* handler never ran -> delivery broken */
    if (got_signo != SIGUSR1) return 4;
    if (got_code != SI_QUEUE) return 5;        /* queued si_code           */
    if (got_value != PAYLOAD) return 6;        /* faithful si_value payload */
    if (got_pid != (int)getpid()) return 7;    /* sender-faithful si_pid    */

    printf("SLATE_GLIBC_SIGQUEUE_OK signo=%d code=%d value=0x%x self=%d\n",
           got_signo, got_code, got_value, got_pid == (int)getpid());
    return 23;
}
EOF
gcc -O2 -o "$STAGE/bin/sigqueue" "$CSRC7" -Wl,-rpath,"$LIBC_DIR" -Wl,--enable-new-dtags
rm -f "$CSRC7"

# --- the "forkexec" test binary: fork()+execl()+waitpid() of a glibc child ----
# Every other Path-Z binary is a single glibc process.  This one proves a real
# glibc program can spawn *another* real glibc program and reap it -- the
# foundation for a shell.  It exercises glibc's fork() (clone(SIGCHLD) with a
# genuine CoW address-space copy + pthread_atfork/malloc-lock handling),
# execl() (PATH-less absolute exec marshalling argv/envp), and waitpid()
# (wrapping wait4) end-to-end.  The child execs the silent /bin/hello (exits 42
# with no output), so the only bytes written to the shared fd 1 come from the
# parent *after* the reap -- output stays deterministic.  Returns 27 on success
# (2 = fork failed, 3 = waitpid mismatch, 4 = child didn't exit normally).
CSRC8="$STAGE/forkexec.c"
cat > "$CSRC8" <<'EOF'
/* SlateOS Path-Z real-glibc fork()+execl()+waitpid() test. */
#include <stdio.h>
#include <unistd.h>
#include <sys/wait.h>

int main(void) {
    pid_t pid = fork();
    if (pid < 0) return 2;               /* fork failed */
    if (pid == 0) {
        /* child: replace image with the silent real-glibc /bin/hello (exit 42) */
        execl("/bin/hello", "/bin/hello", (char *)0);
        _exit(127);                      /* exec failed */
    }
    int status = 0;
    if (waitpid(pid, &status, 0) != pid) return 3;   /* -> wait4 */
    if (!WIFEXITED(status)) return 4;                /* abnormal child exit */

    /* Only the parent writes to fd 1, and only here, after the reap. */
    printf("SLATE_GLIBC_FORKEXEC_OK childexit=%d\n", WEXITSTATUS(status));
    return 27;
}
EOF
gcc -O2 -o "$STAGE/bin/forkexec" "$CSRC8" -Wl,-rpath,"$LIBC_DIR" -Wl,--enable-new-dtags
rm -f "$CSRC8"

# --- the "emit" helper: a glibc program that writes a fixed payload to fd 1 ----
# Used as the downstream end of the pipe test below.  It is exec'd by the pipe
# test's child with fd 1 already rewired to a pipe write end, so its 16-byte
# write(2) travels through the pipe to the reading parent -- proving that an
# open (dup2'd) fd survives execve into a fresh glibc image (no CLOEXEC).
CSRC9="$STAGE/emit.c"
cat > "$CSRC9" <<'EOF'
/* SlateOS Path-Z pipe-downstream helper: write a fixed payload to fd 1. */
#include <unistd.h>

int main(void) {
    /* 16 bytes incl. the trailing newline. */
    (void)write(1, "SLATE_PIPE_BODY\n", 16);
    return 0;
}
EOF
gcc -O2 -o "$STAGE/bin/emit" "$CSRC9" -Wl,-rpath,"$LIBC_DIR" -Wl,--enable-new-dtags
rm -f "$CSRC9"

# --- the "pipe" test binary: the `cmd1 | cmd2` shell primitive ----------------
# A real glibc program that builds the exact plumbing a shell uses for a
# pipeline: pipe(2) -> fork(2) -> the child dup2(2)s the write end onto fd 1,
# closes both raw ends, and execl(2)s /bin/emit; the parent closes the write
# end, read(2)s the pipe to EOF, and waitpid(2)s the child.  This exercises (a)
# pipe-fd inheritance across the CoW fork, (b) dup2 redirection, (c) open fds
# surviving execve into a new glibc image, and (d) pipe EOF arriving once every
# write end (parent's + the exec'd child's) is closed.  The parent then prints
# what it read to its own fd 1 (the capture file) and returns 29.
# (2 = pipe failed, 3 = fork failed, 4 = waitpid mismatch, 5 = child error.)
CSRC10="$STAGE/pipe.c"
cat > "$CSRC10" <<'EOF'
/* SlateOS Path-Z real-glibc pipe()+fork()+dup2()+execl()+read()+wait test. */
#include <stdio.h>
#include <unistd.h>
#include <sys/wait.h>

int main(void) {
    int fds[2];
    if (pipe(fds) != 0) return 2;            /* pipe failed */
    pid_t pid = fork();
    if (pid < 0) return 3;                    /* fork failed */
    if (pid == 0) {
        /* child: rewire stdout onto the pipe write end, then exec the writer */
        if (dup2(fds[1], 1) < 0) _exit(126);
        close(fds[0]);
        close(fds[1]);
        execl("/bin/emit", "/bin/emit", (char *)0);
        _exit(127);                           /* exec failed */
    }
    close(fds[1]);                            /* parent: drop the write end */

    char buf[64];
    int n = 0, r;
    while (n < (int)sizeof(buf) &&
           (r = (int)read(fds[0], buf + n, sizeof(buf) - n)) > 0) {
        n += r;
    }
    close(fds[0]);

    int status = 0;
    if (waitpid(pid, &status, 0) != pid) return 4;       /* -> wait4 */
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) return 5;

    /* Parent's own fd 1 is the capture file; emit deterministic output. */
    printf("SLATE_GLIBC_PIPE_OK n=%d body=%.*s", n, n, buf);
    return 29;
}
EOF
gcc -O2 -o "$STAGE/bin/pipe" "$CSRC10" -Wl,-rpath,"$LIBC_DIR" -Wl,--enable-new-dtags
rm -f "$CSRC10"

# --- the "redir" test binary: the `cmd > file` shell primitive ----------------
# A real glibc program that performs its OWN output redirection the way a shell
# does for `cmd > file`: open(2) a target with O_WRONLY|O_CREAT|O_TRUNC, dup2(2)
# the resulting fd onto fd 1 (the kernel closes the displaced console fd 1),
# close the now-redundant original fd, then printf to the redirected stdout.
# Part 7 (/bin/pipe) proved dup2 onto a *pipe*; this proves dup2 of a
# self-open()ed *File* handle onto stdout plus glibc's exit-time flush landing
# in a real file the program chose.  The SlateOS self-test does NOT inject any
# fd here — it reads the file the program created back from the VFS.  Returns 31
# so the exit-code channel independently confirms a clean run.
# (2 = open failed, 3 = dup2 failed.)
CSRC11="$STAGE/redir.c"
cat > "$CSRC11" <<'EOF'
/* SlateOS Path-Z real-glibc `cmd > file` output-redirection test. */
#include <stdio.h>
#include <unistd.h>
#include <fcntl.h>

int main(void) {
    /* Open the redirect target exactly as a shell does for `> file`. */
    int fd = open("/redir-out.txt", O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) return 2;            /* open failed */
    /* Point stdout at it; the kernel closes the displaced fd 1 (console). */
    if (dup2(fd, 1) < 0) return 3;   /* dup2 failed */
    close(fd);                        /* original fd now redundant */
    /* fd 1 is a regular file now, so glibc full-buffers and flushes the
       write(2) at exit. */
    printf("SLATE_GLIBC_REDIR_OK marker=%d\n", 4242);
    return 31;
}
EOF
gcc -O2 -o "$STAGE/bin/redir" "$CSRC11" -Wl,-rpath,"$LIBC_DIR" -Wl,--enable-new-dtags
rm -f "$CSRC11"

# --- the "redirin" test binary: the `cmd < file` shell primitive --------------
# The mirror image of /bin/redir: a real glibc program that performs its OWN
# *input* redirection the way a shell does for `cmd < file`: open(2) a source
# with O_RDONLY, dup2(2) the resulting fd onto fd 0 (the kernel closes the
# displaced console fd 0), close the now-redundant original fd, then read a line
# from the redirected stdin via glibc's buffered fgets (fstat(0) + read(2)).
# Part 8 (/bin/redir) proved dup2 of a self-open()ed File onto stdout; this
# proves dup2 onto stdin and the glibc *input* path reading from a real file.
# The SlateOS self-test pre-creates the input file the program reads, injects NO
# fd, and confirms success purely via the exit code: the program compares the
# line it read against a compiled-in literal and returns 37 only on an exact
# match, so a correct exit code byte-exactly proves the right bytes flowed in.
# (2 = open failed, 3 = dup2 failed, 4 = fgets failed/EOF, 5 = content mismatch.)
CSRC12="$STAGE/redirin.c"
cat > "$CSRC12" <<'EOF'
/* SlateOS Path-Z real-glibc `cmd < file` input-redirection test. */
#include <stdio.h>
#include <unistd.h>
#include <fcntl.h>
#include <string.h>

int main(void) {
    /* Open the redirect source exactly as a shell does for `< file`. */
    int fd = open("/redir-in.txt", O_RDONLY);
    if (fd < 0) return 2;            /* open failed */
    /* Point stdin at it; the kernel closes the displaced fd 0 (console). */
    if (dup2(fd, 0) < 0) return 3;   /* dup2 failed */
    close(fd);                        /* original fd now redundant */
    /* fd 0 is a regular file now, so glibc fstat(0)s it, fills its buffer
       with a read(2), and serves fgets from that buffer. */
    char buf[64];
    if (!fgets(buf, sizeof buf, stdin)) return 4;  /* read failed / empty */
    if (strcmp(buf, "SLATE_GLIBC_STDIN_OK marker=7777\n") != 0) return 5;
    return 37;                        /* exact-match success */
}
EOF
gcc -O2 -o "$STAGE/bin/redirin" "$CSRC12" -Wl,-rpath,"$LIBC_DIR" -Wl,--enable-new-dtags
rm -f "$CSRC12"

# --- a REAL POSIX shell: dash -------------------------------------------------
# Path Z's individual shell primitives (fork/exec/waitpid, pipe, dup2 onto a
# pipe, dup2 of a file onto stdout/stdin) are each proven by a bespoke test
# binary above.  The culmination is running an *unmodified, prebuilt* POSIX
# shell that orchestrates those primitives itself.  dash is the cleanest
# choice: it links only against libc.so.6 + ld-linux (both already staged) and
# the kernel-provided linux-vdso (no file), so no extra libraries are needed.
# Copied as both /bin/dash and /bin/sh (a copy, not a symlink, so the rootfs
# need not depend on symlink support in the image builder).  The SlateOS
# self-tests drive it with `dash -c '<command>'`.
# --- the "countbytes" pipeline-downstream filter ------------------------------
# Reads stdin to EOF and prints "n=<bytes>\n".  Used as the *downstream* stage
# of a real shell pipeline `cmd1 | countbytes`: the shell wires /bin/emit's
# stdout to this program's stdin through a pipe, so a correct byte count proves
# the pipe carried every byte across the fork/exec boundary.  Deterministic
# output ("n=16\n" for /bin/emit's 16-byte payload) lets the self-test assert
# the exact bytes.  (2 = read error.)
CSRC13="$STAGE/countbytes.c"
cat > "$CSRC13" <<'EOF'
/* SlateOS Path-Z pipeline downstream: count stdin bytes to EOF. */
#include <unistd.h>
#include <stdio.h>

int main(void) {
    char buf[4096];
    long total = 0;
    ssize_t n;
    while ((n = read(0, buf, sizeof buf)) > 0) total += n;
    if (n < 0) return 2;            /* read error */
    printf("n=%ld\n", total);
    return 0;
}
EOF
gcc -O2 -o "$STAGE/bin/countbytes" "$CSRC13" -Wl,-rpath,"$LIBC_DIR" -Wl,--enable-new-dtags
rm -f "$CSRC13"

DASH_SRC="/bin/dash"
if [ -e "$DASH_SRC" ]; then
    cp -L "$DASH_SRC" "$STAGE/bin/dash"
    cp -L "$DASH_SRC" "$STAGE/bin/sh"
    echo "[rootfs] staged real shell: /bin/dash (+ /bin/sh)"
else
    echo "[rootfs] WARNING: $DASH_SRC not found — shell self-tests will no-op"
fi

# --- a REAL build tool: GNU make ----------------------------------------------
# The first rung of the GCC/CMake/Make toolchain initiative (Path Z, design-
# decisions §9/§12).  make is the build *driver* that orchestrates a toolchain:
# it parses a Makefile, builds the dependency graph, and fork/execs each
# recipe via /bin/sh.  It is an unmodified glibc PIE that links ONLY against
# libc.so.6 + ld-linux (both already staged) — no extra libraries needed.  The
# SlateOS self-test (self_test_linux_real_glibc_make) writes a trivial Makefile,
# runs `make`, and asserts the recipe's output, proving make's startup, Makefile
# parse, and recipe dispatch (make -> /bin/sh -> /bin/emit) all work end to end.
MAKE_SRC="$(command -v make || true)"
if [ -n "$MAKE_SRC" ] && [ -e "$MAKE_SRC" ]; then
    cp -L "$MAKE_SRC" "$STAGE/bin/make"
    echo "[rootfs] staged build tool: /bin/make ($MAKE_SRC)"
    echo "[rootfs] make binary DT_NEEDED:"
    readelf -d "$STAGE/bin/make" 2>/dev/null | grep -E 'NEEDED|RUNPATH' | sed 's/^/  /'
else
    echo "[rootfs] WARNING: make not found — the make self-test will no-op"
fi

# --- a REAL C compiler: tcc (TinyCC) ------------------------------------------
# The next rung of the GCC/CMake/Make toolchain initiative (Path Z, design-
# decisions §9/§12): proving an unmodified, prebuilt C *compiler* runs in ring 3
# and produces a working executable.  tcc is the ideal first compiler: a single
# self-contained binary that lexes/parses/codegens AND assembles AND links
# internally (no separate cpp/as/ld needed).  It is a glibc dynamic ELF needing
# only libc.so.6 + libm.so.6 + ld-linux.  For a `-nostdlib -static` freestanding
# compile (the self-test's recipe), tcc opens NO support files at all — verified
# by strace: it reads only the .c source and writes the output ELF, needing
# neither libtcc1.a nor any headers — so we stage only the tcc binary + libm.
# (A hosted compile against the staged glibc/crt/headers is a later rung.)
#
# tcc is not on a default Ubuntu install and `apt install tcc` needs root, so
# this script accepts tcc from PATH or from a cached source build at
# /tmp/tccinstall/bin/tcc (build: git clone https://repo.or.cz/tinycc.git &&
# ./configure && make && make install prefix=/tmp/tccinstall).  Absent tcc the
# self-test no-ops, matching the make/dash best-effort pattern above.
TCC_SRC="$(command -v tcc || true)"
if [ -z "$TCC_SRC" ] && [ -x /tmp/tccinstall/bin/tcc ]; then
    TCC_SRC="/tmp/tccinstall/bin/tcc"
fi
if [ -n "$TCC_SRC" ] && [ -e "$TCC_SRC" ]; then
    cp -L "$TCC_SRC" "$STAGE/bin/tcc"
    # tcc's DT_NEEDED includes libm.so.6 (not staged for the glibc smoke tests);
    # stage it next to libc.so.6 so ld.so resolves it via the same RUNPATH.
    if [ -e "$LIBC_DIR/libm.so.6" ]; then
        cp -L "$LIBC_DIR/libm.so.6" "$STAGE$LIBC_DIR/libm.so.6"
    else
        echo "[rootfs] WARNING: libm.so.6 not found — tcc self-test will no-op (tcc won't load)"
    fi
    echo "[rootfs] staged C compiler: /bin/tcc ($TCC_SRC)"
    echo "[rootfs] tcc binary DT_NEEDED:"
    readelf -d "$STAGE/bin/tcc" 2>/dev/null | grep -E 'NEEDED|RUNPATH' | sed 's/^/  /'

    # --- hosted-compile support files (Path Z Part 36) ------------------------
    # The next rung after the freestanding `-nostdlib -static` compile: a *hosted*
    # compile that links the program against real glibc with crt startup
    # (crt1.o -> __libc_start_main -> main) and runs through ld-linux.  `tcc -vv`
    # shows the exact file set tcc opens for `tcc -o out out.c`:
    #   /usr/lib/x86_64-linux-gnu/crt1.o, crti.o, crtn.o
    #   /usr/lib/x86_64-linux-gnu/libc.so          (GNU-ld linker script)
    #   /lib/x86_64-linux-gnu/libc.so.6            (already staged)
    #   /usr/lib/x86_64-linux-gnu/libc_nonshared.a
    #   /lib64/ld-linux-x86-64.so.2                (already staged, AS_NEEDED)
    #   <tcc install dir>/libtcc1.a
    # Stage each at the EXACT absolute path tcc searches so they resolve
    # unchanged inside SlateOS (the libc.so script GROUPs the .so.6 + nonshared.a
    # + ld-linux by absolute path).  We declare prototypes via `extern` in the
    # self-test C source, so NO glibc header tree is needed.
    CRT_DIR="/usr/lib/x86_64-linux-gnu"
    mkdir -p "$STAGE$CRT_DIR"
    for f in crt1.o crti.o crtn.o libc_nonshared.a libc.so; do
        if [ -e "$CRT_DIR/$f" ]; then
            cp -L "$CRT_DIR/$f" "$STAGE$CRT_DIR/$f"
        else
            echo "[rootfs] WARNING: $CRT_DIR/$f missing — tcc hosted self-test will no-op"
        fi
    done
    # libtcc1.a lives in tcc's compiled-in install dir; stage it at that exact
    # absolute path so tcc finds it unchanged in the VFS.
    TCC_LIBDIR="$("$TCC_SRC" -print-search-dirs 2>/dev/null | sed -n 's/^install: //p' | head -1)"
    if [ -z "$TCC_LIBDIR" ]; then
        TCC_LIBDIR="$(dirname "$TCC_SRC")/../lib/tcc"
    fi
    if [ -e "$TCC_LIBDIR/libtcc1.a" ]; then
        ABS_LIBDIR="$(cd "$TCC_LIBDIR" && pwd)"
        mkdir -p "$STAGE$ABS_LIBDIR"
        cp -L "$TCC_LIBDIR/libtcc1.a" "$STAGE$ABS_LIBDIR/libtcc1.a"
        echo "[rootfs] staged hosted-compile support: crt objects + libc.so script + libtcc1.a ($ABS_LIBDIR)"
    else
        echo "[rootfs] WARNING: libtcc1.a not found ($TCC_LIBDIR) — tcc hosted self-test will no-op"
    fi
else
    echo "[rootfs] WARNING: tcc not found — the C-compiler self-test will no-op"
fi

echo "[rootfs] staged tree:"
( cd "$STAGE" && find . -type f -printf '  %-52p %10s bytes\n' )

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
