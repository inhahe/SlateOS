"""glibc 2.39's <aliases.h> -- setaliasent, getaliasent, getaliasent_r,
getaliasbyname, getaliasbyname_r and endaliasent over /etc/aliases -- as the
oracle for posix/src/aliases.rs.

    python posix/tools/oracle/aliases_harness.py   # writes posix/src/aliases_oracle.txt

/etc/aliases is a fixed path, so the harness runs, statically linked, in a
user and mount namespace of its own (`unshare -rm`) with a tmpfs over /etc:

- `files`: over the test file below, which names an `:include:` file, also
  under /etc;
- `none`: with no file;
- `case N`: over each of CASES, a file with an empty member -- two commas
  together, or a line that starts with one -- enumerated under a five-second
  timeout, since glibc's parser loops on one for good (`case N = loops`):
  it copies a member up to the next comma, and steps past the comma only
  when what it copied was not empty. Its `:include:` branch, which steps
  past the comma either way, skips an empty member; that is what
  posix/src/aliases.rs does everywhere.

One line a probe, `<run> <probe> = <what it gave>`: an entry as
`name|local|[member,...]` (each member quoted, as white space leaves it);
an _r form's number and entry; `NULL` and errno's name (`kept`: as the
probe left it). The inputs head the file, escaped: `input aliases = ...`,
`input alias.list = ...`, `input case N = ...`.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "aliases_oracle.txt"

ALIASES = (
    "# /etc/aliases\n"
    "postmaster: root\n"
    "root:\tadmin, \"quoted user\" , ops,\n"
    "  second-line-member\n"
    "\tthird@host\n"
    "MAILER-DAEMON: postmaster\n"
    "nobody: /dev/null\n"
    "empty:\n"
    "spaced : a\n"
    "Upper: b\n"
    "dup: first\n"
    "dup: second\n"
    "inc: :include:/etc/alias.list, after\n"
    "incmissing: :include:/etc/no-such-list, kept\n"
    "noname\n"
    ": nothing\n"
    "commented: x # y, z\n"
    "hash#name: v\n"
    "trailing: a, b ,\n"
    "tabbed:\ta\t,\tb\t\n"
    "   indented: i\n"
    "cont: one,\n"
    "\n"
    "  after-blank\n"
    "before-blank: a\n"
    "\n"
    "  indented-after-blank: b\n"
    "\r carriage: c\n"
    "last: final"
)

LIST = "l1, l2\nl3 # comment, not-a-member\n\n  l4 ,l5\n,, l6 ,\n"

# Each has an empty member, which glibc's parser loops on.
CASES = [
    "a: x,,y\nb: z\n",
    "a: ,x\nb: z\n",
    "a: x, ,y\nb: z\n",
    "a: x\n  ,y\nb: z\n",
]

PROGRAM = r'''
#define _GNU_SOURCE
#include <aliases.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>

static const char *en(int e)
{
    switch (e) {
    case 0: return "0";
    case 12345: return "kept";
    case ENOENT: return "ENOENT";
    case ERANGE: return "ERANGE";
    case EINVAL: return "EINVAL";
    default: { static char b[16]; snprintf(b, sizeof b, "e%d", e); return b; }
    }
}

static void entry(const struct aliasent *a)
{
    printf("%s|%d|[", a->alias_name, a->alias_local);
    for (size_t i = 0; i < a->alias_members_len; i++)
        printf("%s\"%s\"", i ? "," : "", a->alias_members[i]);
    printf("]");
}

static void show(const char *run, const char *what, struct aliasent *a)
{
    printf("%s %s = ", run, what);
    if (a) entry(a); else printf("NULL");
    printf(" errno=%s\n", en(errno));
}

static void enumerate(const char *run)
{
    char what[64];
    for (int n = 0; n < 64; n++) {
        snprintf(what, sizeof what, "getaliasent #%d", n);
        errno = 12345;
        struct aliasent *a = getaliasent();
        show(run, what, a);
        if (!a) break;
    }
}

int main(int argc, char **argv)
{
    const char *run = argc > 1 ? argv[1] : "files";
    char what[128], buf[4096];
    setvbuf(stdout, NULL, _IONBF, 0);
    if (strncmp(run, "case ", 5) == 0) {
        enumerate(run);
        return 0;
    }
    enumerate(run);
    setaliasent();
    errno = 12345;
    show(run, "getaliasent after setaliasent", getaliasent());
    errno = 12345;
    show(run, "getaliasent after that", getaliasent());
    endaliasent();
    errno = 12345;
    show(run, "getaliasent after endaliasent", getaliasent());
    endaliasent();
    static const char *NAMES[] = { "postmaster", "POSTMASTER", "root", "mailer-daemon",
        "nobody", "empty", "spaced", "spaced ", "upper", "Upper", "dup", "inc", "incmissing",
        "noname", "", "commented", "hash", "hash#name", "trailing", "tabbed", "indented",
        "cont", "after-blank", "before-blank", "indented-after-blank", "carriage", "last",
        "missing", "second-line-member", "admin" };
    for (size_t i = 0; i < sizeof NAMES / sizeof *NAMES; i++) {
        snprintf(what, sizeof what, "getaliasbyname(%s)", NAMES[i]);
        errno = 12345;
        show(run, what, getaliasbyname(NAMES[i]));
    }
    /* the _r forms, at buffer sizes around an entry's need */
    for (size_t size = 0; size <= 200; size += 8) {
        struct aliasent a, *res = (struct aliasent *)1;
        errno = 12345;
        int rc = getaliasbyname_r("root", &a, buf, size, &res);
        printf("%s getaliasbyname_r(root, %zu) = %s ", run, size, en(rc));
        if (rc == 0 && res == &a) entry(&a);
        else printf("%s", res == NULL ? "NULL" : res == (struct aliasent *)1 ? "untouched" : "other");
        printf(" errno=%s\n", en(errno));
    }
    {
        struct aliasent a, *res = (struct aliasent *)1;
        errno = 12345;
        int rc = getaliasbyname_r("missing", &a, buf, sizeof buf, &res);
        printf("%s getaliasbyname_r(missing) = %s %s errno=%s\n", run, en(rc),
               res == NULL ? "NULL" : "other", en(errno));
    }
    setaliasent();
    for (int n = 0; n < 64; n++) {
        struct aliasent a, *res = (struct aliasent *)1;
        errno = 12345;
        int rc = getaliasent_r(&a, buf, n == 1 ? 16 : sizeof buf, &res);
        printf("%s getaliasent_r #%d = %s ", run, n, en(rc));
        if (rc == 0 && res == &a) entry(&a);
        else printf("%s", res == NULL ? "NULL" : "other");
        printf(" errno=%s\n", en(errno));
        if (rc != 0 && rc != ERANGE) break;
    }
    endaliasent();
    return 0;
}
'''


def escape(text: str) -> str:
    """`text` with \\, newline and tab escaped, for the tests to read back."""
    return text.replace("\\", "\\\\").replace("\n", "\\n").replace("\t", "\\t")


def main() -> None:
    with workdir() as t:
        d = Path(t)
        (d / "al.c").write_text(PROGRAM, encoding="utf-8", newline="\n")
        (d / "aliases").write_text(ALIASES, encoding="utf-8", newline="\n")
        (d / "alias.list").write_text(LIST, encoding="utf-8", newline="\n")
        for i, case in enumerate(CASES):
            (d / f"case{i}").write_text(case, encoding="utf-8", newline="\n")
        w = wsl_path(d)

        def namespace(label: str, copy: str, guard: str = "") -> str:
            """The probe run as `label`, after `copy` fills /etc."""
            return (f"unshare -rm sh -c 'mount -t tmpfs none /etc && "
                    f"cp /tmp/nsswitch.conf /etc/ && {copy}{guard}/tmp/alprobe \"{label}\"'")

        cases = "".join(
            namespace(f"case {i}", f"cp /tmp/case{i} /etc/aliases && ", "timeout 5 ")
            + f" || echo 'case {i} = loops'; "
            for i in range(len(CASES)))
        # The build's own warning (NSS in a static program) is not output; a
        # failure is, on stderr. /etc/nsswitch.conf says `aliases: files`,
        # as a system's does, so that only the files differ between runs.
        script = (
            f"set -e; cp {w}/al.c {w}/aliases {w}/alias.list {w}/case* /tmp/ && cd /tmp && "
            "{ gcc -static -O0 -Wall -Werror -o alprobe al.c > gcc.log 2>&1 "
            "|| { cat gcc.log >&2; exit 1; }; }; "
            "printf 'aliases: files\\n' > /tmp/nsswitch.conf; "
            + namespace("files", "cp /tmp/aliases /tmp/alias.list /etc/ && ", "timeout 60 ")
            + " || echo 'files = loops'; "
            + namespace("none", "", "timeout 60 ") + " || echo 'none = loops'; "
            + cases
        )
        r = run(script)
        if r.returncode != 0 or "none getaliasent" not in r.stdout:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
        body = r.stdout
    head = ("# glibc 2.39's <aliases.h>, for posix/src/aliases.rs. Generated by\n"
            "# posix/tools/oracle/aliases_harness.py; do not edit.\n")
    inputs = (f"input aliases = {escape(ALIASES)}\ninput alias.list = {escape(LIST)}\n"
              + "".join(f"input case {i} = {escape(c)}\n" for i, c in enumerate(CASES)))
    OUT.write_text(head + inputs + body, encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {body.count(chr(10))} lines")


if __name__ == "__main__":
    main()
