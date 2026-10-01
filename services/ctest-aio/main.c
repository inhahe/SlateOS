/*
 * ctest-aio: the ring-3 check of Linux kernel AIO -- io_setup, io_submit,
 * io_getevents, io_pgetevents, io_cancel and io_destroy -- as a C program
 * reaches them: through syscall(), as libaio does.
 *
 * posix/src/linux_aio_abi.rs rebuilt them on 2026-09-26 to Linux 6.6's
 * fs/aio.c.  The host tests drive the ring, the checks and the waits with
 * the transfer and the other thread played by closures; only a ring-3 run
 * shows a real read and write landing in a real ring, a real thread asleep in
 * io_getevents woken by another's io_submit, and a C caller finding the ring
 * where its context id points.
 *
 * Exit codes -- one per check:
 *    42  every check passed
 *    10  io_setup failed          11  the ring header is not Linux's
 *    20  creating the file failed 21  writing its first contents failed
 *    30  PWRITE not submitted     31  its event not taken
 *    32  the event's res, data or obj is wrong
 *    40  PREAD not submitted      41  its event not taken
 *    43  PREAD's event is wrong   44  the bytes read are not the ones written
 *    50  PREADV not submitted     51  its event not taken
 *    52  the two segments do not hold what was read
 *    55  pipe() failed            56  PWRITE to the pipe failed
 *    57  PREAD from the pipe failed  58  the pipe's bytes are wrong
 *    60  NOOP was not EINVAL at submission
 *    61  a closed descriptor was not EBADF at submission
 *    62  a NULL iocb was not EFAULT
 *    63  io_cancel of a NULL iocb was not EFAULT
 *    64  io_pgetevents with a 7-byte mask was not EINVAL
 *    70  creating the waiter failed
 *    71  the waiter finished before anything was submitted (it did not wait)
 *    72  the submission that should wake it failed
 *    73  joining it failed        74  it did not come back with its event
 *    75  a 50 ms wait for nothing did not answer 0
 *    76  it answered in under 40 ms (the timeout was not relative)
 *    80  eventfd() failed         81  the RESFD submission failed
 *    82  the eventfd was not signalled once
 *    85  the event was not in the ring for the caller to take itself
 *    86  io_getevents returned an event the caller had already taken
 *    90  creating the second waiter failed
 *    91  it finished before io_destroy
 *    92  io_destroy failed        93  joining it failed
 *    94  the waiter did not fail with EINVAL when its context was destroyed
 *    95  a second io_destroy was not EINVAL
 *
 * A hang instead of an exit code is a lost wake-up: a waiter in io_getevents
 * that neither a submission nor io_destroy reached.  That is either
 * posix/src/objtable.rs's counter or the kernel's futex.
 */

#define _GNU_SOURCE 1 /* syscall() in <unistd.h> */

#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdint.h>
#include <string.h>
#include <sys/eventfd.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

/* <linux/aio_abi.h> is not in the sysroot's header set.  These are the
 * kernel's fixed layouts, little-endian x86-64. */
typedef uint64_t aio_context_t;

struct iocb {
    uint64_t aio_data;
    uint32_t aio_key;
    int32_t aio_rw_flags;
    uint16_t aio_lio_opcode;
    int16_t aio_reqprio;
    uint32_t aio_fildes;
    uint64_t aio_buf;
    uint64_t aio_nbytes;
    int64_t aio_offset;
    uint64_t aio_reserved2;
    uint32_t aio_flags;
    uint32_t aio_resfd;
};

struct io_event {
    uint64_t data;
    uint64_t obj;
    int64_t res;
    int64_t res2;
};

struct aio_ring {
    unsigned id, nr, head, tail;
    unsigned magic, compat_features, incompat_features, header_length;
    struct io_event io_events[];
};

struct aio_sigset {
    const void *sigmask;
    size_t sigsetsize;
};

enum {
    CMD_PREAD = 0,
    CMD_PWRITE = 1,
    CMD_NOOP = 6,
    CMD_PREADV = 7,
    FLAG_RESFD = 1,
};

#define PASS 42
#define PATH "/tmp/ctest-aio"

static long io_setup(unsigned nr, aio_context_t *ctx) {
    return syscall(SYS_io_setup, nr, ctx);
}
static long io_destroy(aio_context_t ctx) { return syscall(SYS_io_destroy, ctx); }
static long io_submit(aio_context_t ctx, long nr, struct iocb **iocbs) {
    return syscall(SYS_io_submit, ctx, nr, iocbs);
}
static long io_getevents(aio_context_t ctx, long min, long nr, struct io_event *ev,
                         struct timespec *ts) {
    return syscall(SYS_io_getevents, ctx, min, nr, ev, ts);
}

static void pause_ms(long ms) {
    struct timespec ts = {ms / 1000, (ms % 1000) * 1000000L};
    nanosleep(&ts, 0);
}

static long now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

static struct iocb make(int op, int fd, const void *buf, uint64_t n, int64_t off,
                        uint64_t data) {
    struct iocb c;
    memset(&c, 0, sizeof c);
    c.aio_lio_opcode = (uint16_t)op;
    c.aio_fildes = (uint32_t)fd;
    c.aio_buf = (uint64_t)(uintptr_t)buf;
    c.aio_nbytes = n;
    c.aio_offset = off;
    c.aio_data = data;
    return c;
}

/* Submit one iocb and take its event, waiting as long as it takes. */
static int one(aio_context_t ctx, struct iocb *c, struct io_event *ev, int bad_submit,
               int bad_take) {
    struct iocb *list[1] = {c};
    if (io_submit(ctx, 1, list) != 1) {
        return bad_submit;
    }
    if (io_getevents(ctx, 1, 1, ev, 0) != 1) {
        return bad_take;
    }
    return 0;
}

struct waiter {
    aio_context_t ctx;
    volatile int done;
    long result;
    int err;
    struct io_event ev;
};

static void *wait_one(void *arg) {
    struct waiter *w = arg;
    w->result = io_getevents(w->ctx, 1, 1, &w->ev, 0);
    w->err = errno;
    w->done = 1;
    return 0;
}

int main(void) {
    aio_context_t ctx = 0;
    struct io_event ev;
    int rc;

    if (io_setup(128, &ctx) != 0 || ctx == 0) {
        return 10;
    }
    {
        const struct aio_ring *r = (const struct aio_ring *)(uintptr_t)ctx;
        if (r->magic != 0xa10a10a1u || r->header_length != 32 || r->nr < 256 ||
            r->incompat_features != 0 || r->head != r->tail) {
            return 11;
        }
    }

    /* ---- a file: write, read back, read into two segments ---- */
    int fd = open(PATH, O_RDWR | O_CREAT | O_TRUNC, 0600);
    if (fd < 0) {
        return 20;
    }
    if (write(fd, "0123456789", 10) != 10) {
        return 21;
    }
    struct iocb w = make(CMD_PWRITE, fd, "ABCD", 4, 2, 0x1111);
    if ((rc = one(ctx, &w, &ev, 30, 31)) != 0) {
        return rc;
    }
    if (ev.res != 4 || ev.data != 0x1111 || ev.obj != (uint64_t)(uintptr_t)&w) {
        return 32;
    }
    char back[16] = {0};
    struct iocb r = make(CMD_PREAD, fd, back, 10, 0, 0x2222);
    if ((rc = one(ctx, &r, &ev, 40, 41)) != 0) {
        return rc;
    }
    if (ev.res != 10 || ev.data != 0x2222) {
        return 43;
    }
    if (memcmp(back, "01ABCD6789", 10) != 0) {
        return 44;
    }
    char a[3] = {0}, b[4] = {0};
    struct {
        void *base;
        size_t len;
    } iov[2] = {{a, 3}, {b, 4}};
    struct iocb rv = make(CMD_PREADV, fd, iov, 2, 1, 0x3333);
    if ((rc = one(ctx, &rv, &ev, 50, 51)) != 0) {
        return rc;
    }
    if (ev.res != 7 || memcmp(a, "1AB", 3) != 0 || memcmp(b, "CD67", 4) != 0) {
        return 52;
    }

    /* ---- a pipe: no position, and the offset means nothing ---- */
    int p[2];
    if (pipe(p) != 0) {
        return 55;
    }
    struct iocb pw = make(CMD_PWRITE, p[1], "pipe", 4, 0, 0x5555);
    if (one(ctx, &pw, &ev, 56, 56) != 0 || ev.res != 4) {
        return 56;
    }
    char pb[8] = {0};
    struct iocb pr = make(CMD_PREAD, p[0], pb, 4, 0, 0x5656);
    if (one(ctx, &pr, &ev, 57, 57) != 0 || ev.res != 4) {
        return 57;
    }
    if (memcmp(pb, "pipe", 4) != 0) {
        return 58;
    }

    /* ---- refusals are refusals, at submission ---- */
    {
        struct iocb n = make(CMD_NOOP, fd, 0, 0, 0, 0);
        struct iocb *list[1] = {&n};
        errno = 0;
        if (io_submit(ctx, 1, list) != -1 || errno != EINVAL) {
            return 60;
        }
        struct iocb closed = make(CMD_PREAD, 250, back, 1, 0, 0);
        list[0] = &closed;
        errno = 0;
        if (io_submit(ctx, 1, list) != -1 || errno != EBADF) {
            return 61;
        }
        list[0] = 0;
        errno = 0;
        if (io_submit(ctx, 1, list) != -1 || errno != EFAULT) {
            return 62;
        }
        errno = 0;
        if (syscall(SYS_io_cancel, ctx, (void *)0, (void *)0) != -1 || errno != EFAULT) {
            return 63;
        }
        uint64_t mask = 0;
        struct aio_sigset s = {&mask, 7};
        struct timespec zero = {0, 0};
        errno = 0;
        if (syscall(SYS_io_pgetevents, ctx, 1L, 1L, &ev, &zero, &s) != -1 ||
            errno != EINVAL) {
            return 64;
        }
    }

    /* ---- another thread waits; this one's submission wakes it ---- */
    {
        struct waiter wt = {ctx, 0, 0, 0, {0, 0, 0, 0}};
        pthread_t t;
        if (pthread_create(&t, 0, wait_one, &wt) != 0) {
            return 70;
        }
        pause_ms(100);
        if (wt.done) {
            return 71;
        }
        char c1[2] = {0};
        struct iocb wake = make(CMD_PREAD, fd, c1, 1, 0, 0x7777);
        struct iocb *list[1] = {&wake};
        if (io_submit(ctx, 1, list) != 1) {
            return 72;
        }
        if (pthread_join(t, 0) != 0) {
            return 73;
        }
        if (wt.result != 1 || wt.ev.data != 0x7777 || wt.ev.res != 1) {
            return 74;
        }
    }

    /* ---- a relative timeout ---- */
    {
        struct timespec fifty = {0, 50 * 1000000L};
        long t0 = now_ms();
        if (io_getevents(ctx, 1, 1, &ev, &fifty) != 0) {
            return 75;
        }
        if (now_ms() - t0 < 40) {
            return 76;
        }
    }

    /* ---- IOCB_FLAG_RESFD: the eventfd is signalled once ---- */
    {
        int efd = eventfd(0, 0);
        if (efd < 0) {
            return 80;
        }
        char c1[2] = {0};
        struct iocb n = make(CMD_PREAD, fd, c1, 1, 0, 0x8888);
        n.aio_flags = FLAG_RESFD;
        n.aio_resfd = (uint32_t)efd;
        struct iocb *list[1] = {&n};
        if (io_submit(ctx, 1, list) != 1) {
            return 81;
        }
        uint64_t count = 0;
        if (read(efd, &count, sizeof count) != (ssize_t)sizeof count || count != 1) {
            return 82;
        }
        close(efd);

        /* ---- the caller reaps the ring itself, as libaio may ---- */
        struct aio_ring *ring = (struct aio_ring *)(uintptr_t)ctx;
        unsigned head = ring->head, tail = ring->tail;
        if (head == tail || ring->io_events[head % ring->nr].data != 0x8888) {
            return 85;
        }
        __atomic_store_n(&ring->head, (head + 1) % ring->nr, __ATOMIC_RELEASE);
        struct timespec zero = {0, 0};
        if (io_getevents(ctx, 0, 1, &ev, &zero) != 0) {
            return 86;
        }
    }

    /* ---- io_destroy wakes a waiter, and fails it with EINVAL ---- */
    {
        struct waiter wt = {ctx, 0, 0, 0, {0, 0, 0, 0}};
        pthread_t t;
        if (pthread_create(&t, 0, wait_one, &wt) != 0) {
            return 90;
        }
        pause_ms(100);
        if (wt.done) {
            return 91;
        }
        if (io_destroy(ctx) != 0) {
            return 92;
        }
        if (pthread_join(t, 0) != 0) {
            return 93;
        }
        if (wt.result != -1 || wt.err != EINVAL) {
            return 94;
        }
    }
    errno = 0;
    if (io_destroy(ctx) != -1 || errno != EINVAL) {
        return 95;
    }

    close(p[0]);
    close(p[1]);
    close(fd);
    unlink(PATH);
    return PASS;
}
