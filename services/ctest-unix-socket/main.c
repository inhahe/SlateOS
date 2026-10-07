/*
 * ctest-unix-socket -- do the C library's Unix-domain sockets that have names
 * work on SlateOS?
 *
 * posix/src/unix_socket.rs builds socket(AF_UNIX, ...) on the kernel's
 * SYS_UNIX_* calls (lane A's ipc::unix_socket). Its host tests check the
 * library's own arithmetic -- names, addresses, timeouts, credentials -- and
 * none of it has met the kernel. This does: a server and a client in two
 * processes, datagrams with names, a sequenced-packet pair, the options.
 *
 * The checks, in order; each is a step a later one leans on:
 *
 *    1x  a stream server: socket, bind to a path in /tmp, listen;
 *        getsockname gives the path back
 *    2x  a client, in a child: connect, send "ping"; the server accepts,
 *        reads it, and sees the peer unnamed (getpeername) and the child's
 *        pid, uid and gid (SO_PEERCRED); its "pong" reaches the child, and
 *        the child's close is end of file
 *    3x  the error answers: a second bind to the path is EADDRINUSE, a
 *        connect to a path with nothing there ENOENT or ECONNREFUSED, a
 *        stream sendto naming an address EISCONN, a non-blocking accept with
 *        nothing waiting EAGAIN
 *    5x  datagrams by abstract name: one socket bound to "\0ctest-unix-dgram",
 *        another sends it two; recvfrom names the sender (bound, so the
 *        name comes back), and a short buffer with MSG_TRUNC is told the
 *        whole length
 *    6x  credentials: with SO_PASSCRED on, recvmsg carries SCM_CREDENTIALS
 *        with this process's pid, uid and gid
 *    7x  a sequenced-packet pair keeps each message whole; poll sees the
 *        receiver readable, and not before
 *    8x  autobind: bind with the family alone gives an abstract name of five
 *        hex digits; SO_RCVTIMEO of 100 ms makes an empty receive EAGAIN
 *    9x  a stream in pieces: sendmsg from three iovecs (one empty) and
 *        recvmsg into three, in order; then MSG_WAITALL fills two iovecs
 *        from 100000 bytes a child sends in two parts, a pause between --
 *        more than the 64 KiB a datagram may be, which the library once
 *        capped a stream's recvmsg at
 *
 * There is no check 4, so no failure is 42.
 *
 * Exit codes: 42 is every check passed.
 *    2  fork or waitpid failed     3  the child failed (its code is printed)
 * Otherwise <check><step>, each step's line printed before it fails.
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/uio.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define SOCK_PATH "/tmp/ctest-unix.sock"
#define MISSING_PATH "/tmp/ctest-unix-nothing-here.sock"
#define DGRAM_NAME "ctest-unix-dgram"
#define SENDER_NAME "ctest-unix-sender"
#define PIECES_NAME "ctest-unix-pieces"
#define BIG_LEN 100000

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
    emit("ctest-unix-socket: FAIL ");
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
    emit("ctest-unix-socket: ok ");
    emit(what);
    emit("\n");
}

/* A sockaddr_un for path `p`, and its length as Linux counts it. */
static socklen_t path_addr(struct sockaddr_un *a, const char *p)
{
    memset(a, 0, sizeof *a);
    a->sun_family = AF_UNIX;
    size_t n = strlen(p);
    memcpy(a->sun_path, p, n);
    return (socklen_t)(sizeof(a->sun_family) + n + 1);
}

/* A sockaddr_un for abstract name `p` (no trailing NUL), and its length. */
static socklen_t abstract_addr(struct sockaddr_un *a, const char *p)
{
    memset(a, 0, sizeof *a);
    a->sun_family = AF_UNIX;
    size_t n = strlen(p);
    memcpy(a->sun_path + 1, p, n);
    return (socklen_t)(sizeof(a->sun_family) + 1 + n);
}

/* The child of check 2: connect, send "ping", expect "pong", close. Exits 0,
 * or the step it failed at. */
static int client(void)
{
    struct sockaddr_un a;
    socklen_t len = path_addr(&a, SOCK_PATH);
    int s = socket(AF_UNIX, SOCK_STREAM, 0);
    if (s < 0) {
        return 1;
    }
    if (connect(s, (struct sockaddr *)&a, len) != 0) {
        return 2;
    }
    if (send(s, "ping", 4, 0) != 4) {
        return 3;
    }
    char buf[8];
    /* MSG_WAITALL for exactly the four bytes the server sends: asked for
     * more, it would wait for bytes that never come. */
    ssize_t n = recv(s, buf, 4, MSG_WAITALL);
    if (n != 4 || memcmp(buf, "pong", 4) != 0) {
        return 4;
    }
    close(s);
    return 0;
}

static int check_stream(void)
{
    struct sockaddr_un a;
    socklen_t len = path_addr(&a, SOCK_PATH);
    unlink(SOCK_PATH);

    int srv = socket(AF_UNIX, SOCK_STREAM, 0);
    if (srv < 0) {
        return fail(11, "socket(AF_UNIX, SOCK_STREAM)");
    }
    if (bind(srv, (struct sockaddr *)&a, len) != 0) {
        return fail(12, "bind to " SOCK_PATH);
    }
    if (listen(srv, 4) != 0) {
        return fail(13, "listen");
    }
    struct sockaddr_un got;
    socklen_t got_len = sizeof got;
    if (getsockname(srv, (struct sockaddr *)&got, &got_len) != 0 || got_len != len
        || strcmp(got.sun_path, SOCK_PATH) != 0) {
        return fail(14, "getsockname gives the path back");
    }
    ok("a stream server listens on " SOCK_PATH);

    pid_t child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        close(srv);
        _exit(client());
    }

    int conn = accept(srv, NULL, NULL);
    if (conn < 0) {
        return fail(21, "accept");
    }
    char buf[8];
    ssize_t n = recv(conn, buf, 4, MSG_WAITALL);
    if (n != 4 || memcmp(buf, "ping", 4) != 0) {
        return fail(22, "the server reads \"ping\"");
    }
    struct sockaddr_un peer;
    socklen_t peer_len = sizeof peer;
    if (getpeername(conn, (struct sockaddr *)&peer, &peer_len) != 0
        || peer_len != sizeof(peer.sun_family)) {
        return fail(23, "getpeername: the client is unnamed");
    }
    struct ucred cred;
    socklen_t cred_len = sizeof cred;
    if (getsockopt(conn, SOL_SOCKET, SO_PEERCRED, &cred, &cred_len) != 0
        || cred_len != sizeof cred || cred.pid != child || cred.uid != getuid()
        || cred.gid != getgid()) {
        return fail(24, "SO_PEERCRED names the child");
    }
    if (send(conn, "pong", 4, 0) != 4) {
        return fail(25, "the server sends \"pong\"");
    }
    n = recv(conn, buf, sizeof buf, 0);
    if (n != 0) {
        return fail(26, "the child's close is end of file");
    }
    int status = 0;
    if (waitpid(child, &status, 0) != child) {
        return fail(2, "waitpid");
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        emit("ctest-unix-socket: the client failed at its step ");
        emit_num(WIFEXITED(status) ? WEXITSTATUS(status) : -1);
        emit("\n");
        return fail(3, "the client");
    }
    ok("a client in another process connected, was seen, and was answered");

    /* 3: the error answers. */
    int other = socket(AF_UNIX, SOCK_STREAM, 0);
    if (other < 0) {
        return fail(31, "a second socket");
    }
    if (bind(other, (struct sockaddr *)&a, len) == 0 || errno != EADDRINUSE) {
        return fail(31, "a second bind to the path is EADDRINUSE");
    }
    struct sockaddr_un missing;
    socklen_t missing_len = path_addr(&missing, MISSING_PATH);
    if (connect(other, (struct sockaddr *)&missing, missing_len) == 0
        || (errno != ENOENT && errno != ECONNREFUSED)) {
        return fail(32, "a connect to nothing is ENOENT or ECONNREFUSED");
    }
    close(other);
    int client_side = socket(AF_UNIX, SOCK_STREAM, 0);
    if (client_side < 0 || connect(client_side, (struct sockaddr *)&a, len) != 0) {
        return fail(33, "a second client connects");
    }
    if (sendto(client_side, "x", 1, 0, (struct sockaddr *)&a, len) != -1 || errno != EISCONN) {
        return fail(33, "a connected stream's sendto naming an address is EISCONN");
    }
    close(client_side);
    int conn2 = accept(srv, NULL, NULL);
    if (conn2 >= 0) {
        close(conn2);
    }
    /* The listener non-blocking: accept4's SOCK_NONBLOCK is the new
     * descriptor's, not the accept's. */
    if (fcntl(srv, F_SETFL, fcntl(srv, F_GETFL) | O_NONBLOCK) != 0) {
        return fail(34, "O_NONBLOCK on the listener");
    }
    int nb = accept(srv, NULL, NULL);
    if (nb != -1 || (errno != EAGAIN && errno != EWOULDBLOCK)) {
        return fail(34, "a non-blocking accept with nothing waiting is EAGAIN");
    }
    ok("the error answers are Linux's");
    close(conn);
    close(srv);
    unlink(SOCK_PATH);
    return 0;
}

static int check_dgram(void)
{
    struct sockaddr_un a, s_addr;
    socklen_t len = abstract_addr(&a, DGRAM_NAME);
    socklen_t s_len = abstract_addr(&s_addr, SENDER_NAME);
    int rx = socket(AF_UNIX, SOCK_DGRAM, 0);
    int tx = socket(AF_UNIX, SOCK_DGRAM, 0);
    if (rx < 0 || tx < 0) {
        return fail(51, "two datagram sockets");
    }
    if (bind(rx, (struct sockaddr *)&a, len) != 0 || bind(tx, (struct sockaddr *)&s_addr, s_len) != 0) {
        return fail(52, "bind both to abstract names");
    }
    if (sendto(tx, "hello", 5, 0, (struct sockaddr *)&a, len) != 5
        || sendto(tx, "0123456789", 10, 0, (struct sockaddr *)&a, len) != 10) {
        return fail(53, "sendto by name, twice");
    }
    char buf[16];
    struct sockaddr_un from;
    socklen_t from_len = sizeof from;
    ssize_t n = recvfrom(rx, buf, sizeof buf, 0, (struct sockaddr *)&from, &from_len);
    if (n != 5 || memcmp(buf, "hello", 5) != 0 || from_len != s_len || from.sun_path[0] != 0
        || memcmp(from.sun_path + 1, SENDER_NAME, strlen(SENDER_NAME)) != 0) {
        return fail(54, "recvfrom gets the first datagram and names its sender");
    }
    n = recv(rx, buf, 4, MSG_TRUNC);
    if (n != 10 || memcmp(buf, "0123", 4) != 0) {
        return fail(55, "MSG_TRUNC reports a datagram's whole length");
    }
    ok("datagrams by abstract name keep their boundaries and their senders");

    /* 6: credentials with each message. */
    int on = 1;
    if (setsockopt(rx, SOL_SOCKET, SO_PASSCRED, &on, sizeof on) != 0) {
        return fail(61, "SO_PASSCRED on");
    }
    if (sendto(tx, "cred", 4, 0, (struct sockaddr *)&a, len) != 4) {
        return fail(62, "send one more");
    }
    char data[8];
    struct iovec iov = { data, sizeof data };
    union {
        struct cmsghdr align;
        char buf[CMSG_SPACE(sizeof(struct ucred))];
    } control;
    struct msghdr msg;
    memset(&msg, 0, sizeof msg);
    msg.msg_iov = &iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.buf;
    msg.msg_controllen = sizeof control.buf;
    n = recvmsg(rx, &msg, 0);
    struct cmsghdr *c = CMSG_FIRSTHDR(&msg);
    if (n != 4 || c == NULL || c->cmsg_level != SOL_SOCKET || c->cmsg_type != SCM_CREDENTIALS) {
        return fail(63, "recvmsg carries SCM_CREDENTIALS");
    }
    struct ucred cred;
    memcpy(&cred, CMSG_DATA(c), sizeof cred);
    if (cred.pid != getpid() || cred.uid != getuid() || cred.gid != getgid()) {
        return fail(64, "the credentials are this process's");
    }
    ok("SO_PASSCRED hands each message's credentials back");
    close(rx);
    close(tx);
    return 0;
}

static int check_seqpacket(void)
{
    int sv[2];
    if (socketpair(AF_UNIX, SOCK_SEQPACKET, 0, sv) != 0) {
        return fail(71, "socketpair(AF_UNIX, SOCK_SEQPACKET)");
    }
    struct pollfd p = { sv[1], POLLIN, 0 };
    if (poll(&p, 1, 0) != 0) {
        return fail(72, "nothing to read yet");
    }
    if (send(sv[0], "one", 3, 0) != 3 || send(sv[0], "second", 6, 0) != 6) {
        return fail(73, "two records");
    }
    if (poll(&p, 1, 1000) != 1 || !(p.revents & POLLIN)) {
        return fail(74, "poll sees the receiver readable");
    }
    char buf[16];
    if (recv(sv[1], buf, sizeof buf, 0) != 3 || recv(sv[1], buf, sizeof buf, 0) != 6
        || memcmp(buf, "second", 6) != 0) {
        return fail(75, "each record comes whole");
    }
    ok("a sequenced-packet pair keeps its records, and poll sees them");
    close(sv[0]);
    close(sv[1]);
    return 0;
}

static int check_autobind_and_timeout(void)
{
    int s = socket(AF_UNIX, SOCK_DGRAM, 0);
    if (s < 0) {
        return fail(81, "socket");
    }
    struct sockaddr_un a;
    memset(&a, 0, sizeof a);
    a.sun_family = AF_UNIX;
    if (bind(s, (struct sockaddr *)&a, sizeof(a.sun_family)) != 0) {
        return fail(81, "bind with the family alone");
    }
    struct sockaddr_un got;
    socklen_t got_len = sizeof got;
    if (getsockname(s, (struct sockaddr *)&got, &got_len) != 0
        || got_len != sizeof(got.sun_family) + 1 + 5 || got.sun_path[0] != 0) {
        return fail(82, "the autobound name is abstract and five bytes long");
    }
    for (int i = 1; i <= 5; i++) {
        char ch = got.sun_path[i];
        if (!((ch >= '0' && ch <= '9') || (ch >= 'a' && ch <= 'f'))) {
            return fail(82, "the autobound name is hex digits");
        }
    }
    struct timeval tv = { 0, 100000 };
    if (setsockopt(s, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv) != 0) {
        return fail(83, "SO_RCVTIMEO");
    }
    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    char buf[4];
    if (recv(s, buf, sizeof buf, 0) != -1 || (errno != EAGAIN && errno != EWOULDBLOCK)) {
        return fail(84, "an empty receive with a timeout is EAGAIN");
    }
    clock_gettime(CLOCK_MONOTONIC, &t1);
    long ms = (t1.tv_sec - t0.tv_sec) * 1000L + (t1.tv_nsec - t0.tv_nsec) / 1000000L;
    if (ms < 90) {
        return fail(85, "the receive waited for its timeout");
    }
    ok("autobind names in five hex digits, and SO_RCVTIMEO ends a wait");
    close(s);
    return 0;
}

/* The child of check 9: connect to the pieces listener and send BIG_LEN
 * patterned bytes in two parts, the second after a pause, so that the
 * parent's receive has to wait for it. Exits 0, or the step it failed at. */
static int pieces_sender(void)
{
    static char big[BIG_LEN];
    for (int i = 0; i < BIG_LEN; i++) {
        big[i] = (char)(i % 251);
    }
    struct sockaddr_un a;
    socklen_t len = abstract_addr(&a, PIECES_NAME);
    int s = socket(AF_UNIX, SOCK_STREAM, 0);
    if (s < 0 || connect(s, (struct sockaddr *)&a, len) != 0) {
        return 1;
    }
    if (write(s, big, 30000) != 30000) {
        return 2;
    }
    struct timespec pause = { 0, 100000000 };
    nanosleep(&pause, NULL);
    size_t done = 30000;
    while (done < sizeof big) {
        ssize_t n = write(s, big + done, sizeof big - done);
        if (n <= 0) {
            return 3;
        }
        done += (size_t)n;
    }
    close(s);
    return 0;
}

static int check_stream_pieces(void)
{
    struct sockaddr_un a;
    socklen_t len = abstract_addr(&a, PIECES_NAME);
    int l = socket(AF_UNIX, SOCK_STREAM, 0);
    if (l < 0 || bind(l, (struct sockaddr *)&a, len) != 0 || listen(l, 2) != 0) {
        return fail(91, "a stream listener at an abstract name");
    }
    int tx = socket(AF_UNIX, SOCK_STREAM, 0);
    if (tx < 0 || connect(tx, (struct sockaddr *)&a, len) != 0) {
        return fail(92, "connect to it");
    }
    int rx = accept(l, NULL, NULL);
    if (rx < 0) {
        return fail(92, "accept");
    }

    char p1[] = "abc", p3[] = "defgh";
    struct iovec out[3] = { { p1, 3 }, { NULL, 0 }, { p3, 5 } };
    struct msghdr m;
    memset(&m, 0, sizeof m);
    m.msg_iov = out;
    m.msg_iovlen = 3;
    if (sendmsg(tx, &m, 0) != 8) {
        return fail(93, "sendmsg from three pieces, one empty, sends all eight bytes");
    }
    char q1[3], q2[2], q3[10];
    struct iovec in[3] = { { q1, sizeof q1 }, { q2, sizeof q2 }, { q3, sizeof q3 } };
    memset(&m, 0, sizeof m);
    m.msg_iov = in;
    m.msg_iovlen = 3;
    if (recvmsg(rx, &m, 0) != 8 || memcmp(q1, "abc", 3) != 0 || memcmp(q2, "de", 2) != 0
        || memcmp(q3, "fgh", 3) != 0) {
        return fail(94, "recvmsg into three pieces fills them in order, and stops at what came");
    }
    close(tx);
    close(rx);

    pid_t child = fork();
    if (child < 0) {
        return fail(2, "fork");
    }
    if (child == 0) {
        _exit(pieces_sender());
    }
    int conn = accept(l, NULL, NULL);
    if (conn < 0) {
        return fail(95, "accept the sender");
    }
    static char got[BIG_LEN];
    struct iovec two[2] = { { got, 40000 }, { got + 40000, BIG_LEN - 40000 } };
    memset(&m, 0, sizeof m);
    m.msg_iov = two;
    m.msg_iovlen = 2;
    ssize_t n = recvmsg(conn, &m, MSG_WAITALL);
    int status = 0;
    if (waitpid(child, &status, 0) != child) {
        return fail(2, "waitpid");
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        emit("ctest-unix-socket: the sender failed at its step ");
        emit_num(WIFEXITED(status) ? WEXITSTATUS(status) : -1);
        emit("\n");
        return fail(3, "the sender");
    }
    if (n != BIG_LEN) {
        emit("ctest-unix-socket: recvmsg returned ");
        emit_num((long)n);
        emit("\n");
        return fail(96, "MSG_WAITALL fills both pieces, more than a datagram's worth");
    }
    for (int i = 0; i < BIG_LEN; i++) {
        if (got[i] != (char)(i % 251)) {
            return fail(97, "every byte in its place");
        }
    }
    ok("a stream's sendmsg and recvmsg in pieces, and MSG_WAITALL across them");
    close(conn);
    close(l);
    return 0;
}

int main(void)
{
    int code;
    if ((code = check_stream()) != 0) {
        return code;
    }
    if ((code = check_dgram()) != 0) {
        return code;
    }
    if ((code = check_seqpacket()) != 0) {
        return code;
    }
    if ((code = check_autobind_and_timeout()) != 0) {
        return code;
    }
    if ((code = check_stream_pieces()) != 0) {
        return code;
    }
    emit("ctest-unix-socket: every check passed\n");
    return 42;
}
