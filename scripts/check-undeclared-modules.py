#!/usr/bin/env python3
"""Refuse a `.rs` file under a crate's `src/` that no `mod` declares.

Run: `python scripts/check-undeclared-modules.py` (0 = none beyond the
pinned; 1 = a new one, or a pin with nothing left to excuse; 2 = it could
not look). `--self-test` runs its fixtures.

**Why.** Cargo compiles what a crate's roots declare and nothing else. A file
under `src/` that no `mod` line names -- or names in a `#[path]` -- is not an
error and not a warning: rustc never opens it, so neither does `cargo test`,
`clippy` or `rustfmt`. Its tests never run, its claims are never checked, and
in review it reads exactly like code. On 2026-10-06 the file dialog's
automation (`gui/toolkit/src/dialog/accessible.rs`, three hundred lines and a
test file) sat like that for hours -- committed, called done in the roadmap,
and compiled by nothing, because `dialog.rs` never said `mod accessible;`.
`scan-orphan-modules.py` could not see it: that scan judges a module by
whether anything names its public items, and a file whose whole content is
an `impl` has none.

**How.** From each crate's roots -- `src/lib.rs`, `src/main.rs`, `src/bin/`,
and every `path =` its `Cargo.toml` names -- it follows `mod` declarations
the way rustc resolves them: `name.rs` or `name/mod.rs`; a crate root's or a
`mod.rs`'s children beside it, any other file's in a directory named after
it; inline `mod a { mod b; }` blocks; `#[path]`, relative to the declaring
file's directory (and an inline module's); and `include!`. Comments and
string literals -- raw ones, and ones continued across a line with a
backslash -- are skipped, so a `mod` in a doc comment declares nothing.
`cfg` is not evaluated: a file behind `#[cfg(...)]` is declared, which is
the right answer, since it is compiled wherever the cfg holds.

**Pinned.** A file left out on purpose is pinned in `PINNED` with why. A pin
whose file is now declared, or gone, fails the check as well: an exemption
for nothing is how a list stops describing the tree it exempts.
"""

import os
import pathlib
import re
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import selftestflag  # noqa: E402

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent

#: Files no `mod` declares, on purpose, by repository-relative path.
PINNED = {
    "rustcrypto/digest/src/dev/variable.rs":
        "vendored RustCrypto `digest` as it ships: its `dev.rs` declares "
        "`fixed`, `mac`, `rng` and `xof` but not `variable`, so upstream's own "
        "build never compiles it either. Left as vendored, so a re-vendor "
        "stays a copy.",
}

#: Directories that hold no crate of the tree's own: build output, git's.
SKIP_DIRS = {".git", "node_modules"}

TOKEN = re.compile(
    r'//[^\n]*'                                       # line comment
    r'|/\*.*?\*/'                                     # block comment
    r'|r(#*)".*?"\1'                                  # raw string
    r'|b?"(?:\\.|[^"\\])*"'                           # string, continuations too
    r"|b?'(?:\\.|[^'\\])'"                            # char literal
    r'|#\s*\[\s*path\s*=\s*"([^"]*)"\s*\]'            # path attribute
    r'|\binclude!\s*\(\s*"([^"]*)"\s*\)'              # include!
    r'|\bmod\s+(?:r#)?([A-Za-z_][A-Za-z0-9_]*)\s*([;{])'  # a module
    r'|[{}]',
    re.DOTALL,
)

#: A lifetime or a label -- `'a`, `'static`, `'outer:` -- which the char
#: literal pattern would otherwise read as the start of one.
LIFETIME = re.compile(r"'[A-Za-z_][A-Za-z0-9_]*\b(?!')")


def read(path):
    """The file's text, or None where it cannot be read."""
    try:
        with open(path, encoding="utf-8", errors="replace", newline="") as f:
            return f.read()
    except OSError:
        return None


def follow(path, mod_rs, reached):
    """Add every file `path` declares to `reached`, and theirs.

    `mod_rs` says whether `path` keeps its children beside it -- a crate root,
    a `mod.rs`, a file loaded by `#[path]` -- or in a directory of its name.
    """
    text = read(path)
    if text is None:
        return
    text = LIFETIME.sub("", text)
    here = os.path.dirname(path)
    stem = os.path.splitext(os.path.basename(path))[0]
    base = here if mod_rs else os.path.join(here, stem)
    inline = []     # the inline modules we are inside, outermost first
    opened = []     # the brace depth each of them opened at
    depth = 0
    path_attr = None
    for m in TOKEN.finditer(text):
        token = m.group(0)
        if token == "{":
            depth += 1
        elif token == "}":
            depth -= 1
            if opened and opened[-1] == depth:
                opened.pop()
                inline.pop()
        elif m.group(2) is not None:
            path_attr = m.group(2)
        elif m.group(3) is not None:
            reached.add(os.path.normpath(os.path.join(here, m.group(3))))
        elif m.group(4) is not None:
            name, end = m.group(4), m.group(5)
            if end == "{":
                inline.append(name)
                opened.append(depth)
                depth += 1
                path_attr = None
                continue
            if path_attr is not None:
                where = here if not inline else os.path.join(base, *inline)
                found = [(os.path.normpath(os.path.join(where, path_attr)), True)]
                path_attr = None
            else:
                where = os.path.join(base, *inline)
                found = [
                    (os.path.normpath(os.path.join(where, name + ".rs")), False),
                    (os.path.normpath(os.path.join(where, name, "mod.rs")), True),
                ]
            for child, child_mod_rs in found:
                if os.path.isfile(child):
                    if child not in reached:
                        reached.add(child)
                        follow(child, child_mod_rs, reached)
                    break


def crate_roots(crate):
    """The files cargo starts a crate's targets from."""
    src = os.path.join(crate, "src")
    roots = [os.path.join(src, n) for n in ("lib.rs", "main.rs")]
    bin_dir = os.path.join(src, "bin")
    if os.path.isdir(bin_dir):
        for entry in sorted(os.listdir(bin_dir)):
            p = os.path.join(bin_dir, entry)
            roots.append(p if entry.endswith(".rs") else os.path.join(p, "main.rs"))
    manifest = read(os.path.join(crate, "Cargo.toml")) or ""
    for p in re.findall(r'^\s*path\s*=\s*"([^"]+\.rs)"', manifest, re.M):
        roots.append(os.path.join(crate, p))
    return [os.path.normpath(r) for r in roots if os.path.isfile(r)]


def crates(root):
    """Every directory under `root` with a `Cargo.toml` and a `src/`."""
    for here, dirs, files in os.walk(root):
        dirs[:] = sorted(d for d in dirs if d not in SKIP_DIRS and not d.startswith("target"))
        if "Cargo.toml" in files and os.path.isdir(os.path.join(here, "src")):
            yield here


def undeclared(root):
    """Every `.rs` file under a crate's `src/` that nothing declares, as a
    `/`-separated path relative to `root`."""
    out = []
    for crate in crates(root):
        roots = crate_roots(crate)
        reached = set(roots)
        for r in roots:
            follow(r, True, reached)
        for here, dirs, files in os.walk(os.path.join(crate, "src")):
            dirs.sort()
            for f in sorted(files):
                p = os.path.normpath(os.path.join(here, f))
                if f.endswith(".rs") and p not in reached:
                    out.append(os.path.relpath(p, root).replace(os.sep, "/"))
    return sorted(set(out))


def check(root, pinned):
    """The new undeclared files, and the pins that excuse nothing."""
    found = undeclared(root)
    new = [p for p in found if p not in pinned]
    stale = [p for p in sorted(pinned) if p not in found]
    return new, stale


#: (what it shows, the files of a crate, the files it should call undeclared)
SELF_TESTS = [
    ("a file no mod names is undeclared",
     {"src/lib.rs": "pub fn f() {}\n", "src/stray.rs": "pub fn g() {}\n"},
     {"src/stray.rs"}),
    ("a file a mod names is not",
     {"src/lib.rs": "mod a;\n", "src/a.rs": ""},
     set()),
    ("a file's children live in a directory of its name",
     {"src/lib.rs": "mod a;\n", "src/a.rs": "mod b;\n", "src/a/b.rs": "", "src/b.rs": ""},
     {"src/b.rs"}),
    ("a mod.rs keeps its children beside it",
     {"src/lib.rs": "mod a;\n", "src/a/mod.rs": "mod b;\n", "src/a/b.rs": ""},
     set()),
    ("a #[path] is beside the file that declares it",
     {"src/lib.rs": "mod a;\n",
      "src/a.rs": '#[cfg(test)]\n#[path = "a_tests.rs"]\nmod tests;\n',
      "src/a_tests.rs": ""},
     set()),
    ("an inline module's children live in a directory of its name",
     {"src/lib.rs": "mod outer {\n    mod inner;\n}\nmod after;\n",
      "src/outer/inner.rs": "", "src/after.rs": ""},
     set()),
    ("a #[path] in an inline module is under the module's directory",
     {"src/lib.rs": 'mod outer {\n    #[path = "x.rs"]\n    mod inner;\n}\n',
      "src/outer/x.rs": ""},
     set()),
    ("a mod in a comment declares nothing",
     {"src/lib.rs": "// mod c;\n/* mod d; */\n/// mod e;\n",
      "src/c.rs": "", "src/d.rs": "", "src/e.rs": ""},
     {"src/c.rs", "src/d.rs", "src/e.rs"}),
    ("a mod in a string declares nothing",
     {"src/lib.rs": 'const S: &str = "mod c;";\nconst R: &str = r#"mod d;"#;\n',
      "src/c.rs": "", "src/d.rs": ""},
     {"src/c.rs", "src/d.rs"}),
    ("a string continued across a line does not swallow what follows",
     {"src/lib.rs": 'const S: &str = "one \\\n    two";\nmod after;\n', "src/after.rs": ""},
     set()),
    ("a lifetime and a quote character do not start a literal",
     {"src/lib.rs": "fn f<'a>(x: &'a str) -> char { '\"' }\nmod after;\n",
      "src/after.rs": ""},
     set()),
    ("a bin is a root, its children beside it",
     {"src/main.rs": "", "src/bin/tool.rs": "mod helper;\n", "src/bin/helper.rs": ""},
     set()),
    ("a file include! names is used",
     {"src/lib.rs": 'include!("generated.rs");\n', "src/generated.rs": ""},
     set()),
    ("a target the manifest names is a root",
     {"src/lib.rs": "", "src/extra/run.rs": "mod part;\n", "src/extra/part.rs": "",
      "Cargo.toml": '[package]\nname = "t"\n\n[[bin]]\nname = "run"\npath = "src/extra/run.rs"\n'},
     set()),
]


def write_crate(top, files):
    """Lay `files` out under `top`, a fresh temporary directory."""
    files = dict(files)
    files.setdefault("Cargo.toml", '[package]\nname = "t"\n')
    for rel, text in files.items():
        p = os.path.join(top, *rel.split("/"))
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w", encoding="utf-8", newline="") as f:
            f.write(text)


def self_test():
    """Each fixture crate's undeclared files, against what they should be;
    then a pin for a declared file, and one for a missing file, refused."""
    bad = 0
    for label, files, want in SELF_TESTS:
        with tempfile.TemporaryDirectory() as top:
            write_crate(top, files)
            got = set(undeclared(top))
        ok = got == want
        bad += not ok
        print(f"{'ok  ' if ok else 'FAIL'}  {label}: {sorted(got)} (want {sorted(want)})")
    with tempfile.TemporaryDirectory() as top:
        write_crate(top, {"src/lib.rs": "mod a;\n", "src/a.rs": "", "src/b.rs": ""})
        new, stale = check(top, {"src/a.rs": "declared now", "src/gone.rs": "deleted"})
    ok = new == ["src/b.rs"] and stale == ["src/a.rs", "src/gone.rs"]
    bad += not ok
    print(f"{'ok  ' if ok else 'FAIL'}  pins that excuse nothing are refused: new {new}, stale {stale}")
    print(f"\n{len(SELF_TESTS) + 1} cases, {bad} failed")
    return 1 if bad else 0


def main(argv):
    if selftestflag.wants_selftest(argv):
        return self_test()
    unknown = selftestflag.unknown_options(argv)
    if unknown:
        print(f"check-undeclared-modules: unknown option(s): {' '.join(unknown)}", file=sys.stderr)
        return 2
    if not (ROOT / "Cargo.toml").is_file():
        print(f"check-undeclared-modules: no workspace at {ROOT}", file=sys.stderr)
        return 2
    new, stale = check(str(ROOT), PINNED)
    for p in new:
        print(f"UNDECLARED  {p}: no `mod` names it, so nothing compiles it -- "
              f"declare it in its parent, delete it, or pin it in PINNED with why")
    for p in stale:
        print(f"STALE PIN   {p}: declared now, or gone -- delete its pin")
    print(f"check-undeclared-modules: {len(new)} undeclared, {len(stale)} stale pin(s), "
          f"{len(PINNED)} pinned")
    return 1 if new or stale else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
