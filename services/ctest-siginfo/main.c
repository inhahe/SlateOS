/*
 * ctest-siginfo -- is a SlateOS program told who sent it a signal, and what
 * the signal carries?
 *
 * An SA_SIGINFO handler, and sigwaitinfo, get a siginfo_t: si_code says how
 * the signal was sent (kill, sigqueue, tgkill, the kernel, a child's exit),
 * si_pid and si_uid who sent it, si_value what sigqueue attached, si_status
 * how a child ended. A SIGCHLD handler reaps the child si_pid names; a
 * daemon logs who sent it SIGTERM; a program passes a pointer in si_value.
 * Until lane A's extended signal frame, a native program was told SI_USER
 * and pid 0 for every signal that came from outside it, sigqueue answered
 * ENOSYS, and tgkill to another thread arrived as a plain kill
 * (known-issues/D-POSIX-SIGINFO-FROM-THE-KERNEL-IS-THE-NUMBER-ALONE.md).
 *
 * The checks, in order, each against Linux's answer (measured on 6.6):
 *
 *    1x  raise: SI_TKILL, this process's pid and uid
 *    2x  kill of itself: SI_USER, its pid
 *    3x  sigqueue to itself: SI_QUEUE, its pid, the value
 *    4x  tgkill of the calling thread: SI_TKILL
 *    5x  setitimer's SIGALRM: SI_KERNEL, pid 0
 *    6x  a child's kill (SI_USER, its pid), its sigqueue (SI_QUEUE, the
 *        value), and its exit (CLD_EXITED, its pid, status 7); then, while
 *        it is a zombie, kill, sigqueue and tgkill of it succeed
 *    7x  a child killed by SIGTERM: CLD_KILLED, status 15
 *    8x  sigwaitinfo takes a child's sigqueue: SI_QUEUE, its pid, the value
 *    9x  tgkill to a thread of another process: it is told SI_TKILL and
 *        this process's pid
 *    10x the refusals: sigqueue with pid 0 or -1 (ESRCH), a bad signal
 *        (EINVAL), no such process (ESRCH), pid 1 (EPERM); tgkill with an id
 *        not above zero (EINVAL), a bad signal (EINVAL), a thread not of the
 *        process named (ESRCH), pid 1's (EPERM)
 *
 * Exit codes: 42 is every check passed.
 *    2  fork, pipe or waitpid failed     Otherwise <check><step>; no step
 *    is 42.
 *
 * It must run as an ordinary user -- pid 1 is not its to signal. The same
 * source compiles and passes on Linux, which is how its expectations were
 * checked to be Linux's.
 */

#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <string.h>
#include <sys/time.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

/* A pid nothing has: above Linux's largest pid_max, 4194304. */
#define NO_SUCH_PID 999999999

static void emit(const char *s)
{
    size_t n = strlen(s);
    if (n != 0) {
        ssize_t written = write(1, s, n);
        (void)written;
    }
}

static void emit_num(long n)
{
    char buf[24];
    int i = (int)sizeof buf;
    int neg = n < 0;
    unsigned long v = neg ? 0UL - (unsigned long)n : (unsigned long)n;
    buf[--i] = '\0';
    do {
        buf[--i] = (char)('0' + v % 10);
        v /= 10;
    } while (v != 0 && i > 1);
    if (neg) {
        buf[--i] = '-';
    }
    emit(buf + i);
}

static int fail(int code, const char *what)
{
    int e = errno;
    emit("ctest-siginfo: FAIL ");
    emit_num(code);
    emit(": ");
    emit(what);
    emit(" (errno ");
    emit_num(e);
    emit(")\n");
    return code;
}

static void ok(const char *what)
{
    emit("ctest-siginfo: ok ");
    emit(what);
    emit("\n");
}

/* What the handler was told, in the order it was told it. */
#define RECORDS 16
static siginfo_t records[RECORDS];
static volatile sig_atomic_t count;

static void record(int sig, siginfo_t *si, void *uc)
{
    (void)sig;
    (void)uc;
    if (count < RECORDS) {
        records[count] = *si;
        count = count + 1;
    }
}

static void nap(long ms)
{
    struct timespec t = { ms / 1000, (ms % 1000) * 1000000L };
    nanosleep(&t, NULL);
}

/* Wait up to five seconds for at least `want` records. */
static int wait_for(int want)
{
    for (int i = 0; i < 1000 && count < want; i++) {
        nap(5);
    }
    return count >= want;
}

/* The first record of signal `sig`, or NULL. */
static const siginfo_t *find(int sig)
{
    for (int i = 0; i < count; i++) {
        if (records[i].si_signo == sig) {
            return &records[i];
        }
    }
    return NULL;
}

static void show(const char *what, const siginfo_t *si)
{
    emit("ctest-siginfo: ");
    emit(what);
    if (si == NULL) {
        emit(": none\n");
        return;
    }
    emit(": signo ");
    emit_num(si->si_signo);
    emit(" code ");
    emit_num(si->si_code);
    emit(" pid ");
    emit_num(si->si_pid);
    emit(" uid ");
    emit_num((long)si->si_uid);
    emit(" value ");
    emit_num(si->si_value.sival_int);
    emit("\n");
}

/* `si` is signal `sig` with code `code` from `pid` (and, unless `uid` is
 * -1, real uid `uid`). */
static int is(const siginfo_t *si, int sig, int code, pid_t pid, long uid)
{
    return si != NULL && si->si_signo == sig && si->si_code == code && si->si_pid == pid
           && (uid < 0 || (long)si->si_uid == uid);
}

static int expect_errno(int ret, int want)
{
    return ret == -1 && errno == want;
}

/* Check 9's child: told of a tgkill from its parent. 0, or the step. */
static int tgkill_child(int ready_fd)
{
    count = 0;
    char c = 'r';
    if (write(ready_fd, &c, 1) != 1) {
        return 2;
    }
    if (!wait_for(1)) {
        return 91;
    }
    const siginfo_t *si = find(SIGUSR1);
    if (!is(si, SIGUSR1, SI_TKILL, getppid(), (long)getuid())) {
        show("the child was told", si);
        return 92;
    }
    return 0;
}

int main(void)
{
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = record;
    sa.sa_flags = SA_SIGINFO;
    sigemptyset(&sa.sa_mask);
    if (sigaction(SIGUSR1, &sa, NULL) != 0 || sigaction(SIGUSR2, &sa, NULL) != 0
        || sigaction(SIGALRM, &sa, NULL) != 0 || sigaction(SIGCHLD, &sa, NULL) != 0) {
        return fail(10, "sigaction with SA_SIGINFO");
    }
    pid_t me = getpid();
    long uid = (long)getuid();

    /* 1. */
    count = 0;
    raise(SIGUSR1);
    if (!wait_for(1) || !is(find(SIGUSR1), SIGUSR1, SI_TKILL, me, uid)) {
        show("raise", find(SIGUSR1));
        return fail(11, "raise is SI_TKILL from this process");
    }
    ok("raise is SI_TKILL from this process");

    /* 2. */
    count = 0;
    kill(me, SIGUSR1);
    if (!wait_for(1) || !is(find(SIGUSR1), SIGUSR1, SI_USER, me, uid)) {
        show("kill", find(SIGUSR1));
        return fail(21, "kill of itself is SI_USER from this process");
    }
    ok("kill of itself is SI_USER from this process");

    /* 3. */
    count = 0;
    union sigval v;
    memset(&v, 0, sizeof v);
    v.sival_int = 1234;
    if (sigqueue(me, SIGUSR1, v) != 0) {
        return fail(31, "sigqueue to itself");
    }
    if (!wait_for(1) || !is(find(SIGUSR1), SIGUSR1, SI_QUEUE, me, uid)
        || find(SIGUSR1)->si_value.sival_int != 1234) {
        show("sigqueue", find(SIGUSR1));
        return fail(32, "sigqueue is SI_QUEUE from this process, with the value");
    }
    ok("sigqueue to itself is SI_QUEUE with its value");

    /* 4. */
    count = 0;
    if (tgkill(me, gettid(), SIGUSR1) != 0) {
        return fail(41, "tgkill of the calling thread");
    }
    if (!wait_for(1) || !is(find(SIGUSR1), SIGUSR1, SI_TKILL, me, uid)) {
        show("tgkill", find(SIGUSR1));
        return fail(43, "tgkill of the calling thread is SI_TKILL");
    }
    ok("tgkill of the calling thread is SI_TKILL");

    /* 5. */
    count = 0;
    struct itimerval it;
    memset(&it, 0, sizeof it);
    it.it_value.tv_usec = 20000;
    if (setitimer(ITIMER_REAL, &it, NULL) != 0) {
        return fail(51, "setitimer");
    }
    if (!wait_for(1) || !is(find(SIGALRM), SIGALRM, SI_KERNEL, 0, -1)) {
        show("setitimer", find(SIGALRM));
        return fail(52, "setitimer's SIGALRM is SI_KERNEL, from no process");
    }
    ok("setitimer's SIGALRM is SI_KERNEL");

    /* 6. */
    count = 0;
    pid_t child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        union sigval w;
        memset(&w, 0, sizeof w);
        w.sival_int = 99;
        kill(getppid(), SIGUSR2);
        nap(20);
        sigqueue(getppid(), SIGUSR1, w);
        nap(20);
        _exit(7);
    }
    if (!wait_for(3)) {
        emit("ctest-siginfo: records from the child: ");
        emit_num(count);
        emit("\n");
        return fail(61, "a child's kill, sigqueue and exit all arrive");
    }
    if (!is(find(SIGUSR2), SIGUSR2, SI_USER, child, uid)) {
        show("the child's kill", find(SIGUSR2));
        return fail(62, "a child's kill is SI_USER from the child");
    }
    if (!is(find(SIGUSR1), SIGUSR1, SI_QUEUE, child, uid) || find(SIGUSR1)->si_value.sival_int != 99) {
        show("the child's sigqueue", find(SIGUSR1));
        return fail(63, "a child's sigqueue is SI_QUEUE from the child, with the value");
    }
    if (!is(find(SIGCHLD), SIGCHLD, CLD_EXITED, child, -1) || find(SIGCHLD)->si_status != 7) {
        show("the child's exit", find(SIGCHLD));
        return fail(64, "a child's exit is CLD_EXITED with its status");
    }
    /* Exited and not reaped: a zombie, which Linux lets anyone signal. */
    union sigval z;
    memset(&z, 0, sizeof z);
    if (kill(child, SIGUSR1) != 0) {
        return fail(65, "kill of a zombie child succeeds");
    }
    if (sigqueue(child, SIGUSR1, z) != 0) {
        return fail(66, "sigqueue of a zombie child succeeds");
    }
    if (tgkill(child, child, SIGUSR1) != 0) {
        return fail(67, "tgkill of a zombie child succeeds");
    }
    errno = 0;
    if (!expect_errno(sigqueue(child, 65, z), EINVAL)) {
        return fail(68, "sigqueue of a zombie with a bad signal is EINVAL");
    }
    int status = 0;
    if (waitpid(child, &status, 0) != child) {
        return fail(2, "waitpid");
    }
    ok("a child's kill, sigqueue and exit say who and what; a zombie may be signalled");

    /* 7. */
    count = 0;
    child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        for (;;) {
            pause();
        }
    }
    nap(50);
    if (kill(child, SIGTERM) != 0) {
        return fail(71, "kill(child, SIGTERM)");
    }
    if (!wait_for(1) || !is(find(SIGCHLD), SIGCHLD, CLD_KILLED, child, -1)
        || find(SIGCHLD)->si_status != SIGTERM) {
        show("the killed child", find(SIGCHLD));
        return fail(72, "a child killed by SIGTERM is CLD_KILLED, status SIGTERM");
    }
    if (waitpid(child, &status, 0) != child) {
        return fail(2, "waitpid");
    }
    ok("a child killed by SIGTERM is CLD_KILLED with the signal");

    /* 8. */
    sigset_t usr1;
    sigemptyset(&usr1);
    sigaddset(&usr1, SIGUSR1);
    if (sigprocmask(SIG_BLOCK, &usr1, NULL) != 0) {
        return fail(81, "sigprocmask");
    }
    child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        union sigval w;
        memset(&w, 0, sizeof w);
        w.sival_int = 4321;
        sigqueue(getppid(), SIGUSR1, w);
        _exit(0);
    }
    siginfo_t taken;
    memset(&taken, 0, sizeof taken);
    if (sigwaitinfo(&usr1, &taken) != SIGUSR1) {
        return fail(82, "sigwaitinfo takes the child's SIGUSR1");
    }
    if (!is(&taken, SIGUSR1, SI_QUEUE, child, uid) || taken.si_value.sival_int != 4321) {
        show("sigwaitinfo", &taken);
        return fail(83, "sigwaitinfo is told SI_QUEUE, the child and the value");
    }
    if (sigprocmask(SIG_UNBLOCK, &usr1, NULL) != 0) {
        return fail(81, "sigprocmask");
    }
    if (waitpid(child, &status, 0) != child) {
        return fail(2, "waitpid");
    }
    ok("sigwaitinfo is told what a handler would be");

    /* 9. */
    int fds[2];
    if (pipe(fds) != 0) {
        return fail(2, "pipe");
    }
    child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        close(fds[0]);
        _exit(tgkill_child(fds[1]));
    }
    close(fds[1]);
    char c;
    if (read(fds[0], &c, 1) != 1) {
        return fail(2, "the child's readiness");
    }
    close(fds[0]);
    if (tgkill(child, child, SIGUSR1) != 0) {
        return fail(93, "tgkill to another process's thread");
    }
    if (waitpid(child, &status, 0) != child || !WIFEXITED(status)) {
        return fail(2, "waitpid");
    }
    if (WEXITSTATUS(status) != 0) {
        emit("ctest-siginfo: the tgkill child failed at its step ");
        emit_num(WEXITSTATUS(status));
        emit("\n");
        return fail(94, "a thread of another process is told SI_TKILL from this one");
    }
    ok("tgkill to another process's thread is SI_TKILL from this one");

    /* 10. */
    union sigval nothing;
    memset(&nothing, 0, sizeof nothing);
    errno = 0;
    if (!expect_errno(sigqueue(0, SIGUSR1, nothing), ESRCH)) {
        return fail(101, "sigqueue to pid 0 is ESRCH");
    }
    errno = 0;
    if (!expect_errno(sigqueue(-1, SIGUSR1, nothing), ESRCH)) {
        return fail(102, "sigqueue to pid -1 is ESRCH");
    }
    errno = 0;
    if (!expect_errno(sigqueue(me, 65, nothing), EINVAL)) {
        return fail(103, "sigqueue of signal 65 is EINVAL");
    }
    errno = 0;
    if (!expect_errno(sigqueue(NO_SUCH_PID, 0, nothing), ESRCH)) {
        return fail(104, "sigqueue to no such process is ESRCH");
    }
    errno = 0;
    if (!expect_errno(sigqueue(1, 0, nothing), EPERM)) {
        return fail(105, "sigqueue to pid 1 is EPERM");
    }
    errno = 0;
    if (!expect_errno(tgkill(0, me, 0), EINVAL) || !expect_errno(tgkill(me, 0, 0), EINVAL)) {
        return fail(106, "tgkill with an id not above zero is EINVAL");
    }
    errno = 0;
    if (!expect_errno(tgkill(me, gettid(), 65), EINVAL)) {
        return fail(107, "tgkill of signal 65 is EINVAL");
    }
    errno = 0;
    if (!expect_errno(tgkill(1, me, 0), ESRCH)) {
        return fail(108, "tgkill of a thread not of the process named is ESRCH");
    }
    errno = 0;
    if (!expect_errno(tgkill(1, 1, 0), EPERM)) {
        return fail(109, "tgkill of pid 1's thread is EPERM");
    }
    ok("sigqueue and tgkill refuse as Linux does");

    emit("ctest-siginfo: every check passed\n");
    return 42;
}
