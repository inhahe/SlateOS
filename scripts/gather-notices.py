#!/usr/bin/env python3
"""Gather the third-party notices a SlateOS image must carry (design-decisions §1433).

Code copied or translated from someone else's project is ours to ship only if
their notice ships with it: the BSD licences ask for their copyright notice and
licence text "in the documentation and/or other materials provided with the
distribution", MIT for its notice "in all copies or substantial portions", and
libjpeg-turbo's IJG licence for one exact sentence. This collects every such
notice the source tree carries, from three places:

* **Manifests.** A `licenses/notices.yaml` anywhere in the tree names the
  third-party code its crate carries -- ported C libraries, typically -- and
  the text files beside it that must travel with a binary:

      # One key per upstream component.
      libjpeg-turbo:
        version: 3.1.1
        licence: IJG AND BSD-3-Clause AND Zlib
        attribution: This software is based in part on the work of the Independent JPEG Group.
        texts:
          - libjpeg-turbo-LICENSE.md
          - libjpeg-turbo-README.ijg

  `version` and `attribution` are optional; `licence` and `texts` are not.
  `attribution` is for a sentence a licence requires to be *shown*, word for
  word. Text paths are relative to the manifest's directory.

* **Vendored Rust crates.** A Cargo package whose root holds `LICENSE*`,
  `COPYING*`, `NOTICE*` or `UNLICENSE*` files is someone else's code (this
  project's own crates carry none). Its `Cargo.toml` names it, versions it and
  gives its licence; those files are its texts. No manifest is needed.

* **crates.io libraries.** Every registry package in `Cargo.lock`, read from
  cargo's registry cache -- which the build that uses them has already filled.
  Two exceptions. A package that ships no licence file but is a procedural
  macro (`[lib] proc-macro = true`) is skipped: it runs inside the compiler
  and none of it is in anything the image carries. And a package a manifest
  names -- same name, same version -- takes the manifest's notice, which is
  how a package that ships no licence file is given one by hand.

Usage:

    python scripts/gather-notices.py --check          # gather, report, write nothing
    python scripts/gather-notices.py --out DIR        # write the bundle into DIR
    python scripts/gather-notices.py --list           # one line per notice
    python scripts/gather-notices.py --self-test

`--out` writes `DIR/index.yaml` (the list, which `gui/notices` reads),
`DIR/<key>/<file>` (each text, byte for byte) and `DIR/NOTICES.txt` (everything
in one file, for a person reading without the screen). `DIR` must not exist or
must be empty: the bundle is written whole or not at all, never merged into a
stale one. In the image it is `/usr/share/licenses` (`notices::SYSTEM_DIR`).

Exit status: 0 on success; 1 when a notice cannot be gathered (a malformed
manifest, a text that is not there, a crate with no licence files, a registry
package missing from the cache); 2 for a usage error. A notice that cannot be
gathered is an error rather than a warning because the bundle's whole job is
to be complete: an image shipped with one notice quietly missing is the
failure this exists to prevent.
"""

from __future__ import annotations

import os
import re
import sys
import tempfile
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

#: The index `gui/notices` reads, and the file a person reads.
INDEX = "index.yaml"
READABLE = "NOTICES.txt"

#: Directories the walk never enters: version control, build output (every
#: `target`, `target-lint`, `target-check`...), and copies kept for history.
SKIP_DIRS = frozenset({".git", "backups", "node_modules", "__pycache__", ".venv"})

#: A licence text at a crate's root, by the names crates.io and upstream
#: projects use: `LICENSE`, `LICENSE-MIT`, `LICENCE.txt`, `COPYING`, `NOTICE`,
#: `UNLICENSE`.
LICENCE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE)([-._].*)?$", re.IGNORECASE)

MANIFEST = "notices.yaml"
FIELDS = ("version", "licence", "attribution", "texts")


class NoticeError(Exception):
    """A notice that cannot be gathered, with where and why."""


@dataclass
class Notice:
    """One component whose notice the image carries."""

    component: str
    licence: str
    #: Where in the tree it came from (a crate directory, POSIX-style), or
    #: `crates.io` for a registry package.
    origin: str
    texts: list[Path]
    #: `manifest`, `vendored` or `crates.io`: which of the three sources.
    kind: str = "manifest"
    version: str | None = None
    attribution: str | None = None
    #: Assigned by `gather`: unique, and safe as a directory name.
    key: str = field(default="")

    def sort_key(self):
        return (self.component.casefold(), self.version or "", self.origin)


# ---------------------------------------------------------------------------
# The manifest: a small, strict YAML subset
# ---------------------------------------------------------------------------

def _scalar(raw: str, where: str) -> str:
    """A plain, single-quoted or double-quoted scalar, its comment removed."""
    raw = raw.strip()
    if raw.startswith("'"):
        end = 1
        out = []
        while True:
            q = raw.find("'", end)
            if q < 0:
                raise NoticeError(f"{where}: unterminated single-quoted value")
            out.append(raw[end:q])
            if raw[q + 1:q + 2] == "'":
                out.append("'")
                end = q + 2
                continue
            rest = raw[q + 1:].strip()
            if rest and not rest.startswith("#"):
                raise NoticeError(f"{where}: text after a quoted value: {rest!r}")
            return "".join(out)
    if raw.startswith('"'):
        out = []
        i = 1
        while i < len(raw):
            ch = raw[i]
            if ch == "\\" and i + 1 < len(raw):
                out.append({"n": "\n", "t": "\t", "\\": "\\", '"': '"'}.get(raw[i + 1], raw[i + 1]))
                i += 2
                continue
            if ch == '"':
                rest = raw[i + 1:].strip()
                if rest and not rest.startswith("#"):
                    raise NoticeError(f"{where}: text after a quoted value: {rest!r}")
                return "".join(out)
            out.append(ch)
            i += 1
        raise NoticeError(f"{where}: unterminated double-quoted value")
    # Plain: a ` #` starts a comment, as in YAML.
    hash_at = raw.find(" #")
    return (raw[:hash_at] if hash_at >= 0 else raw).strip()


def parse_manifest(text: str, where: str) -> dict[str, dict]:
    """`{component: {field: value}}` from a notices manifest.

    Strict on purpose: a manifest is a legal record, and a field this does not
    know is more likely a misspelt `licence` than something to ignore.
    """
    entries: dict[str, dict] = {}
    current = None
    in_texts = False
    for number, line in enumerate(text.splitlines(), start=1):
        at = f"{where}:{number}"
        if "\t" in line[: len(line) - len(line.lstrip())]:
            raise NoticeError(f"{at}: indented with a tab")
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(line) - len(line.lstrip(" "))
        if indent == 0:
            if not stripped.endswith(":"):
                raise NoticeError(f"{at}: a component is a key ending in ':', got {stripped!r}")
            name = _scalar(stripped[:-1], at)
            if not name:
                raise NoticeError(f"{at}: a component with no name")
            if name in entries:
                raise NoticeError(f"{at}: {name!r} is listed twice")
            current = entries[name] = {}
            in_texts = False
            continue
        if current is None:
            raise NoticeError(f"{at}: indented line before any component")
        if stripped.startswith("- "):
            if not in_texts or indent < 4:
                raise NoticeError(f"{at}: a list item outside `texts:`")
            current["texts"].append(_scalar(stripped[2:], at))
            continue
        if indent != 2:
            raise NoticeError(f"{at}: a field is indented by two spaces")
        key, sep, value = stripped.partition(":")
        key = key.strip()
        if not sep or key not in FIELDS:
            raise NoticeError(f"{at}: unknown field {key!r} (known: {', '.join(FIELDS)})")
        if key in current:
            raise NoticeError(f"{at}: {key!r} given twice")
        in_texts = key == "texts"
        if in_texts:
            if value.strip() and not value.strip().startswith("#"):
                raise NoticeError(f"{at}: `texts:` is a list, one `- file` per line")
            current["texts"] = []
        else:
            current[key] = _scalar(value, at)
    for name, entry in entries.items():
        for required in ("licence", "texts"):
            if not entry.get(required):
                raise NoticeError(f"{where}: {name!r} has no {required}")
    return entries


def _origin(directory: Path, root: Path) -> str:
    """The crate a manifest speaks for: its directory, or that directory's
    parent when the manifest sits in the conventional `licenses/`."""
    crate = directory.parent if directory.name == "licenses" else directory
    return crate.relative_to(root).as_posix() or "."


def from_manifest(path: Path, root: Path) -> list[Notice]:
    where = path.relative_to(root).as_posix()
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError as exc:
        raise NoticeError(f"{where}: not UTF-8 ({exc})") from exc
    notices = []
    for name, entry in parse_manifest(text, where).items():
        texts = []
        for rel in entry["texts"]:
            source = (path.parent / rel).resolve()
            if not source.is_file():
                raise NoticeError(f"{where}: {name!r} names {rel!r}, which is not there")
            texts.append(source)
        notices.append(Notice(
            component=name,
            version=entry.get("version") or None,
            licence=entry["licence"],
            attribution=entry.get("attribution") or None,
            origin=_origin(path.parent, root),
            texts=texts,
        ))
    return notices


# ---------------------------------------------------------------------------
# The walk
# ---------------------------------------------------------------------------

def walk(root: Path):
    """`(directory, file names)` for every directory the notices can be in."""
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS and not d.startswith("target"))
        yield Path(dirpath), filenames


def _package(cargo_toml: Path, where: str) -> dict:
    try:
        return tomllib.loads(cargo_toml.read_text(encoding="utf-8")).get("package", {})
    except (tomllib.TOMLDecodeError, UnicodeDecodeError) as exc:
        raise NoticeError(f"{where}: cannot read Cargo.toml ({exc})") from exc


def _crate_notice(directory: Path, licence_files: list[str], origin: str, where: str,
                  kind: str) -> Notice:
    package = _package(directory / "Cargo.toml", where)
    name, version, licence = package.get("name"), package.get("version"), package.get("license")
    if not isinstance(name, str) or not isinstance(version, str):
        raise NoticeError(f"{where}: Cargo.toml gives no plain name and version")
    if not isinstance(licence, str) or not licence.strip():
        raise NoticeError(f"{where}: {name} {version} carries licence files but names no licence")
    return Notice(component=name, version=version, licence=licence.strip(), origin=origin,
                  texts=[directory / f for f in sorted(licence_files)], kind=kind)


def gather_tree(root: Path) -> list[Notice]:
    """Manifests and vendored crates, in walk order."""
    notices = []
    for directory, filenames in walk(root):
        if directory.name == "licenses" and MANIFEST in filenames:
            notices.extend(from_manifest(directory / MANIFEST, root))
        if "Cargo.toml" in filenames:
            licence_files = [f for f in filenames if LICENCE_FILE.match(f)]
            if licence_files:
                origin = directory.relative_to(root).as_posix()
                notices.append(_crate_notice(directory, licence_files, origin, f"{origin}/Cargo.toml",
                                             "vendored"))
    return notices


def cargo_home() -> Path:
    home = os.environ.get("CARGO_HOME")
    return Path(home) if home else Path.home() / ".cargo"


def gather_registry(root: Path, home: Path, covered=frozenset(), skipped=None) -> list[Notice]:
    """Every crates.io package in `Cargo.lock`, from the registry cache.

    `covered` holds `(name, version)` pairs a manifest already gives a notice
    for; those are left to the manifest. A procedural macro that ships no
    licence file is appended to `skipped`, when given, rather than refused.
    """
    lock = root / "Cargo.lock"
    if not lock.is_file():
        return []
    try:
        packages = tomllib.loads(lock.read_text(encoding="utf-8")).get("package", [])
    except tomllib.TOMLDecodeError as exc:
        raise NoticeError(f"Cargo.lock: cannot read ({exc})") from exc
    sources = home / "registry" / "src"
    notices = []
    for package in packages:
        if not str(package.get("source", "")).startswith("registry+"):
            continue
        name, version = package["name"], package["version"]
        if (name, version) in covered:
            continue
        found = sorted(sources.glob(f"*/{name}-{version}"))
        if not found:
            raise NoticeError(
                f"Cargo.lock: {name} {version} is not in the registry cache under {sources} "
                "-- build the workspace once so cargo fetches it")
        directory = found[0]
        licence_files = [f.name for f in directory.iterdir() if f.is_file() and LICENCE_FILE.match(f.name)]
        package_meta = _package(directory / "Cargo.toml", f"{name}-{version}/Cargo.toml")
        extra = package_meta.get("license-file")
        if isinstance(extra, str) and extra not in licence_files and (directory / extra).is_file():
            licence_files.append(extra)
        if not licence_files:
            # A procedural macro runs in the compiler; nothing of it is in any
            # binary the image carries, so its notice is not owed there.
            library = _lib_section(directory / "Cargo.toml", f"{name}-{version}/Cargo.toml")
            if library.get("proc-macro") is True:
                if skipped is not None:
                    skipped.append(f"{name} {version}")
                continue
            raise NoticeError(
                f"Cargo.lock: {name} {version} ships no licence file to carry; give it one in "
                "a `licenses/notices.yaml` of the crate that depends on it (same name and version)")
        notices.append(_crate_notice(directory, licence_files, "crates.io",
                                     f"{name}-{version}/Cargo.toml", "crates.io"))
    return notices


def _lib_section(cargo_toml: Path, where: str) -> dict:
    try:
        return tomllib.loads(cargo_toml.read_text(encoding="utf-8")).get("lib", {})
    except (tomllib.TOMLDecodeError, UnicodeDecodeError) as exc:
        raise NoticeError(f"{where}: cannot read Cargo.toml ({exc})") from exc


def key_for(notice: Notice) -> str:
    base = re.sub(r"[^a-z0-9.]+", "-", notice.component.casefold()).strip("-.") or "component"
    return f"{base}-{notice.version}" if notice.version else base


def gather(root: Path, home: Path, skipped=None) -> list[Notice]:
    """Every notice, sorted by component, each with a unique key.

    `skipped`, when given, collects the procedural macros left out."""
    tree = gather_tree(root)
    covered = frozenset((n.component, n.version) for n in tree if n.kind == "manifest")
    found = sorted(tree + gather_registry(root, home, covered, skipped), key=Notice.sort_key)
    kept: dict[str, Notice] = {}
    for notice in found:
        notice.key = key_for(notice)
        other = kept.get(notice.key)
        if other is None:
            kept[notice.key] = notice
            continue
        # The same component at the same version from two places. Two
        # manifests naming it is an authoring mistake; a vendored copy and the
        # crates.io package of one crate (cfg-if 1.0.5 is both) is one
        # component, and its notice is carried once -- the tree's copy, which
        # is the one the image's build used when both exist -- unless the two
        # disagree about its licence, which nobody should resolve by guessing.
        if notice.kind == "manifest" and other.kind == "manifest":
            raise NoticeError(
                f"{notice.component} {notice.version or ''} is named by two manifests, "
                f"{other.origin} and {notice.origin}; name it once")
        if _licence_key(notice.licence) != _licence_key(other.licence):
            raise NoticeError(
                f"{notice.component} {notice.version or ''} is under {other.licence!r} in "
                f"{other.origin} and {notice.licence!r} in {notice.origin}")
        if RANK[notice.kind] < RANK[other.kind]:
            kept[notice.key] = notice
    return sorted(kept.values(), key=Notice.sort_key)


#: Which source's notice is kept when one component comes from two.
RANK = {"manifest": 0, "vendored": 1, "crates.io": 2}


def _licence_key(licence: str) -> str:
    """A licence expression, compared: crates.io's old `MIT/Apache-2.0` is
    `MIT OR Apache-2.0`, and spacing does not matter."""
    return " ".join(licence.replace("/", " OR ").split()).casefold()


# ---------------------------------------------------------------------------
# The bundle
# ---------------------------------------------------------------------------

def _quoted(value: str) -> str:
    """A single-quoted YAML scalar, which `yamldoc` reads back exactly."""
    if any(ord(c) < 0x20 or c == "\x7f" for c in value):
        raise NoticeError(f"a control character in {value!r}")
    return "'" + value.replace("'", "''") + "'"


def index_text(notices: list[Notice]) -> str:
    lines = [
        "# Third-party notices for this system: the code it carries that others",
        "# wrote, and the texts their licences require to travel with it. Written by",
        "# scripts/gather-notices.py when the image is built (design-decisions 1433);",
        "# each file under `texts` is in this directory, under the entry's key.",
        "notices:",
    ]
    for n in notices:
        lines.append(f"  {n.key}:")
        lines.append(f"    component: {_quoted(n.component)}")
        if n.version:
            lines.append(f"    version: {_quoted(n.version)}")
        lines.append(f"    licence: {_quoted(n.licence)}")
        if n.attribution:
            lines.append(f"    attribution: {_quoted(n.attribution)}")
        lines.append(f"    from: {_quoted(n.origin)}")
        lines.append("    texts:")
        for text in n.texts:
            lines.append(f"      - {_quoted(f'{n.key}/{text.name}')}")
    return "\n".join(lines) + "\n"


def readable_bytes(notices: list[Notice]) -> bytes:
    """Every notice in one file, texts byte for byte."""
    rule = b"=" * 78 + b"\n"
    out = [b"Third-party notices for SlateOS\n", rule, b"\n"]
    for n in notices:
        heading = n.component + (f" {n.version}" if n.version else "")
        out += [rule, heading.encode("utf-8") + b"\n",
                f"Licence: {n.licence}\n".encode("utf-8")]
        if n.attribution:
            out.append(f"{n.attribution}\n".encode("utf-8"))
        out.append(rule)
        for text in n.texts:
            body = text.read_bytes()
            out += [f"\n--- {text.name} ---\n\n".encode("utf-8"), body]
            if not body.endswith(b"\n"):
                out.append(b"\n")
        out.append(b"\n")
    return b"".join(out)


def write_bundle(notices: list[Notice], out: Path) -> None:
    if out.exists() and any(out.iterdir()):
        raise NoticeError(f"{out} is not empty; the bundle is written whole into an empty directory")
    out.mkdir(parents=True, exist_ok=True)
    for n in notices:
        folder = out / n.key
        folder.mkdir()
        names = set()
        for text in n.texts:
            if text.name in names:
                raise NoticeError(f"{n.component}: two texts named {text.name}")
            names.add(text.name)
            (folder / text.name).write_bytes(text.read_bytes())
    (out / INDEX).write_bytes(index_text(notices).encode("utf-8"))
    (out / READABLE).write_bytes(readable_bytes(notices))


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------

def build_fixture(base: Path) -> tuple[Path, Path]:
    """A small tree with one notice of each kind, and a registry cache beside
    it: `(tree root, cargo home)`. The self-test gathers it, and
    `test-gather-notices.py` checks that its bundle is still byte for byte
    the golden one `gui/notices` reads in its own tests."""
    root = base / "tree"
    home = base / "cargo"
    # A crate with a ported component and its manifest.
    (root / "gui" / "codec" / "licenses").mkdir(parents=True)
    (root / "gui" / "codec" / "Cargo.toml").write_bytes(('[package]\nname = "codec"\nversion = "0.1.0"\n').encode("utf-8"))
    (root / "gui" / "codec" / "licenses" / "notices.yaml").write_bytes((
        "libjpeg-turbo:\n  version: 3.1.1\n  licence: IJG AND BSD-3-Clause AND Zlib\n"
        "  attribution: This software is based in part on the work of the Independent JPEG Group.\n"
        "  texts:\n    - LICENSE.md\n").encode("utf-8"))
    # Not CRLF, although that would be the obvious test of "byte for byte": the
    # golden copy of this bundle is committed, and git's `autocrlf=input` would
    # normalise it. A non-ASCII byte and a missing final newline test the same
    # thing and survive a commit.
    (root / "gui" / "codec" / "licenses" / "LICENSE.md").write_bytes(b"IJG text \xc2\xa9 1991\nno final newline")
    # A vendored crate, found by its licence files alone.
    (root / "vendor" / "cfg-if").mkdir(parents=True)
    (root / "vendor" / "cfg-if" / "Cargo.toml").write_bytes((
        '[package]\nname = "cfg-if"\nversion = "1.0.5"\nlicense = "MIT OR Apache-2.0"\n').encode("utf-8"))
    (root / "vendor" / "cfg-if" / "LICENSE-MIT").write_bytes(b"MIT text\n")
    # One of this project's own crates: no licence files, so no notice.
    (root / "gui" / "ours").mkdir(parents=True)
    (root / "gui" / "ours" / "Cargo.toml").write_bytes(('[package]\nname = "ours"\nversion = "0.1.0"\n').encode("utf-8"))
    # Build output is never walked, even when it holds a vendored copy.
    (root / "target" / "x").mkdir(parents=True)
    (root / "target" / "x" / "Cargo.toml").write_bytes(('[package]\nname = "junk"\nversion = "1.0.0"\n').encode("utf-8"))
    (root / "target" / "x" / "LICENSE").write_bytes(("junk\n").encode("utf-8"))
    # A registry package, and the lock file that names it.
    reg = home / "registry" / "src" / "index.crates.io-0" / "spin-0.9.8"
    reg.mkdir(parents=True)
    (reg / "Cargo.toml").write_bytes(('[package]\nname = "spin"\nversion = "0.9.8"\nlicense = "MIT"\n').encode("utf-8"))
    (reg / "LICENSE").write_bytes(b"spin MIT text\n")
    (root / "Cargo.lock").write_bytes((
        'version = 4\n\n[[package]]\nname = "codec"\nversion = "0.1.0"\n\n'
        '[[package]]\nname = "spin"\nversion = "0.9.8"\n'
        'source = "registry+https://github.com/rust-lang/crates.io-index"\n').encode("utf-8"))
    return root, home


def self_test() -> int:
    failures = []

    def expect(ok, what):
        if not ok:
            failures.append(what)

    def raises(fn, fragment, what):
        try:
            fn()
        except NoticeError as exc:
            expect(fragment in str(exc), f"{what}: wrong message {exc}")
        else:
            failures.append(f"{what}: no error")

    good = (
        "# comment\n"
        "libfoo:\n"
        "  version: 1.2.3  # trailing\n"
        "  licence: 'BSD-3-Clause'\n"
        "  attribution: \"It's \\\"quoted\\\"\"\n"
        "  texts:\n"
        "    - LICENSE-foo\n"
        "'bar: baz':\n"
        "  licence: MIT\n"
        "  texts:\n"
        "    - 'bar.txt'\n"
    )
    parsed = parse_manifest(good, "m")
    expect(parsed == {
        "libfoo": {"version": "1.2.3", "licence": "BSD-3-Clause",
                   "attribution": 'It\'s "quoted"', "texts": ["LICENSE-foo"]},
        "bar: baz": {"licence": "MIT", "texts": ["bar.txt"]},
    }, f"a well-formed manifest parsed wrong: {parsed}")
    raises(lambda: parse_manifest("x:\n  licence: MIT\n  texts:\n    - a\n  licenses: MIT\n", "m"),
           "unknown field", "a misspelt field")
    raises(lambda: parse_manifest("x:\n  texts:\n    - a\n", "m"), "no licence", "no licence")
    raises(lambda: parse_manifest("x:\n  licence: MIT\n", "m"), "no texts", "no texts")
    raises(lambda: parse_manifest("x:\n  licence: MIT\n  texts:\n    - a\nx:\n  licence: MIT\n", "m"),
           "listed twice", "a duplicate component")
    raises(lambda: parse_manifest("x:\n\tlicence: MIT\n", "m"), "tab", "a tab")

    with tempfile.TemporaryDirectory(prefix="gather_notices_") as tmp:
        root, home = build_fixture(Path(tmp))
        reg = home / "registry" / "src" / "index.crates.io-0" / "spin-0.9.8"
        notices = gather(root, home)
        expect([(n.key, n.origin) for n in notices] == [
            ("cfg-if-1.0.5", "vendor/cfg-if"),
            ("libjpeg-turbo-3.1.1", "gui/codec"),
            ("spin-0.9.8", "crates.io"),
        ], f"gathered the wrong notices: {[(n.key, n.origin) for n in notices]}")

        out = Path(tmp) / "out"
        write_bundle(notices, out)
        index = (out / INDEX).read_text(encoding="utf-8")
        expect("  libjpeg-turbo-3.1.1:\n    component: 'libjpeg-turbo'\n    version: '3.1.1'\n"
               "    licence: 'IJG AND BSD-3-Clause AND Zlib'\n"
               "    attribution: 'This software is based in part on the work of the Independent JPEG Group.'\n"
               "    from: 'gui/codec'\n    texts:\n      - 'libjpeg-turbo-3.1.1/LICENSE.md'\n" in index,
               f"the index entry is wrong:\n{index}")
        expect((out / "libjpeg-turbo-3.1.1" / "LICENSE.md").read_bytes() == b"IJG text \xc2\xa9 1991\nno final newline",
               "a text was not copied byte for byte")
        readable = (out / READABLE).read_bytes()
        expect(b"based in part on the work of the Independent JPEG Group" in readable
               and b"spin MIT text" in readable and b"MIT text" in readable,
               "NOTICES.txt lacks a notice")
        raises(lambda: write_bundle(notices, out), "not empty", "writing over a bundle")

        # Each way a notice cannot be gathered is an error, not a quiet gap.
        (root / "gui" / "codec" / "licenses" / "LICENSE.md").unlink()
        raises(lambda: gather(root, home), "which is not there", "a missing text")
        (root / "gui" / "codec" / "licenses" / "LICENSE.md").write_bytes(("back\n").encode("utf-8"))
        (reg / "LICENSE").unlink()
        raises(lambda: gather(root, home), "ships no licence file", "a registry crate with no licence")
        (reg / "LICENSE").write_bytes(("back\n").encode("utf-8"))
        raises(lambda: gather(root, Path(tmp) / "nowhere"), "not in the registry cache",
               "a registry package missing from the cache")

        # A vendored copy and the crates.io package of one crate are one notice.
        twin = home / "registry" / "src" / "index.crates.io-0" / "cfg-if-1.0.5"
        twin.mkdir(parents=True)
        (twin / "Cargo.toml").write_bytes(b'[package]\nname = "cfg-if"\nversion = "1.0.5"\nlicense = "MIT/Apache-2.0"\n')
        (twin / "LICENSE-MIT").write_bytes(b"MIT text\n")
        lock = (root / "Cargo.lock").read_bytes()
        (root / "Cargo.lock").write_bytes(lock + b'\n[[package]]\nname = "cfg-if"\nversion = "1.0.5"\n'
                                          b'source = "registry+https://github.com/rust-lang/crates.io-index"\n')
        twins = [n for n in gather(root, home) if n.component == "cfg-if"]
        expect([(n.key, n.kind) for n in twins] == [("cfg-if-1.0.5", "vendored")],
               f"a crate both vendored and from crates.io was not carried once: {twins}")
        (twin / "Cargo.toml").write_bytes(b'[package]\nname = "cfg-if"\nversion = "1.0.5"\nlicense = "GPL-3.0"\n')
        raises(lambda: gather(root, home), "is under", "two copies of one crate disagreeing about its licence")
        (twin / "Cargo.toml").write_bytes(b'[package]\nname = "cfg-if"\nversion = "1.0.5"\nlicense = "MIT/Apache-2.0"\n')

        # A procedural macro with no licence file is left out, and said to be.
        pm = home / "registry" / "src" / "index.crates.io-0" / "derive-it-1.0.0"
        pm.mkdir(parents=True)
        (pm / "Cargo.toml").write_bytes(b'[package]\nname = "derive-it"\nversion = "1.0.0"\n'
                                        b'license = "MIT"\n\n[lib]\nproc-macro = true\n')
        lock = (root / "Cargo.lock").read_bytes()
        (root / "Cargo.lock").write_bytes(lock + b'\n[[package]]\nname = "derive-it"\nversion = "1.0.0"\n'
                                          b'source = "registry+https://github.com/rust-lang/crates.io-index"\n')
        left_out: list[str] = []
        keys = [n.key for n in gather(root, home, left_out)]
        expect("derive-it-1.0.0" not in keys and left_out == ["derive-it 1.0.0"],
               f"a proc-macro with no licence was not left out cleanly: {keys} {left_out}")
        # The same crate as an ordinary library is refused...
        (pm / "Cargo.toml").write_bytes(b'[package]\nname = "derive-it"\nversion = "1.0.0"\nlicense = "MIT"\n')
        raises(lambda: gather(root, home), "ships no licence file", "a library with no licence file")
        # ...until a manifest gives it a notice by name and version.
        (root / "gui" / "codec" / "licenses" / "derive-it-LICENSE").write_bytes(b"MIT, by hand\n")
        manifest = root / "gui" / "codec" / "licenses" / "notices.yaml"
        manifest.write_bytes(manifest.read_bytes() + b"derive-it:\n  version: 1.0.0\n  licence: MIT\n"
                             b"  texts:\n    - derive-it-LICENSE\n")
        given = {n.key: n for n in gather(root, home)}
        expect(given.get("derive-it-1.0.0") is not None and given["derive-it-1.0.0"].kind == "manifest",
               f"a manifest did not give the crate its notice: {sorted(given)}")
        (root / "vendor" / "cfg-if" / "Cargo.toml").write_bytes((
            '[package]\nname = "cfg-if"\nversion = "1.0.5"\n').encode("utf-8"))
        raises(lambda: gather(root, home), "names no licence", "a vendored crate that names no licence")

    if failures:
        print("gather-notices self-test: FAILED")
        for f in failures:
            print("  " + f)
        return 1
    print("gather-notices self-test: all checks passed")
    return 0


def main(argv: list[str]) -> int:
    if argv == ["--self-test"]:
        return self_test()
    if not argv or argv[0] not in ("--check", "--out", "--list") or (argv[0] == "--out") != (len(argv) == 2) \
            or (argv[0] != "--out" and len(argv) != 1):
        print(__doc__.split("\n\n", 1)[0], file=sys.stderr)
        print("usage: gather-notices.py --check | --list | --out DIR | --self-test", file=sys.stderr)
        return 2
    skipped: list[str] = []
    try:
        notices = gather(ROOT, cargo_home(), skipped)
        if argv[0] == "--out":
            write_bundle(notices, Path(argv[1]))
    except NoticeError as exc:
        print(f"gather-notices: {exc}", file=sys.stderr)
        return 1
    if argv[0] == "--list":
        for n in notices:
            print(f"{n.key:40} {n.licence:40} {n.origin}")
    kinds = {"manifest": 0, "vendored": 0, "crates.io": 0}
    for n in notices:
        kinds[n.kind] += 1
    print(f"gather-notices: {len(notices)} notices "
          f"({kinds['manifest']} from manifests, {kinds['vendored']} vendored crates, "
          f"{kinds['crates.io']} from crates.io)")
    if skipped:
        print(f"  left out, as procedural macros with no licence file: {', '.join(skipped)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
