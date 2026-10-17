/*
 * ctest-scm-rights -- ring-3 test of passing descriptors over a Unix socket
 * (SCM_RIGHTS).
 *
 * Until 2026-10-16 a native sendmsg with SCM_RIGHTS answered EOPNOTSUPP,
 * and a descriptor a Linux program sent a native one was dropped. Lane A's
 * kernel carries them (SYS_UNIX_SENDMSG, SYS_UNIX_RECVMSG;
 * requests/a-d-native-sendmsg-and-recvmsg-can-carry-descriptors.md), each
 * as its handle and type -- the descriptor numbers are the C library's --
 * and the library now sends and installs them. The host has no kernel to
 * carry one, so only here are they seen.
 *
 * A datagram socket pair carries most of them; check 8x has a stream pair
 * carry one, which the library's older stream pair could not.
 *
 * Every wait is for something already sent, or for a child that exits at
 * once, and an alarm ends the test after twenty seconds, so it cannot hang.
 * It passes on Linux 6.6 with glibc.
 *
 * Exit code 42 == every check passed. Anything else is the first failing
 * check and step: the tens digit names the check, the units the step. No
 * check is numbered 4, so that no failure reads as 42.
 *
 *   1x  a pipe's write end sent and received is a new descriptor (10) that
 *       writes into the same pipe (11), with no MSG_CTRUNC (12); 19: the
 *       socket pair or the pipe could not be made
 *   2x  two descriptors in one message arrive in order: a device opened
 *       read-only and the pipe's read end (20, 21)
 *   3x  room for one of two: one arrives, and MSG_CTRUNC says the other did
 *       not (30); no room at all: none, MSG_CTRUNC, and the bytes still come
 *       (31)
 *   5x  MSG_CMSG_CLOEXEC marks what arrives close-on-exec (50); without it,
 *       not (51)
 *   6x  a descriptor that is not open is EBADF (60); more than 253 in one
 *       message EINVAL (61)
 *   7x  across fork: a child writes into a pipe of its own, sends the
 *       parent the read end and exits, and the parent reads what the child
 *       wrote (70)
 *   8x  a stream socket pair carries one too (80)
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

static int sv[2];

/* Send `n` descriptors (and one byte) on sv[0]. */
static int send_fds(const int *fds, int n)
{
    char byte = 'x';
    struct iovec iov = {&byte, 1};
    union {
        char buf[CMSG_SPACE(sizeof(int) * 300)];
        struct cmsghdr align;
    } u;
    memset(&u, 0, sizeof u);
    struct msghdr msg;
    memset(&msg, 0, sizeof msg);
    msg.msg_iov = &iov;
    msg.msg_iovlen = 1;
    msg.msg_control = u.buf;
    msg.msg_controllen = CMSG_SPACE(sizeof(int) * n);
    struct cmsghdr *c = CMSG_FIRSTHDR(&msg);
    c->cmsg_level = SOL_SOCKET;
    c->cmsg_type = SCM_RIGHTS;
    c->cmsg_len = CMSG_LEN(sizeof(int) * n);
    memcpy(CMSG_DATA(c), fds, sizeof(int) * n);
    return sendmsg(sv[0], &msg, 0) == 1 ? 0 : -1;
}

/* Receive on sv[1] into room for `room` descriptors (0: no control buffer
 * at all); the descriptors that came go to `out`, their number is the
 * answer, and the message's flags go to `*flags`. -1 for a failed receive. */
static int recv_fds(int *out, int room, int recv_flags, int *flags)
{
    char byte;
    struct iovec iov = {&byte, 1};
    union {
        char buf[CMSG_SPACE(sizeof(int) * 8)];
        struct cmsghdr align;
    } u;
    struct msghdr msg;
    memset(&msg, 0, sizeof msg);
    msg.msg_iov = &iov;
    msg.msg_iovlen = 1;
    if (room > 0) {
        msg.msg_control = u.buf;
        /* CMSG_LEN, not CMSG_SPACE: Linux counts the room for descriptors
         * by the length, so CMSG_SPACE(4) would hold two. */
        msg.msg_controllen = CMSG_LEN(sizeof(int) * room);
    }
    if (recvmsg(sv[1], &msg, recv_flags) != 1)
        return -1;
    *flags = msg.msg_flags;
    int n = 0;
    for (struct cmsghdr *c = CMSG_FIRSTHDR(&msg); c; c = CMSG_NXTHDR(&msg, c)) {
        if (c->cmsg_level != SOL_SOCKET || c->cmsg_type != SCM_RIGHTS)
            continue;
        int k = (int)((c->cmsg_len - CMSG_LEN(0)) / sizeof(int));
        memcpy(out + n, CMSG_DATA(c), sizeof(int) * k);
        n += k;
    }
    return n;
}

int main(void)
{
    alarm(20);
    int p[2];
    if (socketpair(AF_UNIX, SOCK_DGRAM, 0, sv) != 0 || pipe(p) != 0)
        return 19;

    /* 1x */
    int got[8], flags = 0;
    if (send_fds(&p[1], 1) != 0)
        return 10;
    if (recv_fds(got, 4, 0, &flags) != 1 || got[0] == p[1] || got[0] < 0)
        return 10;
    char buf[16];
    if (write(got[0], "hi", 2) != 2 || read(p[0], buf, sizeof buf) != 2 || memcmp(buf, "hi", 2) != 0)
        return 11;
    if (flags & MSG_CTRUNC)
        return 12;
    close(got[0]);

    /* 2x */
    int dev = open("/dev/null", O_RDONLY);
    int two[2] = {dev, p[0]};
    if (dev < 0 || send_fds(two, 2) != 0 || recv_fds(got, 4, 0, &flags) != 2)
        return 20;
    struct stat a, b;
    if (fstat(got[0], &a) != 0 || !S_ISCHR(a.st_mode) || fstat(got[1], &b) != 0 || !S_ISFIFO(b.st_mode))
        return 21;
    if (write(p[1], "yo", 2) != 2 || read(got[1], buf, sizeof buf) != 2)
        return 21;
    close(got[0]);
    close(got[1]);

    /* 3x */
    if (send_fds(two, 2) != 0 || recv_fds(got, 1, 0, &flags) != 1 || !(flags & MSG_CTRUNC))
        return 30;
    close(got[0]);
    if (send_fds(two, 2) != 0 || recv_fds(got, 0, 0, &flags) != 0 || !(flags & MSG_CTRUNC))
        return 31;

    /* 5x */
    if (send_fds(&dev, 1) != 0 || recv_fds(got, 2, MSG_CMSG_CLOEXEC, &flags) != 1)
        return 50;
    if (fcntl(got[0], F_GETFD) != FD_CLOEXEC)
        return 50;
    close(got[0]);
    if (send_fds(&dev, 1) != 0 || recv_fds(got, 2, 0, &flags) != 1 || fcntl(got[0], F_GETFD) != 0)
        return 51;
    close(got[0]);

    /* 6x */
    int bad = 999;
    errno = 0;
    if (send_fds(&bad, 1) != -1 || errno != EBADF)
        return 60;
    int many[254];
    for (int i = 0; i < 254; i++)
        many[i] = dev;
    errno = 0;
    if (send_fds(many, 254) != -1 || errno != EINVAL)
        return 61;

    /* 7x */
    pid_t child = fork();
    if (child == 0) {
        int q[2];
        if (pipe(q) != 0 || write(q[1], "child", 5) != 5 || send_fds(&q[0], 1) != 0)
            _exit(1);
        _exit(0);
    }
    int status = 0;
    if (child < 0 || waitpid(child, &status, 0) != child || !WIFEXITED(status) || WEXITSTATUS(status) != 0)
        return 70;
    if (recv_fds(got, 2, 0, &flags) != 1 || read(got[0], buf, sizeof buf) != 5 || memcmp(buf, "child", 5) != 0)
        return 70;
    close(got[0]);

    /* 8x */
    close(sv[0]);
    close(sv[1]);
    if (socketpair(AF_UNIX, SOCK_STREAM, 0, sv) != 0)
        return 80;
    if (send_fds(&p[1], 1) != 0 || recv_fds(got, 2, 0, &flags) != 1)
        return 80;
    if (write(got[0], "st", 2) != 2 || read(p[0], buf, sizeof buf) != 2 || memcmp(buf, "st", 2) != 0)
        return 80;
    close(got[0]);
    return 42;
}
