"""What a signal does to a call blocked in glibc 2.39 -- the functions that
wait inside the C library, each blocked on a thread of its own and sent a
signal five ways -- as the oracle for posix/src/interrupt.rs.

    python posix/tools/oracle/interrupt_harness.py   # writes posix/src/interrupt_oracle.txt

The five ways: a handler installed without `SA_RESTART`; one installed with
it; the signal set to `SIG_IGN`; a signal whose default is to be ignored
(`SIGURG`); and a handler that runs on another thread, the signal having been
sent there. Each line says whether the call ended as soon as the signal came
(`ends at once`, and what it answered) or went on waiting until what it waits
for was supplied (`waits on`, and what it then answered).

Everything a call waits on is made to never arrive until the probe supplies
it: an empty semaphore or queue, a full one, an empty pipe under an AIO read,
and, for `gai_suspend`, a lookup whose worker is stuck opening /etc/hosts --
a FIFO, on a tmpfs over /etc in a namespace of the probe's own, which opens
only when the probe opens it for writing. The System V objects and the
message queue are in an IPC namespace of the probe's own too. The waits that
take a timeout are given ten seconds, which the probe never lets run out.

Each case runs in a process of its own, and a call that ended early is left
as it ended: glibc's `lio_listio(LIO_WAIT)`, ended by a signal, leaves its
requests pointing at the waiting list it kept in its own stack frame, so
their completing afterwards writes into -- and follows pointers out of -- a
frame that is gone. Before the cases were separated, finishing that one
wedged every case after it.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "interrupt_oracle.txt"

PROGRAM = r'''
#define _GNU_SOURCE
#include <aio.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/aio_abi.h>
#include <linux/futex.h>
#include <mqueue.h>
#include <netdb.h>
#include <pthread.h>
#include <sched.h>
#include <semaphore.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ipc.h>
#include <sys/msg.h>
#include <sys/sem.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static const char *en(int e)
{
    switch (e) {
    case EINTR: return "EINTR";
    case ETIMEDOUT: return "ETIMEDOUT";
    case EAGAIN: return "EAGAIN";
    case EINVAL: return "EINVAL";
    case EIO: return "EIO";
    default: { static char b[16]; snprintf(b, sizeof b, "%d", e); return b; }
    }
}

static const char *eai(int e)
{
    switch (e) {
    case 0: return "0";
    case EAI_INTR: return "EAI_INTR";
    case EAI_AGAIN: return "EAI_AGAIN";
    case EAI_ALLDONE: return "EAI_ALLDONE";
    case EAI_SYSTEM: return "EAI_SYSTEM";
    case EAI_NONAME: return "EAI_NONAME";
    case EAI_INPROGRESS: return "EAI_INPROGRESS";
    default: { static char b[16]; snprintf(b, sizeof b, "%d", e); return b; }
    }
}

/* The waiter's answer. */
static char out[64];

/* A call's value, and errno's name when it is -1. */
static void said(long rc)
{
    if (rc == -1)
        snprintf(out, sizeof out, "-1 %s", en(errno));
    else
        snprintf(out, sizeof out, "%ld", rc);
}

static struct timespec in_ten(clockid_t clock)
{
    struct timespec t;
    clock_gettime(clock, &t);
    t.tv_sec += 10;
    return t;
}

static const struct timespec ten = { 10, 0 };

static void nothing(void) {}

/* POSIX semaphores. */
static sem_t sem;
static void sem_setup(void) { sem_init(&sem, 0, 0); }
static void sem_release(void) { sem_post(&sem); }
static void sem_teardown(void) { sem_destroy(&sem); }
static void w_sem_wait(void) { said(sem_wait(&sem)); }
static void w_sem_timedwait(void)
{
    struct timespec t = in_ten(CLOCK_REALTIME);
    said(sem_timedwait(&sem, &t));
}
static void w_sem_clockwait(void)
{
    struct timespec t = in_ten(CLOCK_MONOTONIC);
    said(sem_clockwait(&sem, CLOCK_MONOTONIC, &t));
}

/* POSIX message queues: an empty one for the receivers, a full one (a
 * message of its one) for the senders. */
static mqd_t mq;
static void mq_setup(void)
{
    struct mq_attr a = { .mq_maxmsg = 1, .mq_msgsize = 8 };
    mq_unlink("/interrupt-probe");
    mq = mq_open("/interrupt-probe", O_CREAT | O_RDWR, 0600, &a);
    if (mq == (mqd_t)-1) { perror("mq_open"); exit(1); }
}
static void mq_full_setup(void) { mq_setup(); mq_send(mq, "f", 1, 0); }
static void mq_feed(void) { mq_send(mq, "r", 1, 0); }
static void mq_drain(void) { char b[8]; mq_receive(mq, b, sizeof b, NULL); }
static void mq_teardown(void) { mq_close(mq); mq_unlink("/interrupt-probe"); }
static void w_mq_receive(void) { char b[8]; said(mq_receive(mq, b, sizeof b, NULL)); }
static void w_mq_timedreceive(void)
{
    char b[8];
    struct timespec t = in_ten(CLOCK_REALTIME);
    said(mq_timedreceive(mq, b, sizeof b, NULL, &t));
}
static void w_mq_send(void) { said(mq_send(mq, "s", 1, 0)); }
static void w_mq_timedsend(void)
{
    struct timespec t = in_ten(CLOCK_REALTIME);
    said(mq_timedsend(mq, "s", 1, 0, &t));
}

/* System V messages: an empty queue, or one whose 8 bytes are taken. */
struct message { long type; char text[8]; };
static int msqid;
static void msg_setup(void) { msqid = msgget(IPC_PRIVATE, IPC_CREAT | 0600); }
static void msg_full_setup(void)
{
    msg_setup();
    struct msqid_ds ds;
    msgctl(msqid, IPC_STAT, &ds);
    ds.msg_qbytes = 8;
    msgctl(msqid, IPC_SET, &ds);
    struct message m = { 1, "full" };
    msgsnd(msqid, &m, 8, 0);
}
static void msg_feed(void) { struct message m = { 1, "r" }; msgsnd(msqid, &m, 1, IPC_NOWAIT); }
static void msg_drain(void) { struct message m; msgrcv(msqid, &m, 8, 0, IPC_NOWAIT); }
static void msg_teardown(void) { msgctl(msqid, IPC_RMID, NULL); }
static void w_msgrcv(void) { struct message m; said(msgrcv(msqid, &m, 8, 0, 0)); }
static void w_msgsnd(void) { struct message m = { 1, "more" }; said(msgsnd(msqid, &m, 8, 0)); }

/* System V semaphores: one, at 0. */
static int semid;
static void semop_setup(void) { semid = semget(IPC_PRIVATE, 1, IPC_CREAT | 0600); }
static void semop_release(void) { struct sembuf b = { 0, 1, 0 }; semop(semid, &b, 1); }
static void semop_teardown(void) { semctl(semid, 0, IPC_RMID); }
static void w_semop(void) { struct sembuf b = { 0, -1, 0 }; said(semop(semid, &b, 1)); }
static void w_semtimedop(void) { struct sembuf b = { 0, -1, 0 }; said(semtimedop(semid, &b, 1, &ten)); }

/* Kernel AIO: a context with nothing submitted, until a read of a file. */
static aio_context_t ctx;
static int file;
static void kaio_setup(void)
{
    ctx = 0;
    if (syscall(SYS_io_setup, 8, &ctx) != 0) { perror("io_setup"); exit(1); }
    file = open("/tmp/interrupt-file", O_RDONLY);
}
static void kaio_release(void)
{
    static char buf[4];
    struct iocb cb;
    memset(&cb, 0, sizeof cb);
    cb.aio_lio_opcode = IOCB_CMD_PREAD;
    cb.aio_fildes = file;
    cb.aio_buf = (unsigned long)buf;
    cb.aio_nbytes = 1;
    struct iocb *list[1] = { &cb };
    syscall(SYS_io_submit, ctx, 1, list);
}
static void kaio_teardown(void) { syscall(SYS_io_destroy, ctx); close(file); }
static void w_io_getevents(void) { struct io_event ev[1]; said(syscall(SYS_io_getevents, ctx, 1, 1, ev, NULL)); }
static void w_io_getevents_timed(void)
{
    struct io_event ev[1];
    struct timespec t = ten;
    said(syscall(SYS_io_getevents, ctx, 1, 1, ev, &t));
}

/* POSIX AIO: a read of an empty pipe, in flight until the pipe is written. */
static int fds[2];
static struct aiocb acb;
static char abuf[4];
static void lio_setup(void)
{
    if (pipe(fds) != 0) { perror("pipe"); exit(1); }
    memset(&acb, 0, sizeof acb);
    acb.aio_fildes = fds[0];
    acb.aio_buf = abuf;
    acb.aio_nbytes = 1;
    acb.aio_lio_opcode = LIO_READ;
}
static void aio_setup(void) { lio_setup(); aio_read(&acb); }
static void aio_release(void) { if (write(fds[1], "x", 1) != 1) perror("write"); }
static void aio_teardown(void)
{
    while (aio_error(&acb) == EINPROGRESS)
        usleep(1000);
    aio_return(&acb);
    close(fds[0]);
    close(fds[1]);
}
static void w_aio_suspend(void) { const struct aiocb *l[1] = { &acb }; said(aio_suspend(l, 1, NULL)); }
static void w_aio_suspend_timed(void) { const struct aiocb *l[1] = { &acb }; said(aio_suspend(l, 1, &ten)); }
static void w_lio_listio(void) { struct aiocb *l[1] = { &acb }; said(lio_listio(LIO_WAIT, l, 1, NULL)); }

/* Asynchronous lookups: one, its worker stuck opening the FIFO at
 * /etc/hosts. Opening the FIFO for writing lets every open of it through;
 * the release writes it a hosts file to read. */
static struct gaicb gcb;
static struct addrinfo hints;
static void gai_setup(void)
{
    memset(&gcb, 0, sizeof gcb);
    memset(&hints, 0, sizeof hints);
    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_STREAM;
    gcb.ar_name = "localhost";
    gcb.ar_request = &hints;
    struct gaicb *list[1] = { &gcb };
    int rc = getaddrinfo_a(GAI_NOWAIT, list, 1, NULL);
    if (rc != 0) { printf("getaddrinfo_a = %s\n", eai(rc)); exit(1); }
}
static void gai_release(void)
{
    static const char hosts[] = "127.0.0.1 localhost\n";
    int fd = open("/etc/hosts", O_WRONLY);
    if (fd >= 0) {
        if (write(fd, hosts, sizeof hosts - 1) < 0) perror("write");
        close(fd);
    }
}
static void gai_teardown(void)
{
    while (gai_error(&gcb) == EAI_INPROGRESS) {
        int fd = open("/etc/hosts", O_WRONLY | O_NONBLOCK);
        if (fd >= 0) close(fd);
        usleep(1000);
    }
    if (gcb.ar_result) freeaddrinfo(gcb.ar_result);
}
static void w_gai_suspend(void)
{
    const struct gaicb *l[1] = { &gcb };
    snprintf(out, sizeof out, "%s", eai(gai_suspend(l, 1, NULL)));
}
static void w_gai_suspend_timed(void)
{
    const struct gaicb *l[1] = { &gcb };
    snprintf(out, sizeof out, "%s", eai(gai_suspend(l, 1, &ten)));
}
/* GAI_WAIT: the lookup is made in the call, which waits for it. */
static void gai_wait_setup(void)
{
    memset(&gcb, 0, sizeof gcb);
    memset(&hints, 0, sizeof hints);
    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_STREAM;
    gcb.ar_name = "localhost";
    gcb.ar_request = &hints;
}
static void w_gai_wait(void)
{
    struct gaicb *list[1] = { &gcb };
    int rc = getaddrinfo_a(GAI_WAIT, list, 1, NULL);
    snprintf(out, sizeof out, "%s, the lookup %s", eai(rc),
             gai_error(&gcb) == EAI_INPROGRESS ? "still being made" : "answered");
}

/* Linux's futex: a word at 0, woken by a FUTEX_WAKE once it is 1. */
static unsigned int word;
static void futex_setup(void) { word = 0; }
static void futex_release(void)
{
    __atomic_store_n(&word, 1, __ATOMIC_SEQ_CST);
    syscall(SYS_futex, &word, FUTEX_WAKE_PRIVATE, 1, NULL, NULL, 0);
}
static void w_futex(void) { said(syscall(SYS_futex, &word, FUTEX_WAIT_PRIVATE, 0, NULL, NULL, 0)); }
static void w_futex_timed(void)
{
    struct timespec t = ten;
    said(syscall(SYS_futex, &word, FUTEX_WAIT_PRIVATE, 0, &t, NULL, 0));
}

/* The two POSIX says never answer EINTR, for comparison. */
static pthread_mutex_t mutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cond = PTHREAD_COND_INITIALIZER;
static int signalled;
static void cond_setup(void) { signalled = 0; }
static void cond_release(void)
{
    pthread_mutex_lock(&mutex);
    signalled = 1;
    pthread_cond_signal(&cond);
    pthread_mutex_unlock(&mutex);
}
static void w_cond_wait(void)
{
    pthread_mutex_lock(&mutex);
    int rc = pthread_cond_wait(&cond, &mutex);
    int was = signalled;
    pthread_mutex_unlock(&mutex);
    snprintf(out, sizeof out, "%d%s", rc, was ? "" : " unsignalled");
}
static void mutex_setup(void) { pthread_mutex_lock(&mutex); }
static void mutex_release(void) { pthread_mutex_unlock(&mutex); }
static void w_mutex_lock(void)
{
    int rc = pthread_mutex_lock(&mutex);
    if (rc == 0) pthread_mutex_unlock(&mutex);
    snprintf(out, sizeof out, "%d", rc);
}

typedef struct {
    const char *name;
    void (*setup)(void);
    void (*wait)(void);
    void (*release)(void);
    void (*teardown)(void);
} Case;

static const Case cases[] = {
    { "sem_wait", sem_setup, w_sem_wait, sem_release, sem_teardown },
    { "sem_timedwait", sem_setup, w_sem_timedwait, sem_release, sem_teardown },
    { "sem_clockwait", sem_setup, w_sem_clockwait, sem_release, sem_teardown },
    { "mq_receive", mq_setup, w_mq_receive, mq_feed, mq_teardown },
    { "mq_timedreceive", mq_setup, w_mq_timedreceive, mq_feed, mq_teardown },
    { "mq_send", mq_full_setup, w_mq_send, mq_drain, mq_teardown },
    { "mq_timedsend", mq_full_setup, w_mq_timedsend, mq_drain, mq_teardown },
    { "msgrcv", msg_setup, w_msgrcv, msg_feed, msg_teardown },
    { "msgsnd", msg_full_setup, w_msgsnd, msg_drain, msg_teardown },
    { "semop", semop_setup, w_semop, semop_release, semop_teardown },
    { "semtimedop", semop_setup, w_semtimedop, semop_release, semop_teardown },
    { "io_getevents", kaio_setup, w_io_getevents, kaio_release, kaio_teardown },
    { "io_getevents timed", kaio_setup, w_io_getevents_timed, kaio_release, kaio_teardown },
    { "aio_suspend", aio_setup, w_aio_suspend, aio_release, aio_teardown },
    { "aio_suspend timed", aio_setup, w_aio_suspend_timed, aio_release, aio_teardown },
    { "lio_listio", lio_setup, w_lio_listio, aio_release, aio_teardown },
    { "gai_suspend", gai_setup, w_gai_suspend, gai_release, gai_teardown },
    { "gai_suspend timed", gai_setup, w_gai_suspend_timed, gai_release, gai_teardown },
    { "getaddrinfo_a GAI_WAIT", gai_wait_setup, w_gai_wait, gai_release, gai_teardown },
    { "futex FUTEX_WAIT", futex_setup, w_futex, futex_release, nothing },
    { "futex FUTEX_WAIT timed", futex_setup, w_futex_timed, futex_release, nothing },
    { "pthread_cond_wait", cond_setup, w_cond_wait, cond_release, nothing },
    { "pthread_mutex_lock", mutex_setup, w_mutex_lock, mutex_release, nothing },
};

enum { HANDLER, RESTART, IGNORED, DEFAULT_IGNORED, ELSEWHERE, WAYS };
static const char *const ways[WAYS] = {
    "handler", "SA_RESTART handler", "SIG_IGN", "ignored by default", "handler on another thread",
};

static volatile sig_atomic_t handled;
static void on_signal(int sig) { (void)sig; handled++; }

static atomic_int started, finished;

static void *waiter(void *arg)
{
    const Case *c = arg;
    atomic_store(&started, 1);
    c->wait();
    atomic_store(&finished, 1);
    return NULL;
}

static void *bystander(void *arg)
{
    (void)arg;
    for (;;)
        pause();
    return NULL;
}

/* Long enough for a thread to be asleep in the kernel, and for a signal to
 * have been handled. */
static void settle(void)
{
    struct timespec t = { 0, 150 * 1000 * 1000 };
    while (nanosleep(&t, &t) == -1 && errno == EINTR)
        ;
}

/* One case, one way, in a process of its own: a call that ends early may
 * leave work in flight that is not safe to finish -- glibc's lio_listio
 * does, see the module docstring -- so a call that ended is not released,
 * and whatever it left dies with the process. */
static void probe(const Case *c, int way)
{
    alarm(20);
    pthread_t other;
    pthread_create(&other, NULL, bystander, NULL);
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sigemptyset(&sa.sa_mask);
    int sig = SIGUSR1;
    sa.sa_handler = on_signal;
    if (way == RESTART)
        sa.sa_flags = SA_RESTART;
    else if (way == IGNORED)
        sa.sa_handler = SIG_IGN;
    else if (way == DEFAULT_IGNORED) {
        sig = SIGURG;
        sa.sa_handler = SIG_DFL;
    }
    sigaction(sig, &sa, NULL);
    c->setup();
    pthread_t w;
    pthread_create(&w, NULL, waiter, (void *)c);
    while (!atomic_load(&started))
        sched_yield();
    settle();
    pthread_kill(way == ELSEWHERE ? other : w, sig);
    settle();
    if (atomic_load(&finished)) {
        printf("%s, %s: ends at once: %s (handler ran %d)\n", c->name, ways[way], out, (int)handled);
        _exit(0);
    }
    int ran = handled;
    c->release();
    pthread_join(w, NULL);
    printf("%s, %s: waits on, then: %s (handler ran %d)\n", c->name, ways[way], out, ran);
    c->teardown();
    _exit(0);
}

int main(void)
{
    setvbuf(stdout, NULL, _IONBF, 0);
    for (size_t i = 0; i < sizeof cases / sizeof *cases; i++)
        for (int way = 0; way < WAYS; way++) {
            pid_t pid = fork();
            if (pid < 0) { perror("fork"); return 1; }
            if (pid == 0)
                probe(&cases[i], way);
            int status;
            waitpid(pid, &status, 0);
            if (!WIFEXITED(status) || WEXITSTATUS(status) != 0)
                printf("%s, %s: the probe ended with status %#x\n", cases[i].name, ways[way], status);
        }
    printf("done\n");
    return 0;
}
'''


def main() -> None:
    with workdir() as t:
        d = Path(t)
        (d / "intr.c").write_text(PROGRAM, encoding="utf-8", newline="\n")
        w = wsl_path(d)
        # /etc/hosts is a FIFO: see the module docstring. The file the kernel
        # AIO read takes is anything with a byte in it.
        script = (
            f"set -e; cp {w}/intr.c /tmp/ && cd /tmp && "
            "{ gcc -static -O0 -Wall -Werror -o intrprobe intr.c -lpthread -lrt > gcc.log 2>&1 "
            "|| { cat gcc.log >&2; exit 1; }; }; "
            "printf 'hosts: files\\n' > /tmp/nsswitch.conf; "
            "printf 'x' > /tmp/interrupt-file; "
            "unshare -rmi sh -c 'mount -t tmpfs none /etc && cp /tmp/nsswitch.conf /etc/ && "
            "mkfifo /etc/hosts && timeout 300 /tmp/intrprobe'"
        )
        r = run(script)
        if r.returncode != 0 or "done" not in r.stdout:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-3000:]}")
        body = r.stdout
    head = ("# glibc 2.39 on Linux: what a signal does to a call blocked in the C library,\n"
            "# for posix/src/interrupt.rs. Generated by\n"
            "# posix/tools/oracle/interrupt_harness.py; do not edit.\n")
    OUT.write_text(head + body, encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {body.count(chr(10))} lines")


if __name__ == "__main__":
    main()
