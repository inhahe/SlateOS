"""glibc 2.39's glob over a directory tree of its own, as the oracle for
posix/src/glob.rs.

    python posix/tools/oracle/glob_harness.py   # writes posix/src/glob_oracle.txt

The tree is not on a disk. glob is given it through GLOB_ALTDIRFUNC -- the
program's own opendir, readdir, closedir, stat and lstat -- over TREE, here,
so that the answers depend on nothing but the tree and glob: not on the
directory order a file system returns, and not on what names the machine
the tests run on allows (the tree has `*`, `?`, a backslash, and names that
differ only in case, which Windows cannot hold). posix/src/glob.rs's tests
build the same tree from the oracle's own `# tree:` line and replay every
probe through the same callbacks.

One line a probe:

    <flags> <pattern> = <return> <magchar> <path>... | <errfunc calls>

`magchar` is 1 when glob set GLOB_MAGCHAR in gl_flags. Each path, and the
pattern, is escaped with `\\xNN` for a backslash, a space and anything
outside printable ASCII (`\\x` alone is the empty string). An errfunc call
is `<path>:<errno>`, the path escaped the same way; the probes whose flags
include ERRSTOP have an errfunc that asks glob to stop.

It writes glob_deviations.txt beside the oracle too: each probe glob_model.py
-- the rules posix/src/glob.rs follows, written a second way -- answers
otherwise than glibc, as a pair of lines, `glibc <glibc's line>` and
`here  <this library's>`. Design-decisions section 1149 says why they differ.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "glob_oracle.txt"
DEVIATIONS = POSIX_SRC / "glob_deviations.txt"

# The tree: a path a line, directories with a trailing slash; relative ones
# are in the current directory, "noperm/" cannot be opened (EACCES).
TREE = [
    "a", "b", "c", "ab", "abc", "A", "B", ".hidden", ".h2", "x.c", "y.c", "z.h",
    "q?", "br[a]", "back\\slash", "*", "{b}", "~tilde", "sp ace",
    "dir1/", "dir1/f1", "dir1/f2", "dir1/.f3", "dir1/sub/", "dir1/sub/g1", "dir1/sub/.g2",
    "dir2/", "dir2/f1", "dir2/x.c",
    "empty/", "noperm/", "noperm/secret",
    "/abs/", "/abs/x1", "/abs/x2", "/abs/y",
    "/home/", "/home/u/", "/home/u/x", "/home/u/.dot", "/home/u/d/", "/home/u/d/e",
]

PATTERNS = [
    "*", "?", "a*", "*b*", ".*", "*.c", "[ab]*", "[!a]*", "[a-c]", "[[:upper:]]",
    "dir1/*", "dir*/*", "*/*", "*/.*", "dir1/.*", "dir1/sub/*", "*/sub/*", "d*/s*/g*", "*/*/*",
    "dir1", "dir1/f1", "nonexistent", "nonexistent/*", "dir1/nonexistent", "nonexistent/f1", "",
    "/abs/*", "/abs/x?", "//abs/*", "/abs//x1", "dir1//*", "./a", "./*", "./dir1/*",
    "*/", "dir1/", "a/", "d*/", "*/..", "dir1/../a", "dir1/sub/..",
    "q\\?", "q?", "br\\[a\\]", "br[[]a]", "\\*", "\\a", "*\\*", "back\\\\slash", "back\\slash",
    "{b}", "a{b,c}", "{a,b}", "{dir1,dir2}/f1", "{a,{b,c}}", "{a,b", "{}", "a{,b}", "{x,y}.c",
    "~", "~/x", "~/*", "~/.*", "~root", "~nosuchuser", "~nosuchuser/x", "\\~", "~tilde",
    "*[", "[", "[!]", "e*", "empty/*", "noperm/*", "*/secret", "sp ace", "sp*", "sp\\ ace",
    "dir1/sub", "dir?/sub/g?", "*/f1", "[d]ir1/f1", "dir1/[f]1",
    ".*/*", ".*/f1", "*/./f1", "dir1/./f1", ".*/", "..", "..*", ".", "*/*/", "?/", "[.]*",
    # One character before a trailing slash, and the same matches spelled
    # longer: glibc answers `*/` and `?/` by a path of their own
    # (design-decisions section 1149), and these as the rest.
    "**/", "?*/", "[!x]/", "??/",
]

# glob_pattern_p's cases, each with quote 0 and 1.
PATTERN_P = [
    "", "a", "*", "?", "a*b", "[", "[a", "[a]", "a]", "]", "[]", "[]]", "\\*", "\\?", "\\[a]",
    "[\\]]", "a\\", "\\", "{a,b}", "~", "a[b\\]c", "[!a]", "**", "a/*/b", "\\\\*",
]

FLAGS = [
    ("0", 0x0),
    ("MARK", 0x2),
    ("NOSORT", 0x4),
    ("NOCHECK", 0x10),
    ("NOESCAPE", 0x40),
    ("NOCHECK|NOESCAPE", 0x50),
    ("PERIOD", 0x80),
    ("BRACE", 0x400),
    ("NOMAGIC", 0x800),
    ("TILDE", 0x1000),
    ("TILDE_CHECK", 0x4000),
    ("ONLYDIR", 0x2000),
    ("MARK|ONLYDIR", 0x2002),
    ("ERR", 0x1),
    ("ERRSTOP", 0x0),
    ("BRACE|TILDE|MARK", 0x1402),
]

PROGRAM = r'''
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <glob.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

@TREE@

/* The node a path names, the tree's way: components, "." and ".." resolved;
 * -1 if there is none. Node 0 is the current directory, node 1 the root. */
enum { CWD = -2, ROOT = -3 };
static int child(int dir, const char *name, size_t len)
{
    /* A node's parent is the longest proper prefix of its path ending in /. */
    for (int i = 0; i < NTREE; i++) {
        const char *p = TREE[i];
        size_t n = strlen(p);
        if (p[n - 1] == '/') n--;
        /* its parent: */
        const char *last = NULL;
        for (size_t k = 0; k < n; k++) if (p[k] == '/') last = p + k;
        int parent;
        const char *nm;
        if (p[0] == '/') {
            if (last == p) { parent = ROOT; nm = p + 1; }
            else {
                parent = -1;
                for (int j = 0; j < NTREE; j++) {
                    size_t pl = (size_t)(last - p) + 1;
                    if (strlen(TREE[j]) == pl && strncmp(TREE[j], p, pl) == 0) parent = j;
                }
                nm = last + 1;
            }
        } else if (last == NULL) { parent = CWD; nm = p; }
        else {
            parent = -1;
            for (int j = 0; j < NTREE; j++) {
                size_t pl = (size_t)(last - p) + 1;
                if (strlen(TREE[j]) == pl && strncmp(TREE[j], p, pl) == 0) parent = j;
            }
            nm = last + 1;
        }
        if (parent == dir && (size_t)(p + n - nm) == len && strncmp(nm, name, len) == 0)
            return i;
    }
    return -1;
}

static int is_dir(int node) { return node < 0 || TREE[node][strlen(TREE[node]) - 1] == '/'; }

/* The node `path` names, or -1 (errno set). ".." goes back along the path
 * taken, and stays at the top. */
static int lookup(const char *path)
{
    int node = path[0] == '/' ? ROOT : CWD;
    int stack[64], depth = 0;
    const char *s = path;
    while (*s) {
        while (*s == '/') s++;
        if (!*s) break;
        const char *e = strchr(s, '/');
        size_t len = e ? (size_t)(e - s) : strlen(s);
        if (len == 1 && s[0] == '.') { /* stay */ }
        else if (len == 2 && s[0] == '.' && s[1] == '.') { if (depth > 0) node = stack[--depth]; }
        else {
            if (!is_dir(node)) { errno = ENOTDIR; return -1; }
            int c = child(node, s, len);
            if (c < 0) { errno = ENOENT; return -1; }
            stack[depth++] = node;
            node = c;
        }
        s += len;
    }
    /* A trailing slash names a directory: "a/" of a file is ENOTDIR, as the
     * kernel's lookup answers. */
    if (*path && path[strlen(path) - 1] == '/' && !is_dir(node)) { errno = ENOTDIR; return -1; }
    return node;
}

struct dirstream { int node; int i; int dots; struct dirent d; };

static void *my_opendir(const char *path)
{
    int node = lookup(path);
    if (node == -1) return NULL;
    if (!is_dir(node)) { errno = ENOTDIR; return NULL; }
    if (node >= 0 && strcmp(TREE[node], "noperm/") == 0) { errno = EACCES; return NULL; }
    struct dirstream *d = calloc(1, sizeof *d);
    d->node = node;
    return d;
}

static struct dirent *my_readdir(void *v)
{
    struct dirstream *d = v;
    if (d->dots < 2) {
        strcpy(d->d.d_name, d->dots ? ".." : ".");
        d->d.d_type = DT_DIR;
        d->dots++;
        return &d->d;
    }
    while (d->i < NTREE) {
        int i = d->i++;
        const char *p = TREE[i];
        size_t n = strlen(p);
        const char *end = p + n - (p[n - 1] == '/');
        const char *nm = end;
        while (nm > p && nm[-1] != '/') nm--;
        if (child(d->node, nm, (size_t)(end - nm)) == i) {
            memcpy(d->d.d_name, nm, (size_t)(end - nm));
            d->d.d_name[end - nm] = 0;
            d->d.d_type = p[n - 1] == '/' ? DT_DIR : DT_REG;
            return &d->d;
        }
    }
    return NULL;
}

static void my_closedir(void *v) { free(v); }

static int my_stat(const char *path, struct stat *st)
{
    int node = lookup(path);
    if (node == -1) return -1;
    memset(st, 0, sizeof *st);
    st->st_mode = is_dir(node) ? (S_IFDIR | 0755) : (S_IFREG | 0644);
    return 0;
}

/* `s` as the oracle writes a name, into `out`. */
static void esc_to(char *out, size_t cap, const char *s)
{
    size_t n = strlen(out);
    if (!*s) { snprintf(out + n, cap - n, "\\x"); return; }
    for (; *s; s++) {
        unsigned char c = *s;
        n = strlen(out);
        if (c == '\\' || c <= ' ' || c > '~') snprintf(out + n, cap - n, "\\x%02x", c);
        else snprintf(out + n, cap - n, "%c", c);
    }
}

static char errlog[4096];
static int stop_on_error;
static int on_error(const char *path, int err)
{
    size_t n = strlen(errlog);
    snprintf(errlog + n, sizeof errlog - n, " ");
    esc_to(errlog, sizeof errlog, path);
    n = strlen(errlog);
    snprintf(errlog + n, sizeof errlog - n, ":%d", err);
    return stop_on_error;
}

static void esc(const char *s)
{
    char buf[8192] = "";
    esc_to(buf, sizeof buf, s);
    fputs(buf, stdout);
}

static void probe(const char *fname, int flags, int stop, const char *pat)
{
    glob_t g;
    memset(&g, 0, sizeof g);
    g.gl_opendir = my_opendir;
    g.gl_readdir = my_readdir;
    g.gl_closedir = my_closedir;
    g.gl_stat = my_stat;
    g.gl_lstat = my_stat;
    errlog[0] = 0;
    stop_on_error = stop;
    int r = glob(pat, flags | GLOB_ALTDIRFUNC, on_error, &g);
    printf("%s ", fname);
    esc(pat);
    printf(" = %d %d", r, (g.gl_flags & GLOB_MAGCHAR) != 0);
    if (r == 0 || r == GLOB_NOMATCH || r == GLOB_ABORTED)
        for (size_t i = 0; i < g.gl_pathc; i++) { putchar(' '); esc(g.gl_pathv[i]); }
    printf(" |%s\n", errlog);
    if (r == 0) globfree(&g);
}

int main(void)
{
    setenv("HOME", "/home/u", 1);
@PROBES@
@PATTERN_P@
    return 0;
}
'''


def c_string(s: str) -> str:
    # Octal, not hex: C's hex escapes take every hex digit that follows.
    return '"' + "".join(f"\\{ord(c):03o}" if c in '"\\' or not " " <= c <= "~" else c for c in s) + '"'


def escape(s: str) -> str:
    if not s:
        return "\\x"
    return "".join(f"\\x{ord(c):02x}" if c in "\\ " or not "!" <= c <= "~" else c for c in s)


def main() -> None:
    tree = "static const char *TREE[] = {" + ", ".join(c_string(t) for t in TREE) + "};\n"
    tree += f"#define NTREE {len(TREE)}\n"
    probes = []
    for name, value in FLAGS:
        stop = 1 if name == "ERRSTOP" else 0
        for p in PATTERNS:
            probes.append(f"    probe({c_string(name)}, {value}, {stop}, {c_string(p)});")
    pattern_p = []
    for p in PATTERN_P:
        for quote in (0, 1):
            pattern_p.append(f'    printf("pattern_p %d ", {quote}); esc({c_string(p)}); '
                             f'printf(" = %d\\n", glob_pattern_p({c_string(p)}, {quote}));')
    program = (PROGRAM.replace("@TREE@", tree).replace("@PROBES@", "\n".join(probes))
               .replace("@PATTERN_P@", "\n".join(pattern_p)))
    with workdir() as t:
        d = Path(t)
        (d / "g.c").write_text(program, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O0 -w -o g g.c && LC_ALL=C ./g")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
    head = [
        "# glibc 2.39's glob over the tree below, through GLOB_ALTDIRFUNC, for posix/src/glob.rs.",
        "# Generated by posix/tools/oracle/glob_harness.py; do not edit.",
        "# tree: " + " ".join(escape(t) for t in TREE),
    ]
    OUT.write_text("\n".join(head) + "\n" + r.stdout, encoding="utf-8", newline="\n")
    probes = [line for line in r.stdout.splitlines() if not line.startswith("pattern_p ")]
    print(f"{OUT.name}: {len(probes)} probes ({len(PATTERNS)} patterns x {len(FLAGS)} flag sets), "
          f"{len(PATTERN_P) * 2} of glob_pattern_p")

    import glob_model  # beside this file; see its docstring

    tree = glob_model.Tree(TREE)
    dev = [
        "# Every probe of glob_oracle.txt that posix/src/glob.rs answers otherwise than glibc",
        "# 2.39 -- design-decisions section 1149 says why -- as glob_model.py answers it: a",
        "# `glibc` line, the oracle's, and a `here` line, this library's, in the oracle's",
        "# format. Generated by posix/tools/oracle/glob_harness.py; do not edit.",
    ]
    for line in probes:
        fname, pat = line.split(" = ", 1)[0].split(" ", 1)
        ours = glob_model.answer(tree, fname, glob_model.unescape_line(pat))
        if ours != line:
            dev += [f"glibc {line}", f"here  {ours}"]
    DEVIATIONS.write_text("\n".join(dev) + "\n", encoding="utf-8", newline="\n")
    print(f"{DEVIATIONS.name}: {(len(dev) - 4) // 2} probes where this library's answer is not glibc's")


if __name__ == "__main__":
    main()
