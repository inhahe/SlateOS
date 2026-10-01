r"""glob, as posix/src/glob.rs does it, written a second way: from POSIX (XSH
glob, XCU 2.13.3) and glibc's manual for its flags, and held to glibc 2.39's
answers (glob_oracle.txt) wherever those leave a case open.

It exists for the probes where glibc's answer contradicts POSIX or glibc's
own answers to the same question asked another way -- design-decisions
section 1149 -- and glob_harness.py uses it to write glob_deviations.txt:
each such probe, glibc's answer and this one, which posix/src/glob.rs's
tests hold the Rust to instead of glibc's. The two:

- One character before a trailing slash -- `*/`, `?/` -- takes a path of
  its own in glibc, meant for the pattern `/`: GLOB_MARK doubles the slash
  (`dir1//`), GLOB_PERIOD does not let the wildcard match "." or "..", and
  GLOB_MAGCHAR is not set. `**/` and `[!x]/`, which match the same names,
  are answered otherwise on all three counts, as is the `*/` of `*/*/`.
  Here `*/` is answered as glibc answers `**/`, and `?/` as `[!x]/`.
- GLOB_NOCHECK for a pattern ending in a slash, two or more characters
  before it (`??/`): POSIX's answer is "a list consisting of only pattern";
  glibc's drops the slash (`??`), where for `a/` and `?/` it keeps it.

The tree is the harness's own (a TREE line, directories with a trailing
slash), read through the same five functions it gives glibc, with the same
lookup rules: "." and ".." resolved, ".." back along the path taken, the
empty path the current directory.

    python posix/tools/oracle/glob_model.py posix/src/glob_oracle.txt [N [FLAGS [M]]]

prints how many of the oracle's probes the model answers differently from
glibc -- the first N of them, only those under FLAGS if given -- and how many
more differ in GLOB_MAGCHAR alone (the first M).
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import fnmatch_model as fm  # noqa: E402  (beside this file)

GLOB_ERR, GLOB_MARK, GLOB_NOSORT, GLOB_DOOFFS, GLOB_NOCHECK, GLOB_APPEND = 1, 2, 4, 8, 0x10, 0x20
GLOB_NOESCAPE, GLOB_PERIOD, GLOB_MAGCHAR, GLOB_ALTDIRFUNC, GLOB_BRACE = 0x40, 0x80, 0x100, 0x200, 0x400
GLOB_NOMAGIC, GLOB_TILDE, GLOB_ONLYDIR, GLOB_TILDE_CHECK = 0x800, 0x1000, 0x2000, 0x4000
GLOB_NOSPACE, GLOB_ABORTED, GLOB_NOMATCH = 1, 2, 3
ENOENT, ENOTDIR, EACCES = 2, 20, 13
FLAG_VALUES = {"0": 0, "MARK": GLOB_MARK, "NOSORT": GLOB_NOSORT, "NOCHECK": GLOB_NOCHECK,
               "NOESCAPE": GLOB_NOESCAPE, "PERIOD": GLOB_PERIOD, "BRACE": GLOB_BRACE,
               "NOMAGIC": GLOB_NOMAGIC, "TILDE": GLOB_TILDE, "TILDE_CHECK": GLOB_TILDE_CHECK,
               "ONLYDIR": GLOB_ONLYDIR, "ERR": GLOB_ERR, "ERRSTOP": 0}
HOMES = {"root": "/root"}  # getpwnam's answers, as the harness's system has them
HOME = "/home/u"           # the harness sets HOME


class Tree:
    """The harness's in-memory directory tree and its callbacks."""

    def __init__(self, entries):
        self.children = {"": [], "/": []}  # node -> [child names] in tree order
        self.dirs = {"", "/"}
        for e in entries:
            is_dir = e.endswith("/")
            path = e[:-1] if is_dir else e
            parent, _, name = path.rpartition("/")
            if path.startswith("/") and parent == "":
                parent = "/"
            self.children.setdefault(parent, []).append(name)
            if is_dir:
                self.dirs.add(path)
                self.children.setdefault(path, [])

    @staticmethod
    def child(parent, name):
        if parent == "":
            return name
        if parent == "/":
            return "/" + name
        return parent + "/" + name

    def lookup(self, path):
        """(node, errno): the node a path names, "." and ".." resolved."""
        node = "/" if path.startswith("/") else ""
        taken = []
        for comp in path.split("/"):
            if comp in ("", "."):
                continue
            if comp == "..":
                if taken:
                    node = taken.pop()
                continue
            if node not in self.dirs:
                return None, ENOTDIR
            if comp not in self.children.get(node, []):
                return None, ENOENT
            taken.append(node)
            node = self.child(node, comp)
        if path.endswith("/") and node not in self.dirs:
            return None, ENOTDIR
        return node, 0

    def opendir(self, path):
        """([(name, is_dir)], 0), or (None, errno)."""
        node, err = self.lookup(path)
        if node is None:
            return None, err
        if node not in self.dirs:
            return None, ENOTDIR
        if node == "noperm":
            return None, EACCES
        names = [".", ".."] + self.children.get(node, [])
        return [(n, n in (".", "..") or self.child(node, n) in self.dirs) for n in names], 0

    def stat(self, path):
        """(exists, is_dir)."""
        node, _ = self.lookup(path)
        return (node is not None, node is not None and node in self.dirs)


def has_magic(p, flags):
    """Does the pattern hold an unescaped `*`, `?` or `[`?"""
    i = 0
    while i < len(p):
        c = p[i]
        if c == "\\" and not flags & GLOB_NOESCAPE:
            i += 2
            continue
        if c in "*?[":
            return True
        i += 1
    return False


def needs_scan(p, flags):
    """Is the pattern matched against a directory's entries, rather than
    looked up? When it holds a wildcard, or an escape."""
    return has_magic(p, flags) or (not flags & GLOB_NOESCAPE and "\\" in p)


def unescape(p, flags):
    if flags & GLOB_NOESCAPE:
        return p
    out, i = [], 0
    while i < len(p):
        if p[i] == "\\" and i + 1 < len(p):
            out.append(p[i + 1])
            i += 2
        else:
            out.append(p[i])
            i += 1
    return "".join(out)


def brace_expand(p, flags):
    """The patterns a pattern's first brace group stands for, each expanded
    again; [p] if it has no balanced group."""
    i = 0
    while i < len(p):
        c = p[i]
        if c == "\\" and not flags & GLOB_NOESCAPE:
            i += 2
            continue
        if c == "{":
            depth, j, commas = 0, i, []
            while j < len(p):
                d = p[j]
                if d == "\\" and not flags & GLOB_NOESCAPE:
                    j += 2
                    continue
                if d == "{":
                    depth += 1
                elif d == "}":
                    depth -= 1
                    if depth == 0:
                        break
                elif d == "," and depth == 1:
                    commas.append(j)
                j += 1
            if j >= len(p):
                return [p]
            bounds = [i] + commas + [j]
            out = []
            for a, b in zip(bounds, bounds[1:]):
                out.extend(brace_expand(p[:i] + p[a + 1:b] + p[j + 1:], flags))
            return out
        i += 1
    return [p]


class Aborted(Exception):
    pass


class Globber:
    def __init__(self, tree, flags, stop, errors):
        self.tree, self.flags, self.stop, self.errors = tree, flags, stop, errors
        # GLOB_MAGCHAR, glibc's way: a last component read from a directory
        # that found something -- or that GLOB_NOCHECK would answer for.
        self.magchar = False
        # For GLOB_NOCHECK's answer: a wildcard directory part that found a
        # directory.
        self.dir_magic_found = False

    def fnm_flags(self):
        f = 0 if self.flags & GLOB_PERIOD else fm.PERIOD
        if self.flags & GLOB_NOESCAPE:
            f |= fm.NOESCAPE
        return f

    def in_dir(self, dirname, prefix, pat, only_dirs):
        """The entries of `dirname` ("" the current directory) that `pat`
        matches, as `prefix` and their names."""
        opened = dirname if dirname else "."
        entries, err = self.tree.opendir(opened)
        if entries is None:
            if err != ENOTDIR:
                self.errors.append((opened, err))
                if self.stop or self.flags & GLOB_ERR:
                    raise Aborted
            return []
        out = []
        for name, is_dir in entries:
            if only_dirs and not is_dir:
                continue
            if fm.match(pat, 0, name, 0, len(name), self.fnm_flags()):
                out.append((prefix + name, is_dir))
        if out or self.flags & GLOB_NOCHECK:
            self.magchar = True
        return out

    def look_up(self, path):
        # The empty pathname names no file (XBD 4.13).
        if not path:
            return []
        exists, is_dir = self.tree.stat(path)
        return [(path, is_dir)] if exists else []

    def expand(self, pattern, only_dirs):
        """(path, is_dir) for everything `pattern` names."""
        if not needs_scan(pattern, self.flags):
            return self.look_up(unescape(pattern, self.flags))
        if pattern.endswith("/") and len(pattern) > 1:
            # "X/": the directories X names, each with its slash back -- X's
            # last component read with the caller's own flags.
            return [(x + "/", True) for x, d in self.expand(pattern[:-1], True) if d]
        slash = pattern.rfind("/")
        if slash < 0:
            return self.in_dir("", "", pattern, only_dirs)
        dirpart, filepart = pattern[:slash], pattern[slash + 1:]
        if dirpart == "":
            dirs = [("/", True)]
        elif needs_scan(dirpart, self.flags):
            # The directories, with only the flags that say how to read them:
            # a wildcard there never matches "." or ".." by GLOB_PERIOD, and
            # nothing is marked, sorted or answered for NOCHECK.
            sub = Globber(self.tree, (self.flags & (GLOB_ERR | GLOB_NOESCAPE)) | GLOB_NOSORT | GLOB_ONLYDIR,
                          self.stop, self.errors)
            dirs = sub.expand(dirpart, True)
            if dirs:
                self.dir_magic_found = True
        else:
            dirs = [(unescape(dirpart, self.flags), True)]
        out = []
        for d, _ in dirs:
            prefix = "/" if d == "/" else d + "/"
            if needs_scan(filepart, self.flags):
                out.extend(self.in_dir(d, prefix, filepart, only_dirs))
            else:
                out.extend(self.look_up(prefix + unescape(filepart, self.flags)))
        return out


def tilde(pattern, flags):
    """(pattern, bare): a leading ~ or ~user expanded, and whether that was
    all of it; None for GLOB_TILDE_CHECK's unknown user."""
    if not pattern.startswith("~") or not flags & (GLOB_TILDE | GLOB_TILDE_CHECK):
        return pattern, False
    slash = pattern.find("/")
    user = pattern[1:slash] if slash >= 0 else pattern[1:]
    rest = pattern[slash:] if slash >= 0 else ""
    home = HOME if user == "" else HOMES.get(user)
    if home is None:
        if flags & GLOB_TILDE_CHECK:
            return None
        return pattern, rest == ""
    return home + rest, rest == ""


def glob(tree, pattern, flags, stop=False):
    """(return, magchar, paths, [(path, errno)] the errfunc was told)."""
    errors = []
    g = Globber(tree, flags, stop, errors)
    pats = brace_expand(pattern, flags) if flags & GLOB_BRACE else [pattern]
    paths = []
    try:
        for p in pats:
            t = tilde(p, flags)
            if t is None:
                continue
            t, bare = t
            if bare:
                # `~` or `~user` alone: the name as it is, looked for nowhere.
                r = [(t, tree.stat(t)[1])]
            else:
                r = g.expand(t, bool(flags & GLOB_ONLYDIR))
            # Marked -- a name ending in a slash is marked already -- then
            # sorted, each pattern's names apart.
            r = [x + "/" if flags & GLOB_MARK and d and not x.endswith("/") else x for x, d in r]
            if not flags & GLOB_NOSORT:
                r.sort(key=lambda s: s.encode("latin-1"))
            paths.extend(r)
    except Aborted:
        return GLOB_ABORTED, g.magchar, [], errors
    if not paths:
        if flags & GLOB_NOCHECK or (flags & GLOB_NOMAGIC and pattern and not needs_scan(pattern, flags)):
            return 0, g.magchar or g.dir_magic_found, [pattern], errors
        return GLOB_NOMATCH, False, [], errors
    return 0, g.magchar, paths, errors


def escape(s):
    r"""As the harness writes a name: `\xNN` for a backslash, a space and
    anything outside printable ASCII; `\x` alone for the empty string."""
    if not s:
        return "\\x"
    return "".join(f"\\x{ord(c):02x}" if c in "\\ " or not "!" <= c <= "~" else c for c in s)


def unescape_line(t):
    out, i = [], 0
    while i < len(t):
        if t[i] == "\\" and t[i + 1:i + 2] == "x":
            if i + 4 <= len(t):
                out.append(chr(int(t[i + 2:i + 4], 16)))
                i += 4
            else:
                i += 2
        else:
            out.append(t[i])
            i += 1
    return "".join(out)


def answer(tree, fname, pattern):
    """A probe's line, as the harness writes glibc's, answered here."""
    flags = sum(FLAG_VALUES[f] for f in fname.split("|"))
    ret, mag, paths, errors = glob(tree, pattern, flags, stop=fname == "ERRSTOP")
    line = f"{fname} {escape(pattern)} = {ret} {int(mag)}"
    line += "".join(" " + escape(p) for p in paths)
    return line + " |" + "".join(f" {escape(p)}:{e}" for p, e in errors)


def tree_of(oracle_lines):
    line = next(line for line in oracle_lines if line.startswith("# tree: "))
    return Tree([unescape_line(t) for t in line[len("# tree: "):].split(" ")])


def main():
    lines = Path(sys.argv[1]).read_text(encoding="utf-8").splitlines()
    tree = tree_of(lines)
    limit = int(sys.argv[2]) if len(sys.argv) > 2 else 40
    only = sys.argv[3] if len(sys.argv) > 3 and sys.argv[3] else None
    bad, magbad = [], []
    for line in lines:
        if line.startswith("#") or line.startswith("pattern_p"):
            continue
        head = line.split(" = ", 1)[0]
        fname, pat = head.split(" ", 1)
        if only and fname != only:
            continue
        ours = answer(tree, fname, unescape_line(pat))
        if ours == line:
            continue
        # The same but for the magchar field?
        g, o = line.split(" "), ours.split(" ")
        at = g.index("=") + 2
        if g[:at] + g[at + 1:] == o[:at] + o[at + 1:]:
            magbad.append(f"{head}: glibc {g[at]}, model {o[at]}")
        else:
            bad.append(f"glibc {line}\nmodel {ours}")
    print(f"{len(bad)} probes differ (return, paths, errfunc calls)")
    for b in bad[:limit]:
        print(b)
    print(f"{len(magbad)} more differ only in GLOB_MAGCHAR")
    for m in magbad[:int(sys.argv[4]) if len(sys.argv) > 4 else 0]:
        print("  ", m)


if __name__ == "__main__":
    main()
