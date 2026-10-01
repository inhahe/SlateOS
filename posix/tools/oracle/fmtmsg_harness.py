"""glibc 2.39's fmtmsg and addseverity, as the oracle for posix/src/fmtmsg.rs.

    python posix/tools/oracle/fmtmsg_harness.py   # writes posix/src/fmtmsg_oracle.txt

Each case runs in a child process of its own, because glibc reads MSGVERB and
SEV_LEVEL once, at a process's first fmtmsg: the child sets them, makes the
case's addseverity calls, calls fmtmsg once with its standard error on a
pipe, and reports what each call returned. One line a case:

    <case> = <fmtmsg's return> <addseverity's returns, or -> <standard error>

where the case is `verb=<MSGVERB> sev=<SEV_LEVEL> add=<calls> class=<n>
label=<s> severity=<n> text=<s> action=<s> tag=<s>`, `\\x` standing for a
NULL or unset value and `""` for an empty one; every string is escaped with
`\\xNN` for a backslash, a space, a double quote and anything outside
printable ASCII, and standard error likewise (`\\x` when nothing was
written). A console message is not
recorded: WSL gives a user no /dev/console, so glibc answers MM_NOCON there,
and its text is the standard error's without MSGVERB's filtering.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "fmtmsg_oracle.txt"
DEVIATIONS = POSIX_SRC / "fmtmsg_deviations.txt"

MM_HARD, MM_SOFT, MM_FIRM, MM_APPL, MM_UTIL, MM_OPSYS = 1, 2, 4, 8, 16, 32
MM_RECOVER, MM_NRECOV, MM_PRINT, MM_CONSOLE = 64, 128, 256, 512

FULL = ("UX:cat", 1, "can't open", "refer to manual", "UX:cat:001")


def cases():
    """(verb, sev_level, adds, class, label, severity, text, action, tag,
    stderr closed)."""
    out = []
    # The label's two fields: up to 10 bytes, a colon, up to 14.
    for label in ["UX:cat", "a:b", ":", "x:", ":y", "nocolon", "", "1234567890:12345678901234",
                  "12345678901:x", "x:123456789012345", "a:b:c", "::"]:
        out.append((None, None, [], MM_PRINT, label, 2, "t", None, None))
    # Every combination of the parts, for the standard severities and one
    # nobody defined, on each channel and none.
    for cls in [MM_PRINT, MM_CONSOLE, MM_PRINT | MM_CONSOLE, 0,
                MM_PRINT | MM_SOFT | MM_APPL | MM_RECOVER]:
        for severity in [0, 1, 2, 3, 4, 99, -1]:
            for label in [None, "UX:cat"]:
                for text in [None, "text"]:
                    for action in [None, "act"]:
                        for tag in [None, "tag"]:
                            out.append((None, None, [], cls, label, severity, text, action, tag))
    # MSGVERB: which parts go to standard error.
    for verb in ["label:severity:text:action:tag", "text", "label:text", "action:tag",
                 "severity:tag", "tag:label", "bogus", "", "text:bogus", ":text", "text::tag",
                 "text:", "TEXT", "label:label"]:
        out.append((verb, None, [], MM_PRINT, *FULL))
        out.append((verb, None, [], MM_PRINT, None, 3, "t", None, "g"))
        out.append((verb, None, [], MM_PRINT | MM_CONSOLE, *FULL))
    # SEV_LEVEL: further severities, for standard error only.
    for sev in ["five,5,FIVE", "a,6,SIX:b,7,SEVEN", "bad", "x,4,FOUR", "y,5,", ",5,NONAME",
                "z,abc,ZED", "w,5,FIVE,extra", "v,5,FIVE:v,5,AGAIN", ":five,5,FIVE:",
                "big,2147483647,MAX", "neg,-5,NEG", "five,05,OCT"]:
        for severity in [2, 4, 5, 6, 7, 2147483647, -5]:
            out.append((None, sev, [], MM_PRINT, "UX:cat", severity, "t", None, None))
        out.append((None, sev, [], MM_CONSOLE, "UX:cat", 5, "t", None, None))
    # addseverity, alone and against SEV_LEVEL.
    adds = [
        [(5, "FIVE")], [(5, "FIVE"), (5, None)], [(4, "X")], [(5, None)], [(6, "")],
        [(-1, "NEG")], [(5, "A"), (5, "B")], [(0, "ZERO")], [(1, "HALTED")], [(5, "FIVE"), (6, "SIX")],
        [(2147483647, "MAX")],
    ]
    for add in adds:
        for severity in [0, 1, 4, 5, 6, -1, 2147483647]:
            out.append((None, None, add, MM_PRINT, "UX:cat", severity, "t", None, None))
    for add in [[(5, "OVER")], [(5, None)]]:
        for severity in [5, 6]:
            out.append((None, "five,5,FIVE:six,6,SIX", add, MM_PRINT, "UX:cat", severity, "t",
                        None, None))
    out.append((None, None, [(5, "FIVE")], MM_CONSOLE, "UX:cat", 5, "t", None, None))
    return [c + (False,) for c in out] + [
        # Standard error closed: its message cannot be written.
        (None, None, [], cls, *FULL, True)
        for cls in [MM_PRINT, MM_PRINT | MM_CONSOLE, MM_CONSOLE, 0]
    ]


def c_string(s) -> str:
    if s is None:
        return "NULL"
    return '"' + "".join(f"\\{ord(c):03o}" if c in '"\\' or not " " <= c <= "~" else c
                         for c in s) + '"'


def escape(s) -> str:
    """A case's string: `\\x` for NULL (or unset), `""` for the empty one."""
    if s is None:
        return "\\x"
    if s == "":
        return '""'
    return "".join(f"\\x{ord(c):02x}" if c in "\\ \"" or not "!" <= c <= "~" else c for c in s)


def describe(case) -> str:
    verb, sev, adds, cls, label, severity, text, action, tag, closed = case
    add = ",".join(f"{n}/{escape(s)}" for n, s in adds) or "\\x"
    return (f"verb={escape(verb)} sev={escape(sev)} add={add} class={cls} label={escape(label)} "
            f"severity={severity} text={escape(text)} action={escape(action)} tag={escape(tag)}"
            + (" stderr=closed" if closed else ""))


PROGRAM = r'''
#define _GNU_SOURCE
#include <fmtmsg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

struct add { int sev; const char *s; };
struct c {
    const char *verb, *sevlevel;
    int nadd; struct add adds[4];
    long cls; const char *label; int severity; const char *text, *action, *tag;
    int closed;
};

static void esc(const unsigned char *s, size_t n)
{
    if (n == 0) { fputs("\\x", stdout); return; }
    for (size_t i = 0; i < n; i++) {
        unsigned c = s[i];
        if (c == '\\' || c <= ' ' || c > '~') printf("\\x%02x", c); else putchar(c);
    }
}

static void one(const struct c *k)
{
    int err[2], res[2];
    if (pipe(err) || pipe(res)) exit(3);
    fflush(stdout);
    pid_t p = fork();
    if (p == 0) {
        if (k->verb) setenv("MSGVERB", k->verb, 1); else unsetenv("MSGVERB");
        if (k->sevlevel) setenv("SEV_LEVEL", k->sevlevel, 1); else unsetenv("SEV_LEVEL");
        if (k->closed) close(2); else dup2(err[1], 2);
        close(err[0]); close(res[0]);
        int got[5];
        for (int i = 0; i < k->nadd; i++) got[i] = addseverity(k->adds[i].sev, k->adds[i].s);
        int r = fmtmsg(k->cls, k->label, k->severity, k->text, k->action, k->tag);
        fflush(stderr);
        dprintf(res[1], "%d", r);
        for (int i = 0; i < k->nadd; i++) dprintf(res[1], "%c%d", i ? ',' : ' ', got[i]);
        if (!k->nadd) dprintf(res[1], " -");
        _exit(0);
    }
    close(err[1]); close(res[1]);
    char out[8192], rb[256];
    size_t n = 0; ssize_t m;
    while ((m = read(err[0], out + n, sizeof out - n)) > 0) n += (size_t)m;
    size_t rn = 0;
    while ((m = read(res[0], rb + rn, sizeof rb - 1 - rn)) > 0) rn += (size_t)m;
    rb[rn] = 0;
    waitpid(p, NULL, 0);
    close(err[0]); close(res[0]);
    printf(" = %s ", rb);
    esc((const unsigned char *)out, n);
    putchar('\n');
}

static const struct c CASES[] = {
@CASES@
};

int main(void)
{
    for (size_t i = 0; i < sizeof CASES / sizeof CASES[0]; i++) {
        printf("%zu", i);
        one(&CASES[i]);
    }
    return 0;
}
'''


def main() -> None:
    all_cases = cases()
    rows = []
    for verb, sev, adds, cls, label, severity, text, action, tag, closed in all_cases:
        a = ", ".join(f"{{{n}, {c_string(s)}}}" for n, s in adds) or "{0, NULL}"
        rows.append(f"    {{{c_string(verb)}, {c_string(sev)}, {len(adds)}, {{{a}}}, {cls}L, "
                    f"{c_string(label)}, {severity}, {c_string(text)}, {c_string(action)}, "
                    f"{c_string(tag)}, {int(closed)}}},")
    program = PROGRAM.replace("@CASES@", "\n".join(rows))
    with workdir() as t:
        d = Path(t)
        (d / "f.c").write_text(program, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O0 -w -o f f.c && LC_ALL=C ./f")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
    lines = [
        "# glibc 2.39's fmtmsg and addseverity, each case in a process of its own, for",
        "# posix/src/fmtmsg.rs: <case> = <fmtmsg's return> <addseverity's returns, or -> <stderr>.",
        "# Generated by posix/tools/oracle/fmtmsg_harness.py; do not edit.",
    ]
    for line in r.stdout.splitlines():
        i, rest = line.split(" ", 1)
        lines.append(describe(all_cases[int(i)]) + " " + rest)
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {len(lines) - 3} cases")

    import fmtmsg_model  # beside this file; see its docstring

    dev = [
        "# Every case of fmtmsg_oracle.txt that posix/src/fmtmsg.rs answers otherwise than glibc",
        "# 2.39 -- design-decisions section 1150 says why -- as fmtmsg_model.py answers it: a",
        "# `glibc` line, the oracle's, and a `here` line, this library's, in the oracle's format.",
        "# Generated by posix/tools/oracle/fmtmsg_harness.py; do not edit.",
    ]
    for line in lines[3:]:
        fields, _ = fmtmsg_model.parse(line)
        ours = line.partition(" = ")[0] + " = " + fmtmsg_model.answer(fields)
        if ours != line:
            dev += [f"glibc {line}", f"here  {ours}"]
    DEVIATIONS.write_text("\n".join(dev) + "\n", encoding="utf-8", newline="\n")
    print(f"{DEVIATIONS.name}: {(len(dev) - 4) // 2} cases where this library's answer is not glibc's")


if __name__ == "__main__":
    main()
