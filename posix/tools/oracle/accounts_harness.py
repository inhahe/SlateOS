"""glibc 2.39's account-file functions, as the oracle for posix/src/pwd.rs and
posix/src/shadow.rs:

    python posix/tools/oracle/accounts_harness.py   # writes posix/src/accounts_oracle.txt

The readers (`fgetpwent_r`, `fgetgrent_r`, `fgetspent_r` and their
non-reentrant forms, `sgetspent_r`, `sgetspent`), the writers (`putpwent`,
`putgrent`, `putspent`), `getusershell` over several `/etc/shells`, `lckpwdf`
and `getpass`. The C program runs in `unshare -r -m` with a private copy of
`/etc` on a tmpfs, so it can write `/etc/shells` and `/etc/.pwd.lock` without
root and without touching WSL's own.

The file carries its inputs as well as glibc's answers, so the Rust tests
build the same inputs from it: one fact, one place. Bytes outside `!`..`~`,
and `|`, `\\`, `,`, `[`, `]`, are written `\\xHH`. A string field is `=`
and its bytes, or `!` for NULL.

    T <kind> <name> <text>                 an input file (kind pw, gr, sp, sh)
    R <fn> <name> <i> <buflen> = <rc> <errno> <entry or ->
    N <fn> <name> <i> = <errno> <entry or ->
    C <kind> <case> <record>               an input record for put*/sgetspent
    S <fn> <case> <buflen> = <rc> <errno> <entry or ->
    P <fn> <case> = <rc> <errno> <output>
    U <case> = [<shell>,...]
    L <step> = <rc> <errno>
    G <step> = <result>

`errno` is set to 1234 before every call, so 1234 afterwards means
"untouched".
"""

import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "accounts_oracle.txt"
SPECIAL = set(b"|\\,[]")


def esc(b: bytes) -> str:
    return "".join(chr(c) if 0x21 <= c <= 0x7E and c not in SPECIAL else "\\x%02x" % c for c in b)


def field(b) -> str:
    return "!" if b is None else "=" + esc(b)


def c_str(b) -> str:
    """A C string literal for `b` (or NULL), octal-escaped so no escape runs
    into the next character."""
    if b is None:
        return "NULL"
    out = []
    for c in b:
        if c in (0x22, 0x5C) or not 0x20 <= c < 0x7F:
            out.append("\\%03o" % c)
        else:
            out.append(chr(c))
    return '"' + "".join(out) + '"'


# ---------------------------------------------------------------------------
# The files the readers read
# ---------------------------------------------------------------------------

PW = {
    "pw_basic": b"root:x:0:0:root:/root:/bin/bash\ndaemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin\n"
                b"bin:x:2:2:bin:/bin:/usr/sbin/nologin\n",
    "pw_messy": b"# a comment\n   \n\n  alice:x:1000:1000:Alice A,,,:/home/alice:/bin/sh\n"
                b"\tbob:*:1001:100::/home/bob:\nbad:x:notanumber:0::/:\nnouid:x::0::/:\nshort:x:5\n"
                b"+\n+nis:::::\n-evil\n+@group::::::\ncarol:x:4294967295:4294967296:C:/c:/bin/c\n"
                b"dave:x:-1:-2:D:/d:/bin/d\neve:x:12abc:0:E:/e:/e\nfrank:x:7:7:F:/f:/bin/f:extra:fields\n"
                b"gina:x:8:8:G:/g:/bin/g\r\nhank:x: 9: 9:H:/h:/bin/h\n:x:11:11:::\nutf:x:12:12:\xc3\xa9t\xc3\xa9:/u:/bin/u\n"
                b"last:x:10:10:L:/l:/bin/l",
    "pw_long": b"first:x:1:1:" + b"g" * 5000 + b":/f:/bin/f\nsecond:x:2:2:s:/s:/bin/s\n",
    "pw_empty": b"",
    "pw_comments": b"# nothing\n\n   # but comments\n",
}

GR = {
    "gr_basic": b"root:x:0:\nwheel:x:10:root,alice\nusers:x:100:alice,bob,carol\n",
    "gr_messy": b"# groups\nroot:x:0:\nspaced:x:11: a , b ,, c,\nnomem:x:12\nbadgid:x:abc:\nemptygid:x::\n"
                b"+\n+:::\n-grp\n+grp:x::m1,m2\nbig:x:4294967296:z\ntrail:x:13:a,b,\n  lead:x:14:y\n"
                b"last:x:15:x",
    "gr_long": b"many:x:1:" + b",".join(b"member%04d" % i for i in range(600)) + b"\nnext:x:2:n\n",
}

SP = {
    "sp_basic": b"root:!:19000:0:99999:7:::\nalice:$6$salt$hash:19001:1:90:14:30:20000:\n"
                b"bob:*:19002::::::\n",
    "sp_messy": b"# shadow\nold:x:1:2:3\n+\n-bad\n+nis::::::::\nspaces:x: 1: 2: 3: 4: 5: 6: 7\n"
                b"neg:x:-1:-2:-3:-4:-5:-6:-7\nbig:x:4294967296:0:0:0:0:0:18446744073709551615\n"
                b"bad:x:1:2:3:4:5:6:7:8\nshort:x:::\nokold:x::::\ntrailing:x:1:2:3:\nnotnum:x:a:::\n"
                b"last:x:1:1:1:1:1:1:1",
}

# (name, text, schedule of buffer sizes)
READS = [(n, t, [4096]) for n, t in PW.items() if n != "pw_long"] + [("pw_basic", PW["pw_basic"], [8, 4096]),
                                                   ("pw_long", PW["pw_long"], [4096, 16384])]
READS_GR = [(n, t, [4096]) for n, t in GR.items() if n != "gr_long"] + [
    ("gr_basic", GR["gr_basic"], [8, 4096]), ("gr_long", GR["gr_long"], [4096, 16384])]
READS_SP = [(n, t, [4096]) for n, t in SP.items()] + [("sp_basic", SP["sp_basic"], [8, 4096])]

SGET = [
    ("full", b"root:!:19000:0:99999:7:::"),
    ("nl", b"root:!:19000:0:99999:7:::\n"),
    ("two", b"root:!:19000:0:99999:7:::\nsecond:x:1:1:1:1:1:1:1"),
    ("lead", b" lead:x:1:2:3"),
    ("hash", b"#c:x:1:2:3"),
    ("empty", b""),
    ("plus", b"+"),
    ("minus", b"-x"),
    ("plusc", b"+:"),
    ("old", b"old:x:1:2:3"),
    ("eight", b"bad:x:1:2:3:4:5:6:7:8"),
    ("short", b"short:x"),
    ("three", b"s:x:::"),
    ("four", b"s:x::::"),
    ("flag", b"f:x:1:2:3:4:5:6:18446744073709551615"),
    ("neg", b"n:x:-5:-1:0:1:2:3:0"),
]

# putpwent: name, passwd, uid, gid, gecos, dir, shell
PUTPW = [
    ("plain", (b"root", b"x", 0, 0, b"root", b"/root", b"/bin/bash")),
    ("nulls", (b"u", None, 5, 6, None, None, None)),
    ("gecos", (b"g", b"x", 1, 1, b"a:b\nc:", b"/h", b"/s")),
    ("namecolon", (b"a:b", b"x", 1, 1, b"", b"/h", b"/s")),
    ("dirnl", (b"a", b"x", 1, 1, b"", b"/h\n", b"/s")),
    ("shellcolon", (b"a", b"x", 1, 1, b"", b"/h", b"/s:x")),
    ("passcolon", (b"a", b"x:y", 1, 1, b"", b"/h", b"/s")),
    ("nullname", (None, b"x", 1, 1, b"", b"/h", b"/s")),
    ("plus", (b"+", b"x", 7, 8, b"g", b"/h", b"/s")),
    ("minus", (b"-bad", None, 7, 8, None, None, None)),
    ("maxid", (b"m", b"x", 4294967295, 4294967294, b"", b"", b"")),
    ("emptyname", (b"", b"x", 1, 1, b"", b"/h", b"/s")),
    ("utf8", (b"u", b"x", 1, 1, b"\xc3\xa9", b"/h", b"/s")),
]

# putgrent: name, passwd, gid, members (None for a NULL gr_mem)
PUTGR = [
    ("plain", (b"wheel", b"x", 10, [b"root", b"alice"])),
    ("nomem", (b"g", b"x", 11, None)),
    ("emptymem", (b"g", b"x", 11, [])),
    ("nullpass", (b"g", None, 12, [b"a"])),
    ("comma", (b"g", b"x", 13, [b"a,b"])),
    ("memcolon", (b"g", b"x", 13, [b"a:b"])),
    ("memnl", (b"g", b"x", 13, [b"a\n"])),
    ("namenl", (b"g\n", b"x", 13, [])),
    ("passcolon", (b"g", b"x:", 13, [])),
    ("nullname", (None, b"x", 13, [])),
    ("plus", (b"+", b"x", 14, [b"a", b"b"])),
    ("minus", (b"-g", None, 14, None)),
    ("maxgid", (b"m", b"", 4294967295, [b"x"])),
]

# putspent: name, passwd, lstchg, min, max, warn, inact, expire, flag
M1 = -1
ALL1 = 18446744073709551615
PUTSP = [
    ("plain", (b"root", b"!", 19000, 0, 99999, 7, M1, M1, ALL1)),
    ("unset", (b"u", b"x", M1, M1, M1, M1, M1, M1, ALL1)),
    ("zero", (b"z", b"", 0, 0, 0, 0, 0, 0, 0)),
    ("flag", (b"f", b"x", 1, 2, 3, 4, 5, 6, 9223372036854775808)),
    ("neg", (b"n", b"x", -5, -2, -9223372036854775808, 9223372036854775807, M1, M1, 5)),
    ("nullpass", (b"p", None, M1, M1, M1, M1, M1, M1, ALL1)),
    ("namecolon", (b"a:b", b"x", M1, M1, M1, M1, M1, M1, ALL1)),
    ("passnl", (b"a", b"x\n", M1, M1, M1, M1, M1, M1, ALL1)),
    ("plus", (b"+", b"x", M1, M1, M1, M1, M1, M1, ALL1)),
]

SHELLS = [
    ("two", b"/bin/sh\n/bin/bash\n"),
    ("messy", b"# a comment line, long enough to pad the file's size\n/bin/sh\n\n  /usr/bin/zsh  # trailing\n"
              b"not/a/shell\n/\nx/bin/ksh\na#/bin/no\n/bin/a /bin/b\n/bin/dash"),
    ("empty", b""),
    ("comments", b"#only comments here\n# and here\n"),
    ("missing", None),
]


def c_program() -> str:
    L = []
    a = L.append
    a(r'''#define _GNU_SOURCE
#include <errno.h>
#include <grp.h>
#include <pwd.h>
#include <shadow.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

static void esc_n(const char *s, size_t n) {
  for (size_t i = 0; i < n; i++) {
    unsigned char c = (unsigned char)s[i];
    if (c < 0x21 || c > 0x7e || strchr("|\\,[]", c)) printf("\\x%02x", c); else putchar(c);
  }
}
static void field(const char *s) { if (!s) { putchar('!'); return; } putchar('='); esc_n(s, strlen(s)); }
static void pw(const struct passwd *p) {
  field(p->pw_name); putchar('|'); field(p->pw_passwd); printf("|%u|%u|", p->pw_uid, p->pw_gid);
  field(p->pw_gecos); putchar('|'); field(p->pw_dir); putchar('|'); field(p->pw_shell);
}
static void gr(const struct group *g) {
  field(g->gr_name); putchar('|'); field(g->gr_passwd); printf("|%u|", g->gr_gid);
  if (!g->gr_mem) { putchar('!'); return; }
  putchar('[');
  for (char **m = g->gr_mem; *m; m++) { if (m != g->gr_mem) putchar(','); field(*m); }
  putchar(']');
}
static void sp(const struct spwd *s) {
  field(s->sp_namp); putchar('|'); field(s->sp_pwdp);
  printf("|%ld|%ld|%ld|%ld|%ld|%ld|%lu", s->sp_lstchg, s->sp_min, s->sp_max, s->sp_warn,
         s->sp_inact, s->sp_expire, s->sp_flag);
}
static FILE *text_file(const char *t, size_t n) {
  FILE *f = tmpfile();
  if (!f) { perror("tmpfile"); exit(2); }
  fwrite(t, 1, n, f); rewind(f); return f;
}
#define READ_R(FN, TYPE, SHOW) \
static void read_##FN(const char *name, const char *t, size_t n, const size_t *sched, int ns) { \
  FILE *f = text_file(t, n); int ends = 0, errs = 0; \
  for (int i = 0; i < 2000 && ends < 2 && errs < 4; i++) { \
    size_t bl = sched[i % ns]; char *buf = malloc(bl); TYPE e, *r = NULL; \
    errno = 1234; int rc = FN(f, &e, buf, bl, &r); int en = errno; \
    printf("R " #FN " %s %d %zu = %d %d ", name, i, bl, rc, en); \
    if (r == &e) SHOW(&e); else if (r == NULL) putchar('-'); else putchar('?'); \
    putchar('\n'); free(buf); if (rc == ENOENT) ends++; \
    errs = rc != 0 && rc != ENOENT ? errs + 1 : 0; \
  } \
  fclose(f); \
}
READ_R(fgetpwent_r, struct passwd, pw)
READ_R(fgetgrent_r, struct group, gr)
READ_R(fgetspent_r, struct spwd, sp)
#define READ_N(FN, TYPE, SHOW) \
static void readn_##FN(const char *name, const char *t, size_t n) { \
  FILE *f = text_file(t, n); \
  for (int i = 0; i < 2000; i++) { \
    errno = 1234; TYPE *p = FN(f); int en = errno; \
    printf("N " #FN " %s %d = %d ", name, i, en); \
    if (p) SHOW(p); else putchar('-'); \
    putchar('\n'); if (!p) break; \
  } \
  fclose(f); \
}
READ_N(fgetpwent, struct passwd, pw)
READ_N(fgetgrent, struct group, gr)
READ_N(fgetspent, struct spwd, sp)
static void sget(const char *name, const char *s) {
  size_t sizes[] = {4096, 4, strlen(s) + 1, strlen(s)};
  for (int k = 0; k < 4; k++) {
    size_t bl = sizes[k]; if (bl == 0) continue;
    char *buf = malloc(bl); struct spwd e, *r = NULL;
    errno = 1234; int rc = sgetspent_r(s, &e, buf, bl, &r); int en = errno;
    printf("S sgetspent_r %s %zu = %d %d ", name, bl, rc, en);
    if (r == &e) sp(&e); else if (r == NULL) putchar('-'); else putchar('?');
    putchar('\n'); free(buf);
  }
  errno = 1234; struct spwd *p = sgetspent(s); int en = errno;
  printf("S sgetspent %s 0 = 0 %d ", name, en);
  if (p) sp(p); else putchar('-');
  putchar('\n');
}
static void show_output(const char *fn, const char *name, int rc, int en, char *out, size_t len) {
  printf("P %s %s = %d %d =", fn, name, rc, en); esc_n(out, len); putchar('\n');
}
static void putpw(const char *name, struct passwd *p) {
  char *out = NULL; size_t len = 0; FILE *f = open_memstream(&out, &len);
  errno = 1234; int rc = putpwent(p, f); int en = errno; fclose(f);
  show_output("putpwent", name, rc, en, out, len); free(out);
}
static void putgr(const char *name, struct group *g) {
  char *out = NULL; size_t len = 0; FILE *f = open_memstream(&out, &len);
  errno = 1234; int rc = putgrent(g, f); int en = errno; fclose(f);
  show_output("putgrent", name, rc, en, out, len); free(out);
}
static void putsp(const char *name, struct spwd *s) {
  char *out = NULL; size_t len = 0; FILE *f = open_memstream(&out, &len);
  errno = 1234; int rc = putspent(s, f); int en = errno; fclose(f);
  show_output("putspent", name, rc, en, out, len); free(out);
}
static void shells(const char *name, const char *t, size_t n, int missing) {
  unlink("/etc/shells");
  if (!missing) { FILE *f = fopen("/etc/shells", "w"); fwrite(t, 1, n, f); fclose(f); }
  endusershell();
  printf("U %s = [", name);
  char *s; int first = 1;
  while ((s = getusershell()) != NULL) { if (!first) putchar(','); first = 0; field(s); }
  printf("]\n");
}
static void one(const char *step) {
  errno = 1234; char *s = getusershell(); printf("U seq-%s = [", step);
  if (s) field(s);
  printf("]\n");
}
''')
    # main
    a("int main(int argc, char **argv) {")
    a('  if (argc > 1 && strcmp(argv[1], "getpass") == 0) {')
    a('    for (int i = 0; i < 3; i++) {')
    a('      char prompt[16]; snprintf(prompt, sizeof prompt, "P%d:", i);')
    a('      errno = 1234; char *r = getpass(prompt);')
    a('      printf("G %d = ", i); field(r); putchar(\'\\n\');')
    a('    }')
    a('    return 0;')
    a('  }')
    a("  setvbuf(stdout, NULL, _IOFBF, 1 << 16);")
    for fn, reads in (("fgetpwent_r", READS), ("fgetgrent_r", READS_GR), ("fgetspent_r", READS_SP)):
        for name, text, sched in reads:
            s = ", ".join(str(x) for x in sched)
            a(f"  {{ static const size_t s[] = {{{s}}}; read_{fn}({c_str(name.encode())}, "
              f"{c_str(text)}, {len(text)}, s, {len(sched)}); }}")
    for fn, texts in (("fgetpwent", PW), ("fgetgrent", GR), ("fgetspent", SP)):
        for name, text in texts.items():
            a(f"  readn_{fn}({c_str(name.encode())}, {c_str(text)}, {len(text)});")
    for name, s in SGET:
        a(f"  sget({c_str(name.encode())}, {c_str(s)});")
    for name, (n, p, u, g, ge, d, sh) in PUTPW:
        a(f"  {{ struct passwd p = {{ (char *){c_str(n)}, (char *){c_str(p)}, {u}u, {g}u, "
          f"(char *){c_str(ge)}, (char *){c_str(d)}, (char *){c_str(sh)} }}; putpw({c_str(name.encode())}, &p); }}")
    for name, (n, p, g, mem) in PUTGR:
        if mem is None:
            m = "NULL"
            decl = ""
        else:
            decl = "char *m[] = {" + "".join(f"(char *){c_str(x)}, " for x in mem) + "NULL}; "
            m = "m"
        a(f"  {{ {decl}struct group g = {{ (char *){c_str(n)}, (char *){c_str(p)}, {g}u, {m} }}; "
          f"putgr({c_str(name.encode())}, &g); }}")
    for name, (n, p, l1, l2, l3, l4, l5, l6, fl) in PUTSP:
        vals = ", ".join(f"(long)({v}LL)" if v != -9223372036854775808 else "(long)(-9223372036854775807LL - 1)"
                         for v in (l1, l2, l3, l4, l5, l6))
        a(f"  {{ struct spwd s = {{ (char *){c_str(n)}, (char *){c_str(p)}, {vals}, {fl}UL }}; "
          f"putsp({c_str(name.encode())}, &s); }}")
    for name, text in SHELLS:
        if text is None:
            a(f"  shells({c_str(name.encode())}, \"\", 0, 1);")
        else:
            a(f"  shells({c_str(name.encode())}, {c_str(text)}, {len(text)}, 0);")
    # The state machine: a fresh start, two reads, a rewind, a read, an end.
    t = SHELLS[0][1]
    a(f"  shells(\"seq\", {c_str(t)}, {len(t)}, 0); endusershell();")
    a('  one("a"); one("b"); one("c"); one("d"); setusershell(); one("e"); endusershell(); one("f");')
    # lckpwdf: twice, then a child -- forked before the lock, so it does not
    # inherit glibc's lock_fd -- that tries once the parent holds it and waits
    # the 15 seconds out; then unlock twice, and once more round.
    a("  int pfd[2]; if (pipe(pfd) != 0) return 2;")
    a("  fflush(stdout);")
    a("  pid_t pid = fork();")
    a("  if (pid == 0) { char c; if (read(pfd[0], &c, 1) != 1) _exit(2); errno = 1234; "
      'int r = lckpwdf(); printf("L child = %d %d\\n", r, errno); fflush(stdout); _exit(0); }')
    a('  errno = 1234; int rc = lckpwdf(); printf("L lock1 = %d %d\\n", rc, errno);')
    a('  errno = 1234; rc = lckpwdf(); printf("L lock2 = %d %d\\n", rc, errno);')
    a("  fflush(stdout);")
    a('  if (write(pfd[1], "x", 1) != 1) return 2;')
    a("  int st; waitpid(pid, &st, 0);")
    a('  errno = 1234; rc = ulckpwdf(); printf("L unlock1 = %d %d\\n", rc, errno);')
    a('  errno = 1234; rc = ulckpwdf(); printf("L unlock2 = %d %d\\n", rc, errno);')
    a('  errno = 1234; rc = lckpwdf(); printf("L lock3 = %d %d\\n", rc, errno);')
    a('  errno = 1234; rc = ulckpwdf(); printf("L unlock3 = %d %d\\n", rc, errno);')
    a("  return 0;")
    a("}")
    return "\n".join(L) + "\n"


def inputs() -> list:
    out = []
    for kind, texts in (("pw", PW), ("gr", GR), ("sp", SP)):
        for name, text in texts.items():
            out.append(f"T {kind} {name} ={esc(text)}")
    for name, text in SHELLS:
        out.append(f"T sh {name} {field(text)}")
    for name, s in SGET:
        out.append(f"C sget {name} ={esc(s)}")
    for name, (n, p, u, g, ge, d, sh) in PUTPW:
        out.append(f"C pw {name} {field(n)}|{field(p)}|{u}|{g}|{field(ge)}|{field(d)}|{field(sh)}")
    for name, (n, p, g, mem) in PUTGR:
        m = "!" if mem is None else "[" + ",".join(field(x) for x in mem) + "]"
        out.append(f"C gr {name} {field(n)}|{field(p)}|{g}|{m}")
    for name, (n, p, *nums) in PUTSP:
        out.append(f"C sp {name} {field(n)}|{field(p)}|" + "|".join(str(v) for v in nums))
    return out


def main():
    with workdir() as tmp:
        work = Path(tmp)
        (work / "accounts_oracle.c").write_text(c_program(), encoding="utf-8", newline="\n")
        w = wsl_path(work)
        build = f"cd {w} && gcc -O0 -w -o accounts_oracle accounts_oracle.c"
        sandbox = ("unshare -r -m sh -c 'e=/tmp/etcacct.$$; mkdir -p $e && mount -t tmpfs none $e && "
                   "(cp -a /etc/. $e/ 2>/dev/null; true) && mount --bind $e /etc && ./accounts_oracle'")
        r = subprocess.run(["wsl", "-d", "Ubuntu", "--exec", "bash", "-c", f"{build} && {sandbox}"],
                           capture_output=True)
        if r.returncode != 0:
            sys.exit(f"oracle failed:\n{r.stdout[-2000:]!r}\n{r.stderr.decode(errors='replace')[-4000:]}")
        main_out = r.stdout.decode("ascii")
        # `setsid`: a session with no controlling terminal, so glibc's
        # `fopen("/dev/tty")` fails and getpass reads stdin and prompts on
        # stderr -- with one, it would wait on a terminal nobody types at.
        g = subprocess.run(["wsl", "-d", "Ubuntu", "--exec", "bash", "-c",
                            f"cd {w} && printf 'secret\\nline2' | setsid -w ./accounts_oracle getpass "
                            "2>prompts.txt"],
                           capture_output=True, timeout=120)
        if g.returncode != 0:
            sys.exit(f"getpass oracle failed:\n{g.stderr.decode(errors='replace')}")
        prompts = (work / "prompts.txt").read_bytes()
        gp_out = g.stdout.decode("ascii")
    lines = [
        "# glibc 2.39's account-file functions: posix/tools/oracle/accounts_harness.py.",
        "# The inputs (T, C), then glibc's answers. See the harness for the format.",
    ]
    lines += inputs()
    lines += main_out.splitlines()
    lines += gp_out.splitlines()
    lines.append(f"G prompts = ={esc(prompts)}")
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {len(lines)} lines")


if __name__ == "__main__":
    main()
