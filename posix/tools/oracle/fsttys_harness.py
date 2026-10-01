"""glibc 2.39's <fstab.h> and <ttyent.h> -- getfsent, getfsspec, getfsfile,
setfsent, endfsent over /etc/fstab; getttyent, getttynam, setttyent,
endttyent over /etc/ttys -- as the oracle for posix/src/fstab.rs and
posix/src/ttyent.rs.

    python posix/tools/oracle/fsttys_harness.py   # writes posix/src/fsttys_oracle.txt

Both read a fixed path, so the harness runs, statically linked, in a user
and mount namespace of its own (`unshare -rm`) with a tmpfs over /etc
holding the two files below -- and again with /etc empty, for no file at
all. One line a probe, `<probe> = <what it gave>`: an fstab entry as
`spec|file|vfstype|mntops|type|freq|passno`, a ttyent as
`name|getty|type|status|window|comment`, `(null)` for a NULL string.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "fsttys_oracle.txt"

FSTAB = (
    "# comment\n"
    "/dev/sda1  /  ext4  rw,noatime  0  1\n"
    "/dev/sda2 /home ext4 defaults 0 2\n"
    "/dev/sdb1 /mnt/ro ext4 ro 1 2\n"
    "\n"
    "/swapfile none swap sw 0 0\n"
    "proc /proc proc rq 0 0\n"
    "/dev/x /x xfs xx 0 0\n"
    "/dev/y /y xfs noauto\n"
    "/dev/z /z\\040dir ext4 rw,ro 0 0\n"
    "nothing\n"
    "tmpfs /tmp tmpfs rw 3\n"
    " /dev/sda3\t/var\text4\tro,rw\t9 9\n"
    "/dev/w /w ext4 norw,rwx 0 0\n"
    "/dev/v /v ext4 sw=1,rq 5 6 extra\n"
)

TTYS = (
    "# /etc/ttys\n"
    "console\t\"/usr/libexec/getty std.9600\"\tvt100\ton  secure\n"
    "ttyv0\t\"/usr/libexec/getty Pc\"\t\tcons25\ton  secure window=\"/usr/X11R6/bin/xterm -e\"\n"
    "ttyv1\tnone\t\t\tnetwork\toff\n"
    "ttyd0 \"/usr/libexec/getty \\\"quoted\\\"\" dialup off # a comment\n"
    "ttyd1\n"
    "ttyd2 getty\n"
    "ttyd3 getty vt100\n"
    "   ttyp0 none network # comment right after\n"
    "ttyp1 none network on window=x\n"
    "ttyp2 none network bogus on\n"
    "ttyp3 \"unterminated\n"
    "ttyp4 none network secure#tight comment\n"
    "ttyp5 none network off on secure off\n"
    "ttyp6 none network window= on\n"
    "ttyp7 none network onsecure\n"
    "ttyp8 none network on\tsecure\t#\ttabbed\n"
    "\n"
    "\t\n"
    "#ttyx0 none network\n"
    "ttylong " + "x" * 100 + "\n"
    "ttyq0 none network on\n"
    "ttyq1 \"a b\"c d\n"
    "ttyq2 a\"b c\"d e\n"
    "ttyq3 none network window=\"w w\" secure\n"
    "ttyq4 none network #\n"
)

PROGRAM = r'''
#define _GNU_SOURCE
#include <errno.h>
#include <fstab.h>
#include <stdio.h>
#include <string.h>
#include <ttyent.h>

static void str(const char *s)
{
    if (!s) { printf("(null)"); return; }
    for (; *s; s++) {
        unsigned char c = (unsigned char)*s;
        if (c == '\\') printf("\\\\");
        else if (c == '\n') printf("\\n");
        else if (c == '\t') printf("\\t");
        else if (c < 0x20 || c >= 0x7f) printf("\\x%02x", c);
        else putchar(c);
    }
}

static void fs(const char *what, struct fstab *f)
{
    printf("%s = ", what);
    if (!f) { printf("NULL\n"); return; }
    str(f->fs_spec); printf("|"); str(f->fs_file); printf("|"); str(f->fs_vfstype);
    printf("|"); str(f->fs_mntops); printf("|"); str(f->fs_type);
    printf("|%d|%d\n", f->fs_freq, f->fs_passno);
}

static void tty(const char *what, struct ttyent *t)
{
    printf("%s = ", what);
    if (!t) { printf("NULL\n"); return; }
    str(t->ty_name); printf("|"); str(t->ty_getty); printf("|"); str(t->ty_type);
    printf("|%d|", t->ty_status); str(t->ty_window); printf("|"); str(t->ty_comment);
    printf("\n");
}

int main(int argc, char **argv)
{
    const char *run = argc > 1 ? argv[1] : "files";
    char what[128];
    setvbuf(stdout, NULL, _IONBF, 0);

    for (int n = 0; n < 20; n++) {
        snprintf(what, sizeof what, "%s getfsent #%d", run, n);
        struct fstab *f = getfsent();
        fs(what, f);
        if (!f) break;
    }
    printf("%s setfsent = %d\n", run, setfsent());
    snprintf(what, sizeof what, "%s getfsent after setfsent", run);
    fs(what, getfsent());
    static const char *SPECS[] = { "/dev/sda2", "/dev/sdb1", "none", "nothing", "/dev/z",
                                    "tmpfs", "/dev/v", "missing", "" };
    for (size_t i = 0; i < sizeof SPECS / sizeof *SPECS; i++) {
        snprintf(what, sizeof what, "%s getfsspec(%s)", run, SPECS[i]);
        fs(what, getfsspec(SPECS[i]));
        snprintf(what, sizeof what, "%s getfsent after getfsspec(%s)", run, SPECS[i]);
        fs(what, getfsent());
    }
    static const char *FILES[] = { "/home", "/", "/z dir", "/z\\040dir", "none", "/var",
                                    "missing" };
    for (size_t i = 0; i < sizeof FILES / sizeof *FILES; i++) {
        snprintf(what, sizeof what, "%s getfsfile(%s)", run, FILES[i]);
        fs(what, getfsfile(FILES[i]));
    }
    endfsent();
    snprintf(what, sizeof what, "%s getfsent after endfsent", run);
    fs(what, getfsent());
    endfsent();
    endfsent();
    printf("%s endfsent twice = ok\n", run);

    for (int n = 0; n < 40; n++) {
        snprintf(what, sizeof what, "%s getttyent #%d", run, n);
        struct ttyent *t = getttyent();
        tty(what, t);
        if (!t) break;
    }
    snprintf(what, sizeof what, "%s getttyent at the end again", run);
    tty(what, getttyent());
    printf("%s setttyent = %d\n", run, setttyent());
    snprintf(what, sizeof what, "%s getttyent after setttyent", run);
    tty(what, getttyent());
    static const char *NAMES[] = { "ttyv1", "ttyd0", "ttyq0", "ttylong", "ttyx0", "missing",
                                   "ttyp3", "" };
    for (size_t i = 0; i < sizeof NAMES / sizeof *NAMES; i++) {
        snprintf(what, sizeof what, "%s getttynam(%s)", run, NAMES[i]);
        tty(what, getttynam(NAMES[i]));
        snprintf(what, sizeof what, "%s getttyent after getttynam(%s)", run, NAMES[i]);
        tty(what, getttyent());
    }
    printf("%s endttyent = %d\n", run, endttyent());
    printf("%s endttyent again = %d\n", run, endttyent());
    printf("%s setttyent after endttyent = %d\n", run, setttyent());
    printf("%s endttyent = %d\n", run, endttyent());
    return 0;
}
'''


def escape(text: str) -> str:
    """`text` as the program's `str` writes a string, for the tests to read
    the files back from the oracle."""
    out = []
    for ch in text:
        c = ord(ch)
        if ch == "\\":
            out.append("\\\\")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\t":
            out.append("\\t")
        elif c < 0x20 or c >= 0x7F:
            out.append(f"\\x{c:02x}")
        else:
            out.append(ch)
    return "".join(out)


def main() -> None:
    with workdir() as t:
        d = Path(t)
        (d / "tf.c").write_text(PROGRAM, encoding="utf-8", newline="\n")
        (d / "fstab").write_text(FSTAB, encoding="utf-8", newline="\n")
        (d / "ttys").write_text(TTYS, encoding="utf-8", newline="\n")
        w = wsl_path(d)
        # Built outside the namespaces, where /etc is the system's; run in
        # them over a tmpfs /etc: with the two files, then with nothing.
        script = (
            f"set -e; cp {w}/tf.c {w}/fstab {w}/ttys /tmp/ && cd /tmp && "
            "gcc -static -O0 -Wall -Werror -o fsttys tf.c 2>&1 | grep -v 'statically linked' || true; "
            "unshare -rm sh -c 'mount -t tmpfs none /etc && cp /tmp/fstab /etc/fstab && "
            "cp /tmp/ttys /etc/ttys && /tmp/fsttys files'; "
            "unshare -rm sh -c 'mount -t tmpfs none /etc && /tmp/fsttys none'"
        )
        r = run(script)
        if r.returncode != 0 or "getfsent" not in r.stdout:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
        body = r.stdout
    head = ("# glibc 2.39's <fstab.h> and <ttyent.h>, for posix/src/fstab.rs and\n"
            "# posix/src/ttyent.rs. Generated by posix/tools/oracle/fsttys_harness.py;\n"
            "# do not edit.\n")
    inputs = f"input fstab = {escape(FSTAB)}\ninput ttys = {escape(TTYS)}\n"
    OUT.write_text(head + inputs + body, encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {body.count(chr(10))} lines")


if __name__ == "__main__":
    main()
