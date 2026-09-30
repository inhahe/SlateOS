/*
 * ctest-eintr -- ring-3 test that a signal ends a call waiting inside the C
 * library as it does on Linux: a handler without SA_RESTART ends it with
 * EINTR; one with SA_RESTART ends only the calls Linux never restarts; a
 * signal that runs no handler ends none.
 *
 * Guards known-issues.md -> D-POSIX-FUTEX-WAITS-DISCARD-EINTR and
 * design-decisions.md §1156.
 *
 * ## Why a fixture and not only the unit tests
 *
 * posix/src/interrupt.rs replays glibc's answers for every such call on the
 * host, with a scripted stand-in for the kernel: it says what a futex wait
 * met, and the trampoline's dispatch runs as it would.  Only here does the
 * kernel end a real futex wait for a real signal from another process,
 * convert its restart sentinel into KernelError::Interrupted for the
 * trampoline's frame, and the wait then read the handler counts the
 * dispatch left in this thread's block.
 *
 * ## The shape of every check
 *
 * The process that waits is single-threaded.  Signals here are
 * process-directed (see pthread_kill's note in posix/src/pthread.rs), so with
 * a second thread the kernel could hand the signal to it instead, and the
 * check would test that thread's luck.  A child it forks sends the signals,
 * 200 ms apart: the first -- the one under test -- then, where the check
 * needs one, SIGALRM, whose handler has no SA_RESTART and so ends any of these
 * waits.  A call that SIGALRM ends has waited through the first signal.
 *
 * ## It cannot hang
 *
 * A wait that nothing ends is the failure this fixture exists to catch, so
 * every wait is bounded from outside: the child SIGKILLs the waiting process
 * three seconds after its last signal unless it has been killed itself by
 * then, which the parent does as soon as its wait is over.  A wrong answer
 * is therefore a wrong exit code, or death by SIGKILL -- never a boot test
 * held up.  (ctest-pty once blocked one for two hours on a read that could
 * not return.)
 *
 * Exit code 42 == every check passed.  Anything else is the first failing
 * check and step: the tens digit names the check, the units the step.
 *
 *   x0  its setup failed            x1  fork failed
 *   x2  the call's answer, or errno, is not the one expected
 *   x3  it ended at the wrong signal (the handler counts say which ran)
 *   x4  the object does not work after the interruption
 *
 *   1x  sem_wait, a handler without SA_RESTART    -- EINTR at once
 *   2x  sem_wait, a handler with SA_RESTART       -- waits on, EINTR at SIGALRM
 *   3x  sem_wait, SIGUSR1 set to SIG_IGN          -- waits on, EINTR at SIGALRM
 *   4x  sem_wait, SIGURG (ignored by default)     -- waits on, EINTR at SIGALRM
 *   5x  sem_timedwait, a handler with SA_RESTART  -- EINTR at once
 *   6x  mq_receive, a handler with SA_RESTART     -- waits on, EINTR at SIGALRM
 *   7x  msgrcv, a handler with SA_RESTART         -- EINTR at once
 *   8x  mq_receive, a handler without SA_RESTART  -- EINTR at once
 *
 * and the calls the kernel itself sleeps in, or the library's loops around
 * its non-blocking ones:
 *
 *   9x  read on a pipe, a handler with SA_RESTART -- waits on, EINTR at SIGALRM
 *  10x  read on a pipe, a child's exit (SIGCHLD) and SIGURG, both ignored by
 *       default                                   -- waits on, EINTR at SIGALRM
 *  11x  nanosleep, a handler with SA_RESTART      -- EINTR at once, ~4.8 s
 *       left (x4: not 4 whole seconds left)
 *  12x  poll, a handler with SA_RESTART           -- EINTR at once
 *  13x  waitpid, a handler with SA_RESTART        -- waits on, EINTR at SIGALRM
 *  14x  pause, SIGUSR1 set to SIG_IGN             -- waits on, EINTR at SIGALRM
 *
 * and taking a signal:
 *
 *  15x  sigwait for SIGUSR2, blocked              -- takes it, no handler run
 *       (x3: its handler ran instead; x4: SIGUSR2 not blocked afterwards)
 *  16x  sigtimedwait, a SIGUSR1 handler with SA_RESTART -- EINTR at once
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <mqueue.h>
#include <poll.h>
#include <semaphore.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/ipc.h>
#include <sys/msg.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static volatile sig_atomic_t usr1_runs;
static volatile sig_atomic_t alrm_runs;

static void on_usr1(int sig)
{
    (void)sig;
    usr1_runs++;
}

static void on_alrm(int sig)
{
    (void)sig;
    alrm_runs++;
}

static void sleep_ms(long ms)
{
    struct timespec t = { ms / 1000, (ms % 1000) * 1000000L };
    while (nanosleep(&t, &t) == -1 && errno == EINTR)
        ;
}

static int install(int sig, void (*fn)(int), int flags)
{
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = fn;
    sa.sa_flags = flags;
    sigemptyset(&sa.sa_mask);
    return sigaction(sig, &sa, NULL);
}

/* SIGUSR1's disposition for a check, SIGALRM's handler, and the counts
 * cleared. */
static int arrange(void (*usr1)(int), int usr1_flags)
{
    usr1_runs = 0;
    alrm_runs = 0;
    if (install(SIGUSR1, usr1, usr1_flags) != 0)
        return -1;
    return install(SIGALRM, on_alrm, 0);
}

/* A child that sends this process `first` after 200 ms, then SIGALRM 200 ms
 * later when `then_alarm`; and SIGKILL three seconds after that, unless it
 * has been killed first.  Its pid, or -1. */
static pid_t signaller(int first, int then_alarm)
{
    pid_t parent = getpid();
    pid_t pid = fork();
    if (pid == 0) {
        sleep_ms(200);
        kill(parent, first);
        if (then_alarm) {
            sleep_ms(200);
            kill(parent, SIGALRM);
        }
        sleep_ms(3000);
        kill(parent, SIGKILL);
        _exit(0);
    }
    return pid;
}

/* The wait is over: the child is not needed, and must not send the SIGKILL. */
static void done_with(pid_t child)
{
    kill(child, SIGKILL);
    waitpid(child, NULL, 0);
}

static void defaults(void)
{
    install(SIGUSR1, SIG_DFL, 0);
    install(SIGALRM, SIG_DFL, 0);
    install(SIGURG, SIG_DFL, 0);
}

/* sem_wait on an empty semaphore, signalled as `first` then (optionally)
 * SIGALRM: its answer, errno, and the handler counts at the moment it
 * returned.  `base` is the check's tens. */
static int sem_wait_case(int base, int first, int then_alarm, int want_usr1, int want_alrm)
{
    sem_t s;
    if (sem_init(&s, 0, 0) != 0)
        return base + 0;
    pid_t child = signaller(first, then_alarm);
    if (child < 0)
        return base + 1;
    int rc = sem_wait(&s);
    int e = errno;
    int usr1 = usr1_runs, alrm = alrm_runs;
    done_with(child);
    defaults();
    if (rc != -1 || e != EINTR)
        return base + 2;
    if (usr1 != want_usr1 || alrm != want_alrm)
        return base + 3;
    /* Still a semaphore: a post is taken at once. */
    if (sem_post(&s) != 0 || sem_wait(&s) != 0)
        return base + 4;
    sem_destroy(&s);
    return 0;
}

static int check_sem_wait_handler(void)
{
    if (arrange(on_usr1, 0) != 0)
        return 10;
    /* SIGALRM too, so that a SIGUSR1 which came before the wait began shows
     * as the wrong ending (13) rather than as a SIGKILL. */
    return sem_wait_case(10, SIGUSR1, 1, 1, 0);
}

static int check_sem_wait_restart(void)
{
    if (arrange(on_usr1, SA_RESTART) != 0)
        return 20;
    return sem_wait_case(20, SIGUSR1, 1, 1, 1);
}

static int check_sem_wait_ignored(void)
{
    if (arrange(SIG_IGN, 0) != 0)
        return 30;
    return sem_wait_case(30, SIGUSR1, 1, 0, 1);
}

static int check_sem_wait_ignored_by_default(void)
{
    if (arrange(on_usr1, 0) != 0 || install(SIGURG, SIG_DFL, 0) != 0)
        return 40;
    return sem_wait_case(40, SIGURG, 1, 0, 1);
}

static int check_sem_timedwait_restart(void)
{
    if (arrange(on_usr1, SA_RESTART) != 0)
        return 50;
    sem_t s;
    if (sem_init(&s, 0, 0) != 0)
        return 50;
    struct timespec at;
    clock_gettime(CLOCK_REALTIME, &at);
    at.tv_sec += 10;
    pid_t child = signaller(SIGUSR1, 1);
    if (child < 0)
        return 51;
    int rc = sem_timedwait(&s, &at);
    int e = errno;
    int usr1 = usr1_runs, alrm = alrm_runs;
    done_with(child);
    defaults();
    if (rc != -1 || e != EINTR)
        return 52;
    if (usr1 != 1 || alrm != 0)
        return 53;
    if (sem_post(&s) != 0 || sem_timedwait(&s, &at) != 0)
        return 54;
    sem_destroy(&s);
    return 0;
}

/* mq_receive on an empty queue, SIGUSR1's handler with `flags`. */
static int mq_case(int base, int flags, int then_alarm, int want_alrm)
{
    if (arrange(on_usr1, flags) != 0)
        return base + 0;
    struct mq_attr a;
    memset(&a, 0, sizeof a);
    a.mq_maxmsg = 1;
    a.mq_msgsize = 8;
    mq_unlink("/ctest-eintr");
    mqd_t q = mq_open("/ctest-eintr", O_CREAT | O_RDWR, 0600, &a);
    if (q == (mqd_t)-1)
        return base + 0;
    pid_t child = signaller(SIGUSR1, then_alarm);
    if (child < 0)
        return base + 1;
    char buf[8];
    ssize_t rc = mq_receive(q, buf, sizeof buf, NULL);
    int e = errno;
    int usr1 = usr1_runs, alrm = alrm_runs;
    done_with(child);
    defaults();
    int err = 0;
    if (rc != -1 || e != EINTR)
        err = base + 2;
    else if (usr1 != 1 || alrm != want_alrm)
        err = base + 3;
    else if (mq_send(q, "x", 1, 0) != 0 || mq_receive(q, buf, sizeof buf, NULL) != 1)
        err = base + 4;
    mq_close(q);
    mq_unlink("/ctest-eintr");
    return err;
}

static int check_mq_receive_restart(void)
{
    return mq_case(60, SA_RESTART, 1, 1);
}

static int check_mq_receive_handler(void)
{
    return mq_case(80, 0, 1, 0);
}

/* ---- The calls the kernel itself sleeps in, and the library's loops ---- */

/* read on an empty pipe: `first`'s handler, then SIGALRM; what it answered
 * and the counts when it did. */
static int pipe_read_case(int base, int first, int then_alarm, int want_usr1, int want_alrm)
{
    int fds[2];
    if (pipe(fds) != 0)
        return base + 0;
    pid_t child = signaller(first, then_alarm);
    if (child < 0)
        return base + 1;
    char c;
    ssize_t rc = read(fds[0], &c, 1);
    int e = errno;
    int usr1 = usr1_runs, alrm = alrm_runs;
    done_with(child);
    defaults();
    int err = 0;
    if (rc != -1 || e != EINTR)
        err = base + 2;
    else if (usr1 != want_usr1 || alrm != want_alrm)
        err = base + 3;
    else if (write(fds[1], "x", 1) != 1 || read(fds[0], &c, 1) != 1)
        err = base + 4;
    close(fds[0]);
    close(fds[1]);
    return err;
}

static int check_pipe_read_restart(void)
{
    if (arrange(on_usr1, SA_RESTART) != 0)
        return 90;
    return pipe_read_case(90, SIGUSR1, 1, 1, 1);
}

/* A child's exit sends SIGCHLD, whose default is to be ignored: it must not
 * end a read.  One child exits at 200 ms; the signaller's SIGALRM, 400 ms
 * after its SIGURG (itself ignored by default), ends the read. */
static int check_pipe_read_sigchld(void)
{
    if (arrange(on_usr1, 0) != 0)
        return 100;
    int fds[2];
    if (pipe(fds) != 0)
        return 100;
    pid_t quick = fork();
    if (quick == 0) {
        sleep_ms(200);
        _exit(0);
    }
    if (quick < 0)
        return 101;
    pid_t child = signaller(SIGURG, 1);
    if (child < 0)
        return 101;
    char c;
    ssize_t rc = read(fds[0], &c, 1);
    int e = errno;
    int alrm = alrm_runs;
    done_with(child);
    waitpid(quick, NULL, 0);
    defaults();
    close(fds[0]);
    close(fds[1]);
    if (rc != -1 || e != EINTR)
        return 102;
    return alrm == 1 ? 0 : 103;
}

/* nanosleep ends for any handler, with what was left. */
static int check_nanosleep_restart(void)
{
    if (arrange(on_usr1, SA_RESTART) != 0)
        return 110;
    pid_t child = signaller(SIGUSR1, 1);
    if (child < 0)
        return 111;
    struct timespec t = { 5, 0 }, rem = { -1, -1 };
    int rc = nanosleep(&t, &rem);
    int e = errno;
    int usr1 = usr1_runs, alrm = alrm_runs;
    done_with(child);
    defaults();
    if (rc != -1 || e != EINTR)
        return 112;
    if (usr1 != 1 || alrm != 0)
        return 113;
    /* About 4.8 s were left: the signal came at 0.2. */
    return rem.tv_sec == 4 ? 0 : 114;
}

/* poll ends for any handler. */
static int check_poll_restart(void)
{
    if (arrange(on_usr1, SA_RESTART) != 0)
        return 120;
    int fds[2];
    if (pipe(fds) != 0)
        return 120;
    pid_t child = signaller(SIGUSR1, 1);
    if (child < 0)
        return 121;
    struct pollfd p = { fds[0], POLLIN, 0 };
    int rc = poll(&p, 1, 5000);
    int e = errno;
    int usr1 = usr1_runs, alrm = alrm_runs;
    done_with(child);
    defaults();
    close(fds[0]);
    close(fds[1]);
    if (rc != -1 || e != EINTR)
        return 122;
    return usr1 == 1 && alrm == 0 ? 0 : 123;
}

/* waitpid for the signaller itself: waits on through an SA_RESTART
 * handler, and ends at SIGALRM. */
static int check_waitpid_restart(void)
{
    if (arrange(on_usr1, SA_RESTART) != 0)
        return 130;
    pid_t child = signaller(SIGUSR1, 1);
    if (child < 0)
        return 131;
    pid_t rc = waitpid(child, NULL, 0);
    int e = errno;
    int usr1 = usr1_runs, alrm = alrm_runs;
    done_with(child);
    defaults();
    if (rc != -1 || e != EINTR)
        return 132;
    return usr1 == 1 && alrm == 1 ? 0 : 133;
}

/* pause waits for a handler: an ignored signal leaves it waiting. */
static int check_pause_ignored(void)
{
    if (arrange(SIG_IGN, 0) != 0)
        return 140;
    pid_t child = signaller(SIGUSR1, 1);
    if (child < 0)
        return 141;
    int rc = pause();
    int e = errno;
    int alrm = alrm_runs;
    done_with(child);
    defaults();
    if (rc != -1 || e != EINTR)
        return 142;
    return alrm == 1 ? 0 : 143;
}

/* ---- Taking a signal: sigwait and sigtimedwait ---- */

static volatile sig_atomic_t usr2_runs;

static void on_usr2(int sig)
{
    (void)sig;
    usr2_runs++;
}

/* sigwait takes a blocked SIGUSR2 the child sends: no handler runs, and
 * the signal is blocked again afterwards. */
static int check_sigwait(void)
{
    if (arrange(on_usr1, 0) != 0 || install(SIGUSR2, on_usr2, 0) != 0)
        return 150;
    usr2_runs = 0;
    sigset_t set, old, now;
    sigemptyset(&set);
    sigaddset(&set, SIGUSR2);
    if (sigprocmask(SIG_BLOCK, &set, &old) != 0)
        return 150;
    pid_t child = signaller(SIGUSR2, 0);
    if (child < 0)
        return 151;
    int sig = 0;
    int rc = sigwait(&set, &sig);
    done_with(child);
    sigprocmask(SIG_BLOCK, NULL, &now);
    int still = sigismember(&now, SIGUSR2);
    sigprocmask(SIG_SETMASK, &old, NULL);
    defaults();
    install(SIGUSR2, SIG_DFL, 0);
    if (rc != 0 || sig != SIGUSR2)
        return 152;
    if (usr2_runs != 0)
        return 153;
    return still == 1 ? 0 : 154;
}

/* sigtimedwait ends for a handler of another signal, SA_RESTART or not. */
static int check_sigtimedwait_restart(void)
{
    if (arrange(on_usr1, SA_RESTART) != 0)
        return 160;
    sigset_t set, old;
    sigemptyset(&set);
    sigaddset(&set, SIGUSR2);
    if (sigprocmask(SIG_BLOCK, &set, &old) != 0)
        return 160;
    pid_t child = signaller(SIGUSR1, 1);
    if (child < 0)
        return 161;
    struct timespec t = { 5, 0 };
    int rc = sigtimedwait(&set, NULL, &t);
    int e = errno;
    int usr1 = usr1_runs, alrm = alrm_runs;
    done_with(child);
    sigprocmask(SIG_SETMASK, &old, NULL);
    defaults();
    if (rc != -1 || e != EINTR)
        return 162;
    return usr1 == 1 && alrm == 0 ? 0 : 163;
}

static int check_msgrcv_restart(void)
{
    if (arrange(on_usr1, SA_RESTART) != 0)
        return 70;
    int id = msgget(IPC_PRIVATE, IPC_CREAT | 0600);
    if (id < 0)
        return 70;
    pid_t child = signaller(SIGUSR1, 1);
    if (child < 0)
        return 71;
    struct { long type; char text[8]; } m;
    ssize_t rc = msgrcv(id, &m, sizeof m.text, 0, 0);
    int e = errno;
    int usr1 = usr1_runs, alrm = alrm_runs;
    done_with(child);
    defaults();
    int err = 0;
    if (rc != -1 || e != EINTR)
        err = 72;
    else if (usr1 != 1 || alrm != 0)
        err = 73;
    else {
        m.type = 1;
        m.text[0] = 'x';
        if (msgsnd(id, &m, 1, 0) != 0 || msgrcv(id, &m, sizeof m.text, 0, 0) != 1)
            err = 74;
    }
    msgctl(id, IPC_RMID, NULL);
    return err;
}

int main(void)
{
    static const struct {
        const char *name;
        int (*run)(void);
    } checks[] = {
        {"sem_wait, a handler without SA_RESTART", check_sem_wait_handler},
        {"sem_wait, a handler with SA_RESTART", check_sem_wait_restart},
        {"sem_wait, an ignored signal", check_sem_wait_ignored},
        {"sem_wait, a signal ignored by default", check_sem_wait_ignored_by_default},
        {"sem_timedwait, a handler with SA_RESTART", check_sem_timedwait_restart},
        {"mq_receive, a handler with SA_RESTART", check_mq_receive_restart},
        {"msgrcv, a handler with SA_RESTART", check_msgrcv_restart},
        {"mq_receive, a handler without SA_RESTART", check_mq_receive_handler},
        {"read on a pipe, a handler with SA_RESTART", check_pipe_read_restart},
        {"read on a pipe, a child's SIGCHLD", check_pipe_read_sigchld},
        {"nanosleep, a handler with SA_RESTART", check_nanosleep_restart},
        {"poll, a handler with SA_RESTART", check_poll_restart},
        {"waitpid, a handler with SA_RESTART", check_waitpid_restart},
        {"pause, an ignored signal", check_pause_ignored},
        {"sigwait, a blocked signal", check_sigwait},
        {"sigtimedwait, a handler with SA_RESTART", check_sigtimedwait_restart},
    };
    setvbuf(stdout, NULL, _IONBF, 0);
    for (unsigned i = 0; i < sizeof checks / sizeof checks[0]; i++) {
        int r = checks[i].run();
        if (r != 0) {
            printf("[eintr] FAIL %s: %d\n", checks[i].name, r);
            return r;
        }
        printf("[eintr] ok %s\n", checks[i].name);
    }
    return 42;
}
