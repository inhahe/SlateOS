"""glibc 2.39's option parsers, `getopt`, `getopt_long` and
`getopt_long_only`, as the oracle for posix/src/getopt.rs's:

    python posix/tools/oracle/getopt_harness.py   # writes posix/src/getopt_oracle.txt

One line a case, each case a fresh parse (`optind = 0`, glibc's restart):

    <how> <table> <posixly> <opterr> <optstring> <argv...> = <calls> | <argv after> | <stderr>

`<how>` is `s` (getopt), `l` (getopt_long) or `o` (getopt_long_only);
`<table>` names one of the long-option tables below (`-` for none);
`<posixly>` is 1 when POSIXLY_CORRECT is in the environment; `<opterr>` is
what `opterr` is set to first. `<calls>` are the calls in order, until the
one that answers -1, each `<rc>,<optind>,<optopt>,<optarg>,<longindex>,<flag>`
-- `<optarg>` `-` for NULL, else its text and `@<n>` when it points into
argv[n] (else `@?`); `<longindex>` -1 when the call left it so; `<flag>`
the flag variable the `flag` table entries set. `<argv after>` is argv
once parsing is done (glibc permutes it), and `<stderr>` everything the
parse wrote there. Text is written as posix/src/resolv.rs's tests write it
(a byte from `!` to `~` as itself but `\\`, any other as `\\xHH`, the empty
text `\\x`).
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "getopt_oracle.txt"

# The long-option tables: (name, has_arg, flag?, val). `flag` entries set
# the harness's flag variable to `val` and answer 0.
TABLES = {
    "t1": [
        ("verbose", 0, False, ord("v")), ("version", 0, False, ord("V")),
        ("file", 1, False, ord("f")), ("opt", 2, False, ord("o")),
        ("flag", 0, True, 42), ("nofile", 0, False, ord("n")),
        ("verbatim", 0, False, ord("v")), ("color", 2, False, 300),
        ("colour", 2, False, 300), ("x", 0, False, ord("x")),
    ],
    # Prefixes of one another, and one name that is a prefix of another.
    "t2": [
        ("aa", 0, False, ord("1")), ("aab", 1, False, ord("2")), ("ab", 2, False, ord("3")),
        ("b", 1, False, ord("4")), ("bee", 0, True, 7), ("a", 0, False, ord("5")),
    ],
}

OPTSTRINGS = ["ab:c::", ":ab:c::", "+ab:c::", "-ab:c::", "-:ab:", "+:a", "abW;", "W;ab:",
              "vVf:o::", "", ":", "a:b"]

ARGVS = [
    ["-a"], ["-ab", "x"], ["-bx"], ["-b"], ["-c"], ["-cx"], ["-c", "x"], ["-abc"],
    ["file", "-a"], ["-a", "file", "-b", "arg", "file2", "--", "-c", "x"],
    ["-a", "--", "-b"], ["-", "-a"], ["-z"], ["-az", "-b"], ["--"], ["-a", "--"],
    ["f1", "f2", "-a", "f3", "-bv", "f4"], ["-b", "--", "x"], ["-b", "-a"],
    ["-W", "verbose"], ["-Wverbose"], ["-W"], ["-W", "file=x"], ["-Wfile", "x"], ["-W", "nope"],
    ["-W", "ver"], ["-Wa"],
    ["--foo"], ["--verbose"], ["--verb"], ["--ver"], ["--vers"], ["--file=x"], ["--file", "x"],
    ["--file"], ["--fi", "x"], ["--nofile=x"], ["--opt"], ["--opt=v"], ["--opt", "v"],
    ["--flag"], ["--fl"], ["--col"], ["--col=red"], ["--co"], ["--x"], ["--="], ["---x"],
    ["--verbose=1"], ["--version", "rest", "-a"],
    ["-verbose"], ["-v"], ["-ver"], ["-vf", "x"], ["-file=x"], ["-fi", "x"], ["-x"], ["-xa"],
    ["-a", "-verbose", "-b", "y"],
    ["--aa"], ["--a"], ["--aab", "q"], ["--aab"], ["--ab"], ["--ab=1"], ["--b", "z"], ["--be"],
    ["--bee"], ["-aa"], ["-a"], ["-ab"], ["-b", "z"], ["-bee"], ["-bq"],
    ["-a", "-z", "file", "--verbose", "-b"], [], ["plain"], ["-", "--"],
]

# (how, table, posixly, opterr, the optstrings asked of it)
MODES = [
    ("s", "-", 0, 1, OPTSTRINGS),
    ("s", "-", 1, 1, ["ab:c::", "-ab:c::", ":ab:c::", "+ab:c::"]),
    ("s", "-", 0, 0, ["ab:c::", "abW;", "+:a"]),
    ("l", "t1", 0, 1, ["ab:c::", ":ab:c::", "+ab:c::", "-ab:c::", "vVf:o::", "W;ab:", ""]),
    ("l", "t1", 1, 1, ["ab:c::", "-ab:c::"]),
    ("l", "t1", 0, 0, ["ab:c::", "W;ab:"]),
    ("o", "t1", 0, 1, ["ab:c::", ":ab:c::", "vVf:o::", "W;ab:", ""]),
    ("l", "t2", 0, 1, ["ab:c::", ":a", "-b:"]),
    ("o", "t2", 0, 1, ["ab:c::", ":a", "-b:"]),
]


def token(b: bytes) -> str:
    if not b:
        return "\\x"
    return "".join(chr(c) if 0x21 <= c <= 0x7E and c != 0x5C else f"\\x{c:02x}" for c in b)


def c_str(b: bytes) -> str:
    return '"' + "".join(f"\\x{c:02x}" for c in b) + '"'


def c_escape(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"')


def c_program(cases) -> str:
    L = [r'''#define _GNU_SOURCE
#include <getopt.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int flagvar;

/* A text as the lines write it. */
static void text(const char *s, size_t n)
{
    if (n == 0) {
        fputs("\\x", stdout);
        return;
    }
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char) s[i];
        if (c >= 0x21 && c <= 0x7e && c != '\\')
            putchar(c);
        else
            printf("\\x%02x", c);
    }
}
''']
    for name, entries in TABLES.items():
        L.append(f"static struct option {name}[] = {{")
        for (n, has, flag, val) in entries:
            L.append(f'    {{"{n}", {has}, {"&flagvar" if flag else "NULL"}, {val}}},')
        L.append("    {NULL, 0, NULL, 0},\n};")
    L.append(r'''
static void one(const char *head, char how, struct option *table, int posixly, int err,
                const char *optstring, int argc, char **argv)
{
    char *orig[64];
    for (int i = 0; i < argc; i++)
        orig[i] = argv[i];
    if (posixly)
        setenv("POSIXLY_CORRECT", "1", 1);
    else
        unsetenv("POSIXLY_CORRECT");
    char *errbuf = NULL;
    size_t errlen = 0;
    FILE *saved = stderr;
    stderr = open_memstream(&errbuf, &errlen);
    optind = 0;
    opterr = err;
    optopt = 1234;
    optarg = NULL;
    flagvar = 0;
    printf("%s =", head);
    for (int calls = 0; calls < 40; calls++) {
        int li = -1;
        int rc;
        if (how == 's')
            rc = getopt(argc, argv, optstring);
        else if (how == 'l')
            rc = getopt_long(argc, argv, optstring, table, &li);
        else
            rc = getopt_long_only(argc, argv, optstring, table, &li);
        printf(" %d,%d,%d,", rc, optind, optopt);
        if (optarg == NULL) {
            putchar('-');
        } else {
            text(optarg, strlen(optarg));
            int at = -1;
            for (int i = 0; i < argc; i++)
                if (optarg >= orig[i] && optarg <= orig[i] + strlen(orig[i]))
                    at = i;
            if (at >= 0)
                printf("@%d", at);
            else
                fputs("@?", stdout);
        }
        printf(",%d,%d", li, flagvar);
        if (rc == -1)
            break;
    }
    fputs(" |", stdout);
    for (int i = 0; i < argc; i++) {
        putchar(' ');
        text(argv[i], strlen(argv[i]));
    }
    fclose(stderr);
    stderr = saved;
    fputs(" | ", stdout);
    text(errbuf, errlen);
    free(errbuf);
    putchar('\n');
}

int main(void)
{
''')
    for i, (how, table, posixly, err, optstring, argv) in enumerate(cases):
        args = ["prog"] + argv
        head = " ".join([how, table, str(posixly), str(err), token(optstring.encode())]
                        + [token(a.encode()) for a in argv])
        L.append("  {")
        L.append(f"    static char *argv{i}[] = {{{', '.join(c_str(a.encode()) for a in args)}, NULL}};")
        # String literals are not writable; argv's strings are, as a
        # program's are.
        L.append(f"    char *copy[{len(args) + 1}];")
        L.append(f"    for (int k = 0; k < {len(args)}; k++) copy[k] = strdup(argv{i}[k]);")
        L.append(f"    copy[{len(args)}] = NULL;")
        tab = "NULL" if table == "-" else table
        L.append(f'    one("{c_escape(head)}", \'{how}\', {tab}, {posixly}, {err}, {c_str(optstring.encode())}, '
                 f'{len(args)}, copy);')
        L.append("  }")
    L.append("  return 0;\n}")
    return "\n".join(L) + "\n"


def cases():
    out = []
    for how, table, posixly, err, optstrings in MODES:
        for optstring in optstrings:
            for argv in ARGVS:
                # Long-option argvs only where long options are parsed, and
                # the short-only modes only over the short argvs.
                if how == "s" and any(a.startswith("--") and len(a) > 2 for a in argv):
                    continue
                out.append((how, table, posixly, err, optstring, argv))
    return out


def main() -> int:
    cs = cases()
    with workdir() as d:
        c = Path(d) / "getopt.c"
        c.write_text(c_program(cs), encoding="utf-8", newline="\n")
        exe = wsl_path(Path(d) / "getopt")
        r = run(f"gcc -O1 -Wall -Werror -o {exe} {wsl_path(c)} && {exe}")
    if r.returncode != 0:
        sys.stderr.write(r.stdout[-4000:] + r.stderr[-4000:])
        return 1
    header = ("# glibc 2.39's getopt, getopt_long and getopt_long_only "
              "(posix/tools/oracle/getopt_harness.py):\n"
              "# one line a case -- see the harness for the forms.\n")
    OUT.write_text(header + r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT} ({len(r.stdout.splitlines())} cases)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
