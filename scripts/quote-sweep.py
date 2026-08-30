#!/usr/bin/env python3
"""One-shot: route every file name in a coreutils diagnostic through `quote`.

Kept only so the sweep it performed can be re-read, not because it should be
run again -- rerunning it is a no-op by construction, since every line it
rewrites stops matching once rewritten. New diagnostics should call the
quoting functions when they are written.
"""

import re
from pathlib import Path

BIN = Path("userspace/coreutils/src/bin")


def write(p: Path, text: str) -> None:
    """Write with LF endings whatever the host is.

    `Path.write_text` translates on Windows, which rewrote every line of all
    50 files it touched here and buried the real change in the diff.
    """
    with open(p, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


# Names embedded in a sentence: GNU's `quoteaf`, which never goes bare. These
# sites already hand-wrote the quotes, which is exactly the bug -- a name
# holding a quote, a newline or a control byte defeated them.
AF = [
    ("cp.rs", """eprintln!("cp: target '{dest}' is not a directory");""",
     """eprintln!("cp: target {} is not a directory", quoteaf_os(dest));""", 1),
    ("cp.rs", """eprintln!("cp: omitting directory '{src_str}'");""",
     """eprintln!("cp: omitting directory {}", quoteaf_os(src_str));""", 1),
    ("cp.rs", """eprintln!("cp: error copying '{src_str}': {e}");""",
     """eprintln!("cp: error copying {}: {e}", quoteaf_os(src_str));""", 1),
    ("dd.rs", """eprintln!("dd: failed to open '{path}': {e}");""",
     """eprintln!("dd: failed to open {}: {e}", quoteaf_os(path));""", 2),
    # A URL is not a file name and never was a shell word, so it takes
    # `quote` -- always quoted, C escapes -- rather than either shell style.
    ("fetch.rs", """eprintln!("fetch: invalid URL '{url_str}': {e}");""",
     """eprintln!("fetch: invalid URL {}: {e}", quote_os(url_str));""", 1),
    ("fetch.rs", """eprintln!("fetch: invalid redirect URL '{location}': {e}");""",
     """eprintln!("fetch: invalid redirect URL {}: {e}", quote_os(location));""", 1),
    ("find.rs", """eprintln!("find: '{}': {e}", dir.display());""",
     """eprintln!("find: {}: {e}", quoteaf_os(dir));""", 1),
    ("ls.rs", """eprintln!("ls: cannot access '{path}': {e}");""",
     """eprintln!("ls: cannot access {}: {e}", quoteaf_os(path));""", 1),
    ("mkdir.rs", """eprintln!("mkdir: cannot create directory '{dir}': {e}");""",
     """eprintln!("mkdir: cannot create directory {}: {e}", quoteaf_os(dir));""", 1),
    ("mkfifo.rs", """eprintln!("mkfifo: cannot create fifo '{name}'");""",
     """eprintln!("mkfifo: cannot create fifo {}", quoteaf_os(name));""", 1),
    ("mv.rs", """eprintln!("mv: target '{dest}' is not a directory");""",
     """eprintln!("mv: target {} is not a directory", quoteaf_os(dest));""", 1),
    ("mv.rs", """eprintln!("mv: cannot move '{src_str}' to '{}': {e}", target.display());""",
     """eprintln!("mv: cannot move {} to {}: {e}", quoteaf_os(src_str), quoteaf_os(&target));""", 1),
    ("rm.rs", """eprintln!("rm: cannot remove '{path_str}': No such file or directory");""",
     """eprintln!("rm: cannot remove {}: No such file or directory", quoteaf_os(path_str));""", 1),
    ("rm.rs", """eprintln!("rm: cannot remove '{path_str}': Is a directory");""",
     """eprintln!("rm: cannot remove {}: Is a directory", quoteaf_os(path_str));""", 1),
    ("rm.rs", """eprintln!("rm: cannot remove '{path_str}': {e}");""",
     """eprintln!("rm: cannot remove {}: {e}", quoteaf_os(path_str));""", 1),
    ("rmdir.rs", """eprintln!("rmdir: failed to remove '{dir}': {e}");""",
     """eprintln!("rmdir: failed to remove {}: {e}", quoteaf_os(dir));""", 1),
    ("stat.rs", """eprintln!("stat: cannot stat '{path_str}': {e}");""",
     """eprintln!("stat: cannot stat {}: {e}", quoteaf_os(path_str));""", 1),
    ("touch.rs", """eprintln!("touch: cannot touch '{path}': {e}");""",
     """eprintln!("touch: cannot touch {}: {e}", quoteaf_os(path));""", 1),
]

# Names that end the message -- `PROG: NAME: reason` -- take `quotef`, which
# leaves an ordinary name alone. These are given explicitly because the name
# arrives as something other than a plain `{ident}`.
F_EXPLICIT = [
    ("cat.rs", """eprintln!("cat: {}: {}", path.to_string_lossy(), strerror(&e));""",
     """eprintln!("cat: {}: {}", quotef_os(path), strerror(&e));""", 1),
    ("cmp.rs", """eprintln!("cmp: {}: {e}", files[0]);""",
     """eprintln!("cmp: {}: {e}", quotef_os(&files[0]));""", 2),
    ("cmp.rs", """eprintln!("cmp: {}: {e}", files[1]);""",
     """eprintln!("cmp: {}: {e}", quotef_os(&files[1]));""", 2),
    ("diff.rs", """eprintln!("diff: {}: {e}", files[0]);""",
     """eprintln!("diff: {}: {e}", quotef_os(&files[0]));""", 1),
    ("diff.rs", """eprintln!("diff: {}: {e}", files[1]);""",
     """eprintln!("diff: {}: {e}", quotef_os(&files[1]));""", 1),
    ("du.rs", """eprintln!("du: {}: {e}", entry_path.display());""",
     """eprintln!("du: {}: {e}", quotef_os(&entry_path));""", 2),
    ("grep.rs", """eprintln!("grep: {}: {}", dir.display(), strerror(&e));""",
     """eprintln!("grep: {}: {}", quotef_os(dir), strerror(&e));""", 1),
    ("uniq.rs", """eprintln!("uniq: {}: {e}", files[0]);""",
     """eprintln!("uniq: {}: {e}", quotef_os(&files[0]));""", 1),
    ("uniq.rs", """eprintln!("uniq: {}: {e}", files[1]);""",
     """eprintln!("uniq: {}: {e}", quotef_os(&files[1]));""", 1),
    # `subject` is cat's "what failed" helper: a path in one branch and the
    # fixed words "write error" in the other. Only the path may be quoted.
    ("cat.rs", """            path.to_string_lossy().into_owned()""",
     """            quotef_os(path)""", 1),
]

# The regular shape: `eprintln!("prog: {name}: ...");`, with or without
# trailing format arguments. A capture that is a message rather than a name
# would be a false positive, so every rewrite is printed for review.
NO_ARGS = re.compile(r'^(\s*)eprintln!\("([a-z0-9_]+): \{(\w+)\}: ([^"]*)"\);$')
WITH_ARGS = re.compile(r'^(\s*)eprintln!\("([a-z0-9_]+): \{(\w+)\}: ([^"]*)", (.*)\);$')


def main() -> int:
    touched: dict[Path, set[str]] = {}

    def apply(name, old, new, count, func):
        p = BIN / name
        text = p.read_text(encoding="utf-8")
        got = text.count(old)
        if got != count:
            print(f"!! {name}: expected {count} of {old!r}, found {got}")
            return False
        write(p, text.replace(old, new))
        touched.setdefault(p, set()).add(func)
        return True

    ok = True
    for name, old, new, count in AF:
        func = "quote_os" if "quote_os(" in new else "quoteaf_os"
        ok &= apply(name, old, new, count, func)
    for name, old, new, count in F_EXPLICIT:
        ok &= apply(name, old, new, count, "quotef_os")
    if not ok:
        return 1

    for p in sorted(BIN.rglob("*.rs")):
        lines = p.read_text(encoding="utf-8").split("\n")
        changed = False
        for i, line in enumerate(lines):
            m = NO_ARGS.match(line)
            if m:
                indent, prog, var, rest = m.groups()
                lines[i] = (f'{indent}eprintln!("{prog}: {{}}: {rest}", '
                            f'quotef_os({var}));')
            else:
                m = WITH_ARGS.match(line)
                if not m:
                    continue
                indent, prog, var, rest, args = m.groups()
                lines[i] = (f'{indent}eprintln!("{prog}: {{}}: {rest}", '
                            f'quotef_os({var}), {args});')
            print(f"{p.name}:{i + 1}\n  - {line.strip()}\n  + {lines[i].strip()}")
            changed = True
        if changed:
            write(p, "\n".join(lines))
            touched.setdefault(p, set()).add("quotef_os")

    for p, funcs in sorted(touched.items()):
        lines = p.read_text(encoding="utf-8").split("\n")
        if any(l.startswith("use coreutils::quote::") for l in lines):
            continue
        first = next(i for i, l in enumerate(lines) if l.startswith("use "))
        items = ", ".join(sorted(funcs))
        decl = (f"use coreutils::quote::{{{items}}};" if len(funcs) > 1
                else f"use coreutils::quote::{items};")
        lines.insert(first, decl)
        write(p, "\n".join(lines))

    print(f"\n{len(touched)} files touched")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
