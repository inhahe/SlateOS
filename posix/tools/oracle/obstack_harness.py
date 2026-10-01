"""glibc 2.39's obstacks, <obstack.h>, as the oracle for posix/src/obstack.rs
and posix/include/obstack.h:

    python posix/tools/oracle/obstack_harness.py
        # writes posix/src/obstack_oracle.txt and services/ctest-obstack/main.c
    python posix/tools/oracle/obstack_harness.py --header
        # posix/include/obstack.h's macros, both forms, over glibc's functions

Each scenario is a list of operations on one obstack, run in a child process
of its own; after each, the obstack's state is written. The chunks come
from a counting allocator whose blocks are 4096-aligned, so that where an
object lands in its chunk does not depend on where malloc put the chunk.

    D <chunk size> <alignment mask> <obstack_exit_failure> <handler set>
    S <scenario> <op> = <state>
    P <format> <argument> = <returned> <object size> <object, hex>
    F default = <what the default failure handler wrote, hex> <exit status>

An operation is one of: `begin:S:A` (obstack_specify_allocation, size S,
alignment A), `begin1:S:A` (the _with_arg form, the functions given their
argument), `mask:M` (obstack_alignment_mask set), `grow:N` `grow0:N`
`1grow:C` `ptr:K` `int:K` `blank:N` `room:N` (obstack_make_room) `finish`
`alloc:N` `copy:N` `copy0:N` `free:#K` (back to finished object K)
`freeall` (obstack_free with NULL) `newchunk:N` (_obstack_newchunk) `fail:K`
(the K-th allocation from now fails, a handler longjmps out). A state is:

    size room base next limit chunks used empty [allocs] frees [obj]

the object's size and the room after it; object_base, next_free and
chunk_limit as offsets from the current chunk's start; the chunks in the
chain; _obstack_memory_used; obstack_empty_p; the sizes the allocation
function was asked for, and how many chunks were given back, since the
operation before; and for `finish`, `alloc`, `copy` and `copy0`, the new
object -- the chunk it is in, counted back from the current one, its
offset there, and whether it holds what was put in it.

**The fixture.** The same program, built against posix/include's
<obstack.h> and this library (`services/ctest-obstack`, SLATEOS_FIXTURE
defined), is what tests the header's macros themselves, which the host
tests can only mirror: it collects the same lines -- the scenarios run in
one process rather than one each -- compares them with glibc's, embedded,
and exits 42 if every one is the same. It makes one check of its own,
which is no line of glibc's: that a failure handler that returns ends the
process as abort() does (glibc's faults instead; design-decisions §1162).
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "obstack_oracle.txt"
FIXTURE = HERE.parent.parent.parent / "services" / "ctest-obstack" / "main.c"


def scenarios():
    """(name, [op])."""
    out = []
    # The defaults, and objects growing past a chunk.
    out.append(("init", ["begin:0:0", "grow:10", "finish", "grow:4000", "finish", "grow:5000",
                         "finish", "alloc:1", "alloc:4064", "alloc:100000"]))
    # Growth from inside an object far into a chunk, by small steps.
    steps = ["begin:0:0"] + [f"grow:{n}" for n in (1, 2, 3, 500, 1000, 1500, 2000, 4000)] + \
        ["finish"] + [f"1grow:{65 + k}" for k in range(5)] + ["finish"]
    out.append(("steps", steps))
    # Every size of chunk asked for, small and odd.
    for size in (1, 64, 100, 128, 255, 4095, 9000):
        out.append((f"size{size}", [f"begin:{size}:0", "grow:50", "finish", "grow:200", "finish",
                                    "alloc:1000", "copy:30", "copy0:30"]))
    # Every alignment, and one changed after the obstack began.
    for align in (1, 2, 4, 8, 16, 32, 256):
        out.append((f"align{align}", [f"begin:0:{align}", "grow:3", "finish", "alloc:5",
                                      "alloc:17", "grow:1", "finish", "copy0:4"]))
    out.append(("mask", ["begin:0:0", "mask:0", "alloc:3", "alloc:3", "mask:7", "alloc:3",
                         "mask:255", "alloc:3", "alloc:300"]))
    # Each way of growing an object.
    out.append(("grows", ["begin:200:0", "1grow:120", "1grow:0", "ptr:1234", "int:-5",
                          "blank:20", "blank:-3", "grow0:7", "finish", "room:5000", "grow:10",
                          "finish", "blank:0", "finish"]))
    # Freeing: back to an object in the current chunk, in an earlier one, to
    # the first, to everything; and empty objects, which keep a chunk alive.
    out.append(("free", ["begin:0:0", "alloc:10", "alloc:3000", "alloc:3000", "alloc:3000",
                         "free:#3", "free:#2", "alloc:10", "free:#1", "alloc:5000", "freeall"]))
    out.append(("empty", ["begin:0:0", "grow:100", "finish", "finish", "grow:4000", "finish",
                          "grow:4000", "finish", "free:#2", "grow:5", "finish"]))
    out.append(("emptyfirst", ["begin:0:0", "finish", "grow:5000", "finish"]))
    out.append(("move", ["begin:0:0", "grow:4000", "grow:100", "finish", "alloc:20",
                         "grow:3000", "grow:3000", "finish"]))
    # _obstack_newchunk itself, at every length, and with the extra-argument
    # functions.
    out.append(("newchunk", ["begin:0:0", "newchunk:0", "grow:7", "newchunk:1", "newchunk:100",
                             "grow:900", "newchunk:5000", "finish", "newchunk:-1"]))
    out.append(("witharg", ["begin1:300:8", "alloc:100", "alloc:250", "grow:600", "finish",
                            "free:#1", "freeall"]))
    # A chunk that cannot be had.
    out.append(("fails", ["begin:0:0", "alloc:10", "fail:1", "alloc:5000", "alloc:10",
                          "grow:9000", "finish"]))
    return out


PRINTF = [
    ("%d", "42"), ("%s-%s", "ab"), ("%5000d", "7"), ("%%", "0"), ("", "0"), ("%c%c", "88"),
    ("x%sy", "long"), ("%.3s", "abcdef"),
]

C_MAIN = r'''
#include <obstack.h>
#include <setjmp.h>
#include <signal.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

#ifdef SLATEOS_FIXTURE
/* The lines, collected, to be compared with glibc's at the end. */
static char text[1 << 16];
static size_t text_len;

static void OUT(const char *fmt, ...) __attribute__((__format__(__printf__, 1, 2)));
static void OUT(const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(text + text_len, sizeof text - text_len, fmt, ap);
    va_end(ap);
    if (n > 0 && (size_t) n < sizeof text - text_len)
        text_len += (size_t) n;
}
#else
# define OUT printf
#endif

static long allocs[64];
static int nalloc, nfree, fail_in;
static jmp_buf jb;

static void *chunk(long size)
{
    if (fail_in > 0 && --fail_in == 0)
        return NULL;
    if (nalloc < 64)
        allocs[nalloc] = size;
    nalloc++;
    return aligned_alloc(4096, (size_t) ((size + 4095) / 4096) * 4096);
}

static void unchunk(void *p)
{
    nfree++;
    free(p);
}

static void *chunk1(void *arg, long size)
{
    if (arg != (void *) 0x5eed)
        abort();
    return chunk(size);
}

static void unchunk1(void *arg, void *p)
{
    if (arg != (void *) 0x5eed)
        abort();
    unchunk(p);
}

static void jump_out(void)
{
    longjmp(jb, 1);
}

static unsigned char pattern[200000];
static char *objs[64];
static int nobj;

static int chunks_in(struct obstack *h)
{
    int n = 0;
    for (struct _obstack_chunk *c = h->chunk; c; c = c->prev)
        n++;
    return n;
}

/* The chunk an address is in, counted back from the current one, and its
 * offset there; -1 for none. */
static void where(struct obstack *h, char *p, int *which, long *off)
{
    int k = 0;
    for (struct _obstack_chunk *c = h->chunk; c; c = c->prev, k++)
        if ((char *) c < p && p <= c->limit) {
            *which = k;
            *off = (long) (p - (char *) c);
            return;
        }
    *which = -1;
    *off = 0;
}

static void state(struct obstack *h, int obj, int good)
{
    char *base = (char *) h->chunk;
    OUT("%u %u %ld %ld %ld %d %d %d [", obstack_object_size(h), obstack_room(h),
        (long) (h->object_base - base), (long) (h->next_free - base),
        (long) (h->chunk_limit - base), chunks_in(h), obstack_memory_used(h),
        obstack_empty_p(h) ? 1 : 0);
    for (int k = 0; k < nalloc && k < 64; k++)
        OUT("%s%ld", k ? "," : "", allocs[k]);
    OUT("] %d", nfree);
    if (obj >= 0) {
        int which;
        long off;
        where(h, objs[obj], &which, &off);
        OUT(" #%d:%d:%ld:%s", obj + 1, which, off, good ? "ok" : "bad");
    }
    OUT("\n");
    nalloc = nfree = 0;
}

static void run_ops(const char *name, const char *const *ops, int n)
{
    /* Static, as what changes between a setjmp and its longjmp must be. */
    static struct obstack ob;
    static int live;
    struct obstack *h = &ob;
    nobj = 0;
    nalloc = nfree = fail_in = live = 0;
    obstack_alloc_failed_handler = jump_out;
    for (int i = 0; i < n; i++) {
        const char *op = ops[i];
        OUT("S %s %s = ", name, op);
        if (setjmp(jb)) {
            OUT("failed\n");
            nalloc = nfree = 0;
            continue;
        }
        long a = 0, b = 0;
        int obj = -1, good = 1;
        if (sscanf(op, "begin:%ld:%ld", &a, &b) == 2) {
            obstack_specify_allocation(h, (int) a, (int) b, chunk, unchunk);
            live = 1;
        } else if (sscanf(op, "begin1:%ld:%ld", &a, &b) == 2) {
            obstack_specify_allocation_with_arg(h, (int) a, (int) b, chunk1, unchunk1,
                                                (void *) 0x5eed);
            live = 1;
        } else if (sscanf(op, "mask:%ld", &a) == 1)
            obstack_alignment_mask(h) = (int) a;
        else if (sscanf(op, "grow0:%ld", &a) == 1)
            obstack_grow0(h, pattern, (int) a);
        else if (sscanf(op, "grow:%ld", &a) == 1)
            obstack_grow(h, pattern, (int) a);
        else if (sscanf(op, "1grow:%ld", &a) == 1)
            obstack_1grow(h, (char) a);
        else if (sscanf(op, "ptr:%ld", &a) == 1)
            obstack_ptr_grow(h, (void *) a);
        else if (sscanf(op, "int:%ld", &a) == 1)
            obstack_int_grow(h, (int) a);
        else if (sscanf(op, "blank:%ld", &a) == 1)
            obstack_blank(h, (int) a);
        else if (sscanf(op, "room:%ld", &a) == 1)
            obstack_make_room(h, (int) a);
        else if (sscanf(op, "newchunk:%ld", &a) == 1)
            _obstack_newchunk(h, (int) a);
        else if (sscanf(op, "fail:%ld", &a) == 1)
            fail_in = (int) a;
        else if (sscanf(op, "free:#%ld", &a) == 1)
            obstack_free(h, objs[a - 1]);
        else if (!strcmp(op, "freeall")) {
            /* Which leaves h->chunk pointing at a chunk given back. */
            obstack_free(h, NULL);
            live = 0;
            OUT("freed %d\n", nfree);
            nalloc = nfree = 0;
            continue;
        } else {
            int size = (int) obstack_object_size(h);
            char *p;
            if (!strcmp(op, "finish"))
                p = obstack_finish(h);
            else if (sscanf(op, "alloc:%ld", &a) == 1) {
                p = obstack_alloc(h, (int) a);
                memset(p, 0x5a, (size_t) a);
                size = (int) a;
            } else if (sscanf(op, "copy0:%ld", &a) == 1) {
                p = obstack_copy0(h, pattern, (int) a);
                size = (int) a;
                good = p[a] == 0;
            } else if (sscanf(op, "copy:%ld", &a) == 1) {
                p = obstack_copy(h, pattern, (int) a);
                size = (int) a;
            } else {
                OUT("bad op\n");
                continue;
            }
            if (op[0] == 'c')
                good = good && memcmp(p, pattern, (size_t) size) == 0;
            objs[nobj] = p;
            obj = nobj++;
        }
        state(h, obj, good);
    }
    if (live)
        obstack_free(h, NULL);
}

static void p_case(const char *fmt, const char *arg)
{
    struct obstack ob;
    obstack_specify_allocation(&ob, 0, 0, chunk, unchunk);
    obstack_grow(&ob, "<", 1);
    int r;
    if (!strcmp(fmt, "%d") || !strcmp(fmt, "%5000d"))
        r = obstack_printf(&ob, fmt, atoi(arg));
    else if (!strcmp(fmt, "%c%c"))
        r = obstack_printf(&ob, fmt, arg[0], arg[1]);
    else if (!strcmp(fmt, "%s-%s"))
        r = obstack_printf(&ob, fmt, "a", "b");
    else
        r = obstack_printf(&ob, fmt, arg);
    int size = (int) obstack_object_size(&ob);
    char *p = obstack_finish(&ob);
    OUT("P %s %s = %d %d ", *fmt ? fmt : "\\-", arg, r, size);
    for (int k = 0; k < size && k < 64; k++)
        OUT("%02x", (unsigned char) p[k]);
    if (size > 64) {
        unsigned long sum = 0;
        for (int k = 0; k < size; k++)
            sum = sum * 31 + (unsigned char) p[k];
        OUT("...%lx", sum);
    }
    OUT("\n");
    obstack_free(&ob, NULL);
}

static void f_case(void)
{
    int fds[2];
    if (pipe(fds))
        return;
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        dup2(fds[1], 2);
        struct obstack ob;
        obstack_exit_failure = 7;
        obstack_alloc_failed_handler = obstack_alloc_failed_handler_default;
        fail_in = 1;
        obstack_specify_allocation(&ob, 0, 0, chunk, unchunk);
        _exit(99);
    }
    close(fds[1]);
    char buf[256];
    int n = (int) read(fds[0], buf, sizeof buf - 1);
    buf[n > 0 ? n : 0] = 0;
    int st = 0;
    waitpid(pid, &st, 0);
    OUT("F default = ");
    for (int k = 0; k < n; k++)
        OUT("%02x", (unsigned char) buf[k]);
    OUT(" %d\n", WIFEXITED(st) ? WEXITSTATUS(st) : -WTERMSIG(st));
}

#ifdef SLATEOS_FIXTURE
static void returns_quietly(void)
{
}

/* A failure handler that returns, which it must not: glibc's
 * _obstack_newchunk goes on through the null chunk and faults, this
 * library's aborts before the macro can write past the chunk it has
 * (design-decisions 1162) -- not glibc's answer, so no line of its. 1 if
 * the child ended as abort() ends one: exit status 134 here, SIGABRT where
 * there are signals. */
static int r_case(void)
{
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        struct obstack ob;
        obstack_alloc_failed_handler = returns_quietly;
        obstack_specify_allocation(&ob, 0, 0, chunk, unchunk);
        fail_in = 1;
        obstack_grow(&ob, pattern, 5000);
        _exit(99);
    }
    int st = 0;
    if (pid < 0 || waitpid(pid, &st, 0) != pid)
        return 0;
    return (WIFEXITED(st) && WEXITSTATUS(st) == 134) || (WIFSIGNALED(st) && WTERMSIG(st) == SIGABRT);
}
#endif

int main(void)
{
    obstack_alloc_failed_handler_default = obstack_alloc_failed_handler;
    for (unsigned k = 0; k < sizeof pattern; k++)
        pattern[k] = (unsigned char) (k * 7 + 3);
    {
        struct obstack ob;
        obstack_specify_allocation(&ob, 0, 0, chunk, unchunk);
        OUT("D %ld %d %d %d\n", obstack_chunk_size(&ob), obstack_alignment_mask(&ob),
            obstack_exit_failure, obstack_alloc_failed_handler != NULL);
        obstack_free(&ob, NULL);
    }
    for (unsigned s = 0; s < sizeof SCEN / sizeof *SCEN; s++) {
#ifdef SLATEOS_FIXTURE
        run_ops(SCEN[s], OPS[s], NOPS[s]);
#else
        fflush(stdout);
        pid_t pid = fork();
        if (pid == 0) {
            run_ops(SCEN[s], OPS[s], NOPS[s]);
            fflush(stdout);
            _exit(0);
        }
        int st = 0;
        waitpid(pid, &st, 0);
        if (!WIFEXITED(st))
            OUT("S %s crash %d\n", SCEN[s], WTERMSIG(st));
#endif
    }
    obstack_alloc_failed_handler = obstack_alloc_failed_handler_default;
    for (unsigned k = 0; k < sizeof PF / sizeof *PF; k++)
        p_case(PF[k], PA[k]);
    nalloc = nfree = 0;
    f_case();
#ifdef SLATEOS_FIXTURE
    if (!r_case()) {
        printf("ctest-obstack: a failure handler that returned did not end the process "
               "as abort() does\n");
        return 2;
    }
    /* Each line against glibc's. */
    const char *at = text;
    for (unsigned k = 0; k < sizeof EXPECTED / sizeof *EXPECTED; k++) {
        size_t len = strlen(EXPECTED[k]);
        if (strncmp(at, EXPECTED[k], len) != 0 || at[len] != '\n') {
            const char *nl = strchr(at, '\n');
            printf("ctest-obstack: line %u is not glibc's:\n  want %s\n  got  %.*s\n", k + 1,
                   EXPECTED[k], nl ? (int) (nl - at) : (int) strlen(at), at);
            return 1;
        }
        at += len + 1;
    }
    if (*at) {
        printf("ctest-obstack: more lines than glibc's, from: %.60s\n", at);
        return 1;
    }
    printf("ctest-obstack: %u lines, each glibc's\n", (unsigned) (sizeof EXPECTED / sizeof *EXPECTED));
    return 42;
#else
    return 0;
#endif
}
'''


def c_str(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def tables(sc) -> list:
    lines = ["static void (*obstack_alloc_failed_handler_default)(void);"]
    lines.append("static const char *const SCEN[] = {" + ", ".join(c_str(n) for n, _ in sc) + "};")
    for i, (_n, ops) in enumerate(sc):
        lines.append(f"static const char *const OPS{i}[] = {{" + ", ".join(c_str(o) for o in ops)
                     + "};")
    lines.append("static const char *const *const OPS[] = {"
                 + ", ".join(f"OPS{i}" for i in range(len(sc))) + "};")
    lines.append("static const int NOPS[] = {" + ", ".join(str(len(ops)) for _n, ops in sc) + "};")
    lines.append("static const char *const PF[] = {" + ", ".join(c_str(f) for f, _a in PRINTF)
                 + "};")
    lines.append("static const char *const PA[] = {" + ", ".join(c_str(a) for _f, a in PRINTF)
                 + "};")
    return lines


def source(sc, expected=None) -> str:
    """The program: the harness's, or with `expected` (glibc's lines) the
    fixture's."""
    tabs = "\n".join(tables(sc))
    if expected is not None:
        tabs += "\nstatic const char *const EXPECTED[] = {\n" + ",\n".join(
            "    " + c_str(e) for e in expected) + "\n};"
    body = C_MAIN.replace("static long allocs[64];", tabs + "\n\nstatic long allocs[64];", 1)
    if expected is None:
        return "#define _GNU_SOURCE\n" + body
    return ("/* Generated by posix/tools/oracle/obstack_harness.py from glibc 2.39's\n"
            " * answers; do not edit. services/ctest-obstack/build.py says what it is. */\n"
            "#define _GNU_SOURCE\n#define SLATEOS_FIXTURE\n" + body)


def check_header() -> None:
    """posix/include/obstack.h's macros over glibc's own functions, in both
    the header's forms, must print glibc's oracle exactly. The fixture runs
    the GNU C form over this library's functions; the portable form, which no
    compiler that defines __GNUC__ (clang does) ever takes, is tested only
    here. glibc's own headers do not survive __GNUC__ undefined, so it is
    undefined only around <obstack.h>, after everything that includes."""
    header = POSIX_SRC.parent / "include" / "obstack.h"
    want = [line for line in OUT.read_text(encoding="utf-8").split("\n")
            if line and not line.startswith("#")]
    src = source(scenarios())
    if src.count("#include <obstack.h>\n") != 1 or "#include <unistd.h>\n" not in src:
        sys.exit("the program's includes are not where --header expects them")
    forms = {
        "gnu": src,
        "portable": src.replace("#include <obstack.h>\n", "", 1).replace(
            "#include <unistd.h>\n",
            "#include <unistd.h>\n#include <stddef.h>\n#pragma push_macro(\"__GNUC__\")\n"
            "#undef __GNUC__\n#include <obstack.h>\n#pragma pop_macro(\"__GNUC__\")\n", 1),
    }
    failed = False
    with workdir() as t:
        d = Path(t)
        (d / "inc").mkdir()
        (d / "inc" / "obstack.h").write_bytes(header.read_bytes())
        for name, text in forms.items():
            (d / f"{name}.c").write_text(text, encoding="utf-8", newline="\n")
            # Which form the build took, checked rather than assumed: the GNU
            # one is a statement expression.
            r = run(f"cd {wsl_path(d)} && gcc -E -dM -I inc {name}.c "
                    "| grep '^#define obstack_1grow('")
            if ("({" in r.stdout) != (name == "gnu"):
                sys.exit(f"{name}: the build did not take that form: {r.stdout.strip()}")
            r = run(f"cd {wsl_path(d)} && gcc -O1 -Wall -Wextra -Werror -I inc -o {name} "
                    f"{name}.c && LC_ALL=C ./{name}")
            if r.returncode != 0:
                sys.exit(f"{name}: the build or the run failed:\n{r.stderr[-3000:]}")
            got = [line for line in r.stdout.split("\n") if line]
            bad = [(w, g) for w, g in zip(want, got) if w != g]
            print(f"obstack.h, {name} form, over glibc's functions: {len(got)} lines, "
                  f"{len(bad) + abs(len(got) - len(want))} not glibc's")
            for w, g in bad[:8]:
                print(f"  want {w}\n  got  {g}")
            failed = failed or bool(bad) or len(got) != len(want)
    if failed:
        sys.exit(1)


def main() -> None:
    if sys.argv[1:] == ["--header"]:
        check_header()
        return
    sc = scenarios()
    with workdir() as t:
        d = Path(t)
        (d / "ob.c").write_text(source(sc), encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O1 -w -o ob ob.c && LC_ALL=C ./ob")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stderr[-3000:]}\n{r.stdout[-2000:]}")
    lines = [line for line in r.stdout.split("\n") if line]
    out = [
        "# glibc 2.39's obstacks, for posix/src/obstack.rs and posix/include/obstack.h.",
        "# Generated by posix/tools/oracle/obstack_harness.py; do not edit.",
    ] + lines
    OUT.write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")
    FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    FIXTURE.write_text(source(sc, lines), encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {len(lines)} lines; {FIXTURE.parent.name}/main.c")


if __name__ == "__main__":
    main()
