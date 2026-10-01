/*
 * ctest-sysvipc — native C ring-3 fixture for System V message queues and
 * semaphores between threads.
 *
 * Compiled by `zig cc --target=x86_64-linux-musl` and linked against the
 * posix sysroot `libc.a` (see build.py).  The kernel runs it as a ring-3
 * self-test and asserts the exit code (requested of lane A in
 * `requests/d-a-run-the-ctest-sysvipc-fixture.md`).
 *
 * Why it exists.  posix/src/sysv_msg.rs blocks a msgsnd on a full queue and
 * a msgrcv on an empty one by sleeping on a futex until the queues change,
 * and fails a call blocked on a queue that is removed with EIDRM;
 * posix/src/sysv_sem.rs does the same for semop.  The host
 * tests drive those loops with the waiting step played by the test itself;
 * only here does a sleeping thread depend on another thread's change waking
 * it.  It also checks, where real programs would see them, the fixes that
 * came with it: msgrcv takes a buffer longer than 256 bytes (it used to
 * refuse one with EINVAL), a queue holds 16384 bytes, and MSG_INFO and
 * MSG_STAT list the queues as `ipcs -q` walks them.  For semaphores it
 * checks what only a C caller can: semctl is variadic in C, and its fourth
 * argument -- SETVAL's value, GETALL's array -- used to be dropped, since
 * the library's semctl took three.  And semtimedop's timeout is relative:
 * it used to be read as a date, so every timed wait ended at once.
 *
 * Exit codes: 42 = every check passed.  Anything else names the failing step:
 *
 *   10  msgget failed
 *   11  a new queue's msg_qbytes is not 16384, or IPC_STAT failed
 *
 *   20  msgsnd of a small message failed                (the large buffer)
 *   21  msgrcv into an 8192-byte buffer failed or read the wrong message
 *
 *   30  pthread_create failed                           (a blocked receive)
 *   31  the receiver finished before anything was sent: it did not block
 *   32  msgsnd to the blocked receiver failed
 *   33  pthread_join failed
 *   34  the receiver failed, or got the wrong message
 *
 *   40  filling the queue to its 16384 bytes failed     (a blocked send)
 *   41  one more byte with IPC_NOWAIT did not answer EAGAIN
 *   42  (not used: 42 is success)
 *   43  pthread_create failed
 *   44  the sender finished before room was made: it did not block
 *   45  msgrcv to make room failed
 *   46  pthread_join failed
 *   47  the blocked sender failed
 *
 *   50  pthread_create failed                           (IPC_RMID)
 *   51  the receiver finished before the queue was removed
 *   52  msgctl(IPC_RMID) failed
 *   53  pthread_join failed
 *   54  the receiver did not fail with EIDRM
 *
 *   60  msgctl(MSG_INFO) failed                         (the ipcs walk)
 *   61  MSG_STAT over 0..=MSG_INFO's index did not find the three queues
 *
 *   70  semget failed                                  (semaphores)
 *   71  semctl(id, 0, SETVAL, 1) failed, or GETVAL did not read 1 back:
 *       the fourth argument did not arrive
 *   72  SETALL/GETALL through a union semun did not round-trip
 *   73  pthread_create failed                          (a blocked semop)
 *   74  the waiter finished before the semaphore was raised: it did not block
 *   75  GETNCNT did not count the waiter
 *   76  raising the semaphore failed, or the waiter's semop failed
 *   77  semtimedop on a zero semaphore did not fail with EAGAIN
 *   78  it came back in under 150 ms of a 300 ms timeout: the timeout was
 *       not taken as relative
 *   79  IPC_RMID under a blocked semop did not fail it with EIDRM
 *
 * Progress lines go to stdout as "[msg] ..." and "[sem] ...".
 */

#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <stdio.h>
#include <string.h>
#include <sys/ipc.h>
#include <sys/msg.h>
#include <sys/sem.h>
#include <time.h>

#define MSGMAX 8192
#define MSGMNB 16384

struct big {
    long mtype;
    char mtext[MSGMAX];
};

static struct big out_msg;

/* A worker's call and its outcome; `done` is set once the call returned. */
struct call {
    int q;
    long mtype;
    size_t len;
    long result;
    int err;
    struct big buf;
    volatile int done;
};

static void pause_ms(long ms)
{
    struct timespec ts = { ms / 1000, (ms % 1000) * 1000000L };
    nanosleep(&ts, NULL);
}

static void *receiver(void *arg)
{
    struct call *c = arg;
    c->result = msgrcv(c->q, &c->buf, sizeof c->buf.mtext, c->mtype, 0);
    c->err = errno;
    __atomic_store_n(&c->done, 1, __ATOMIC_SEQ_CST);
    return NULL;
}

static void *sender(void *arg)
{
    struct call *c = arg;
    c->buf.mtype = c->mtype;
    memset(c->buf.mtext, 's', c->len);
    c->result = msgsnd(c->q, &c->buf, c->len, 0);
    c->err = errno;
    __atomic_store_n(&c->done, 1, __ATOMIC_SEQ_CST);
    return NULL;
}

static int done(struct call *c)
{
    return __atomic_load_n(&c->done, __ATOMIC_SEQ_CST);
}

/* C programs define union semun themselves. */
union semun {
    int val;
    struct semid_ds *buf;
    unsigned short *array;
};

struct semcall {
    int id;
    int result;
    int err;
    volatile int done;
};

static void *sem_waiter(void *arg)
{
    struct semcall *c = arg;
    struct sembuf down = { 0, -1, 0 };
    c->result = semop(c->id, &down, 1);
    c->err = errno;
    __atomic_store_n(&c->done, 1, __ATOMIC_SEQ_CST);
    return NULL;
}

static long elapsed_ms(const struct timespec *a, const struct timespec *b)
{
    return (b->tv_sec - a->tv_sec) * 1000L + (b->tv_nsec - a->tv_nsec) / 1000000L;
}

static int sem_checks(void)
{
    static struct semcall sc;
    pthread_t t;
    int id = semget(IPC_PRIVATE, 3, 0600 | IPC_CREAT);
    if (id < 0)
        return 70;

    /* The regression: a bare int as the variadic fourth argument. */
    if (semctl(id, 0, SETVAL, 1) != 0 || semctl(id, 0, GETVAL) != 1)
        return 71;
    {
        unsigned short set[3] = { 4, 5, 6 }, got[3] = { 0, 0, 0 };
        union semun arg;
        arg.array = set;
        if (semctl(id, 0, SETALL, arg) != 0)
            return 72;
        arg.array = got;
        if (semctl(id, 0, GETALL, arg) != 0 || got[0] != 4 || got[1] != 5 || got[2] != 6)
            return 72;
    }
    printf("[sem] semctl's fourth argument arrives\n");

    /* A blocked semop, woken by another thread's. */
    if (semctl(id, 0, SETVAL, 0) != 0)
        return 76;
    memset(&sc, 0, sizeof sc);
    sc.id = id;
    if (pthread_create(&t, NULL, sem_waiter, &sc) != 0)
        return 73;
    pause_ms(100);
    if (__atomic_load_n(&sc.done, __ATOMIC_SEQ_CST))
        return 74;
    if (semctl(id, 0, GETNCNT) != 1)
        return 75;
    {
        struct sembuf up = { 0, 1, 0 };
        if (semop(id, &up, 1) != 0)
            return 76;
    }
    pthread_join(t, NULL);
    if (sc.result != 0 || semctl(id, 0, GETVAL) != 0)
        return 76;
    printf("[sem] a blocked semop woke when the semaphore rose\n");

    /* semtimedop's timeout is how long to wait. */
    {
        struct sembuf down = { 0, -1, 0 };
        struct timespec limit = { 0, 300000000L }, t0, t1;
        clock_gettime(CLOCK_MONOTONIC, &t0);
        int r = semtimedop(id, &down, 1, &limit);
        int e = errno;
        clock_gettime(CLOCK_MONOTONIC, &t1);
        if (r != -1 || e != EAGAIN)
            return 77;
        if (elapsed_ms(&t0, &t1) < 150)
            return 78;
        printf("[sem] semtimedop waited %ld ms of 300\n", elapsed_ms(&t0, &t1));
    }

    /* IPC_RMID under a blocked semop. */
    memset(&sc, 0, sizeof sc);
    sc.id = id;
    if (pthread_create(&t, NULL, sem_waiter, &sc) != 0)
        return 73;
    pause_ms(100);
    if (__atomic_load_n(&sc.done, __ATOMIC_SEQ_CST))
        return 74;
    if (semctl(id, 0, IPC_RMID) != 0)
        return 79;
    pthread_join(t, NULL);
    if (sc.result != -1 || sc.err != EIDRM)
        return 79;
    printf("[sem] IPC_RMID failed the blocked semop with EIDRM\n");
    return 0;
}

static int new_queue(void)
{
    return msgget(IPC_PRIVATE, 0600 | IPC_CREAT);
}

int main(void)
{
    static struct call c;
    pthread_t t;
    struct msqid_ds ds;

    /* -- a queue is Linux's size -- */
    int q = new_queue();
    if (q < 0)
        return 10;
    if (msgctl(q, IPC_STAT, &ds) != 0 || ds.msg_qbytes != MSGMNB)
        return 11;
    printf("[msg] a new queue holds %lu bytes\n", (unsigned long)ds.msg_qbytes);

    /* -- msgrcv takes a buffer longer than 256 bytes -- */
    out_msg.mtype = 5;
    memcpy(out_msg.mtext, "hello", 5);
    if (msgsnd(q, &out_msg, 5, IPC_NOWAIT) != 0)
        return 20;
    memset(&c, 0, sizeof c);
    if (msgrcv(q, &c.buf, sizeof c.buf.mtext, 0, IPC_NOWAIT) != 5 || c.buf.mtype != 5 ||
        memcmp(c.buf.mtext, "hello", 5) != 0)
        return 21;
    printf("[msg] msgrcv into an 8192-byte buffer: ok\n");

    /* -- a blocked receive is woken by a send -- */
    memset(&c, 0, sizeof c);
    c.q = q;
    c.mtype = 7;
    if (pthread_create(&t, NULL, receiver, &c) != 0)
        return 30;
    pause_ms(100);
    if (done(&c))
        return 31;
    out_msg.mtype = 3; /* not the type it waits for */
    if (msgsnd(q, &out_msg, 1, IPC_NOWAIT) != 0)
        return 32;
    pause_ms(50);
    if (done(&c))
        return 31;
    out_msg.mtype = 7;
    memcpy(out_msg.mtext, "wake", 4);
    if (msgsnd(q, &out_msg, 4, IPC_NOWAIT) != 0)
        return 32;
    if (pthread_join(t, NULL) != 0)
        return 33;
    if (c.result != 4 || c.buf.mtype != 7 || memcmp(c.buf.mtext, "wake", 4) != 0)
        return 34;
    if (msgrcv(q, &c.buf, sizeof c.buf.mtext, 3, IPC_NOWAIT) != 1)
        return 34;
    printf("[msg] a blocked msgrcv woke for its type, not another\n");

    /* -- a blocked send is woken by a receive -- */
    out_msg.mtype = 1;
    memset(out_msg.mtext, 'f', MSGMAX);
    if (msgsnd(q, &out_msg, MSGMAX, IPC_NOWAIT) != 0 || msgsnd(q, &out_msg, MSGMAX, IPC_NOWAIT) != 0)
        return 40;
    if (msgsnd(q, &out_msg, 1, IPC_NOWAIT) != -1 || errno != EAGAIN)
        return 41;
    memset(&c, 0, sizeof c);
    c.q = q;
    c.mtype = 2;
    c.len = 100;
    if (pthread_create(&t, NULL, sender, &c) != 0)
        return 43;
    pause_ms(100);
    if (done(&c))
        return 44;
    {
        static struct big room;
        if (msgrcv(q, &room, sizeof room.mtext, 1, IPC_NOWAIT) != MSGMAX)
            return 45;
    }
    if (pthread_join(t, NULL) != 0)
        return 46;
    if (c.result != 0)
        return 47;
    printf("[msg] a blocked msgsnd woke when room was made\n");
    msgctl(q, IPC_RMID, NULL);

    /* -- IPC_RMID under a blocked receive is EIDRM -- */
    q = new_queue();
    if (q < 0)
        return 10;
    memset(&c, 0, sizeof c);
    c.q = q;
    if (pthread_create(&t, NULL, receiver, &c) != 0)
        return 50;
    pause_ms(100);
    if (done(&c))
        return 51;
    if (msgctl(q, IPC_RMID, NULL) != 0)
        return 52;
    if (pthread_join(t, NULL) != 0)
        return 53;
    if (c.result != -1 || c.err != EIDRM)
        return 54;
    printf("[msg] IPC_RMID failed the blocked msgrcv with EIDRM\n");

    /* -- MSG_INFO and MSG_STAT, as ipcs -q walks them -- */
    {
        int qs[3], found = 0;
        struct msginfo info;
        for (int i = 0; i < 3; i++) {
            qs[i] = new_queue();
            if (qs[i] < 0)
                return 10;
        }
        int max = msgctl(0, MSG_INFO, (struct msqid_ds *)&info);
        if (max < 0 || info.msgpool < 3)
            return 60;
        for (int i = 0; i <= max; i++) {
            int id = msgctl(i, MSG_STAT, &ds);
            for (int k = 0; k < 3; k++)
                if (id >= 0 && id == qs[k])
                    found++;
        }
        if (found != 3)
            return 61;
        for (int i = 0; i < 3; i++)
            msgctl(qs[i], IPC_RMID, NULL);
        printf("[msg] MSG_INFO and MSG_STAT listed the queues\n");
    }

    {
        int r = sem_checks();
        if (r != 0)
            return r;
    }

    printf("[ipc] all checks passed\n");
    return 42;
}
