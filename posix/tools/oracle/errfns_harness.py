"""glibc 2.39's <error.h> and <err.h> -- error, error_at_line, warn, warnx,
err, errx -- and the variables they share with the program, as the oracle
for posix/src/error.rs and posix/src/err.rs.

    python posix/tools/oracle/errfns_harness.py   # writes posix/src/errfns_oracle.txt

One line a probe, `<probe> = <output> | exit <status>`: everything the probe
wrote, `stdout` and `stderr` together, as one pipe read it -- escaped, `\\n`
for a newline -- and how the probe's process ended. Each probe runs in a
child whose `stdout` is a pipe it has not used before, so fully buffered, as
a program's is when its output is redirected: that is what shows `error`
flushing it first and `warn` not.

The program's name is written `<full>` for argv[0] as it was started
(`program_invocation_name`, which `error` prints) and `<short>` for its last
component (`__progname`, which `warn` prints); a 5000-byte body is written
`<5000 y>`.

The probes of the variables themselves -- that `program_invocation_name` and
`__progname_full` are one variable, `program_invocation_short_name` and
`__progname` another, and `environ`, `__environ` and `_environ` a third --
are recorded as comments: on the host, where the tests run, there is no C
program to share a variable with, and on the target the three are one word
under several names, which `scripts/check-libc-shape.py` checks in libc.a.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "errfns_oracle.txt"
EXE = "errfns"

PROGRAM = r'''
#define _GNU_SOURCE
#include <err.h>
#include <errno.h>
#include <error.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

extern char *__progname;
extern char *__progname_full;
extern char **__environ;
extern char **_environ;

static void cb(void) { fprintf(stderr, "[cb]"); }

/* Run `f` in a child with stdout and stderr on one pipe; print what came out,
 * escaped, and how the child ended. The parent writes with dprintf, never
 * through stdout, so that each child's stdout is untouched until it uses it. */
static void probe(const char *name, void (*f)(void))
{
    int p[2];
    if (pipe(p) != 0) return;
    pid_t pid = fork();
    if (pid == 0) {
        dup2(p[1], 1);
        dup2(p[1], 2);
        close(p[0]);
        close(p[1]);
        f();
        fflush(stdout);
        _exit(0);
    }
    close(p[1]);
    static char buf[65536];
    size_t n = 0;
    ssize_t r;
    while ((r = read(p[0], buf + n, sizeof buf - 1 - n)) > 0) n += r;
    close(p[0]);
    int st;
    waitpid(pid, &st, 0);
    dprintf(1, "%s = ", name);
    for (size_t i = 0; i < n; i++) {
        unsigned char c = buf[i];
        if (c == '\n') dprintf(1, "\\n");
        else if (c == '\\') dprintf(1, "\\\\");
        else if (c < 32 || c > 126) dprintf(1, "\\x%02x", c);
        else dprintf(1, "%c", c);
    }
    dprintf(1, " | %s %d\n", WIFEXITED(st) ? "exit" : "signal", WIFEXITED(st) ? WEXITSTATUS(st) : WTERMSIG(st));
}

static void p_error(void) { error(0, 0, "msg %d", 5); }
static void p_error_errnum(void) { error(0, ENOENT, "open %s", "x"); }
static void p_error_unknown(void) { error(0, -5, "odd"); }
static void p_error_status(void) { error(3, 0, "bye"); fprintf(stderr, "not reached"); }
static void p_error_at_line(void) { error_at_line(0, 0, "f.c", 12, "m %s", "z"); }
static void p_error_at_line_null(void) { error_at_line(0, EACCES, NULL, 12, "m"); }
static void p_one_per_line(void)
{
    error_one_per_line = 1;
    error_at_line(0, 0, "f.c", 12, "first");
    error_at_line(0, 0, "f.c", 12, "second");
    error_at_line(0, 0, "f.c", 13, "third");
    error_at_line(0, 0, "g.c", 13, "fourth");
    error_at_line(0, 0, "g.c", 13, "fifth");
    error(0, 0, "plain");
    error_at_line(0, 0, "g.c", 13, "sixth");
    fprintf(stderr, "count=%u\n", error_message_count);
}
static void p_callback(void)
{
    error_print_progname = cb;
    error(0, 0, "a");
    error_at_line(0, 0, "f.c", 1, "b");
}
static void p_flush(void)
{
    printf("partial");
    error(0, 0, "after");
    printf("rest\n");
}
static void p_long(void)
{
    static char big[5001];
    memset(big, 'y', 5000);
    error(0, 0, "%s", big);
}
static void p_count(void)
{
    error(0, 0, "a");
    error(0, 0, "b");
    fprintf(stderr, "count=%u\n", error_message_count);
}
static void p_warn(void) { errno = EPERM; warn("w %d", 1); }
static void p_warn_null(void) { errno = EPERM; warn(NULL); }
static void p_warnx(void) { warnx("x %d", 2); }
static void p_warnx_null(void) { warnx(NULL); }
static void p_err(void) { errno = ENOENT; err(4, "e"); }
static void p_errx(void) { errx(5, "e"); }
static void p_warn_noflush(void)
{
    printf("partial");
    warnx("w");
    printf("rest\n");
}
static void p_renamed(void)
{
    program_invocation_name = "renamed/full";
    error(0, 0, "e");
    warnx("w");
    fprintf(stderr, "__progname_full=%s\n", __progname_full);
}
static void p_renamed_short(void)
{
    program_invocation_short_name = "short2";
    warnx("w");
    fprintf(stderr, "__progname=%s\n", __progname);
    __progname = "short3";
    warnx("w");
    fprintf(stderr, "program_invocation_short_name=%s\n", program_invocation_short_name);
}
static void p_environ(void)
{
    setenv("SLATE_A", "1", 1);
    fprintf(stderr, "&environ==&__environ:%d &environ==&_environ:%d\n", &environ == &__environ,
            &environ == &_environ);
    static char *list[] = {"SLATE_B=2", NULL};
    __environ = list;
    const char *a = getenv("SLATE_A"), *b = getenv("SLATE_B");
    fprintf(stderr, "after __environ=list: A=%s B=%s environ==list:%d\n", a ? a : "(null)", b ? b : "(null)",
            environ == list);
}

int main(void)
{
    probe("error", p_error);
    probe("error(ENOENT)", p_error_errnum);
    probe("error(-5)", p_error_unknown);
    probe("error(status 3)", p_error_status);
    probe("error_at_line", p_error_at_line);
    probe("error_at_line(NULL file)", p_error_at_line_null);
    probe("error_one_per_line", p_one_per_line);
    probe("error_print_progname", p_callback);
    probe("error flushes stdout", p_flush);
    probe("error 5000 bytes", p_long);
    probe("error_message_count", p_count);
    probe("warn", p_warn);
    probe("warn(NULL)", p_warn_null);
    probe("warnx", p_warnx);
    probe("warnx(NULL)", p_warnx_null);
    probe("err", p_err);
    probe("errx", p_errx);
    probe("warnx does not flush stdout", p_warn_noflush);
    probe("# program_invocation_name", p_renamed);
    probe("# program_invocation_short_name", p_renamed_short);
    probe("# environ", p_environ);
    return 0;
}
'''


def main() -> None:
    with workdir() as t:
        d = Path(t)
        (d / "errfns.c").write_text(PROGRAM, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O0 -w -o {EXE} errfns.c && ./{EXE}")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
        body = r.stdout
    # The names, full first: the short one is inside it.
    body = body.replace("./" + EXE, "<full>").replace(EXE, "<short>").replace("y" * 5000, "<5000 y>")
    head = ("# glibc 2.39's error, error_at_line, warn, warnx, err and errx, and the program's\n"
            "# name and environment variables, for posix/src/error.rs and err.rs.\n"
            "# Generated by posix/tools/oracle/errfns_harness.py; do not edit.\n")
    OUT.write_text(head + body, encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {body.count(chr(10))} lines")


if __name__ == "__main__":
    main()
