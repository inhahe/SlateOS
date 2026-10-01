#!/usr/bin/env python3
"""The catalogue of every program the workspace builds: `programs.md`.

Every binary target in the workspace, each with a line on what it does, in the
program's own words: the first sentence of its binary's module doc (`//!`),
else its crate's `description`. Taken from the source, so the catalogue cannot
drift from the programs -- regenerate it and it is current.

The operator asked for this answering B-Q21 (design-decisions §1053): a list of
every program with a short description, and "a rule somewhere that if you make
a program, record it somewhere so everybody knows it exists". This file is that
somewhere, and `--check` is the rule: it fails when a binary exists that
`programs.md` does not list, or lists one that no longer exists.

Beside each program: whether it is on the disk image that boots
(`scripts/rootfs-bin-manifest.txt`, or the fastpy `PROMOTED` map of
`scripts/create-ext4-rootfs.sh`), and the other names it answers to -- those
the manifest installs it under (`name = producer` lines), and those nothing
installs yet (`scripts/multicall-aliases-baseline.txt`).

    python scripts/program-catalogue.py            # rewrite programs.md
    python scripts/program-catalogue.py --check    # exit 1 if it is stale
    python scripts/program-catalogue.py --selftest
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
OUT = os.path.join(ROOT, "programs.md")
MANIFEST = os.path.join(HERE, "rootfs-bin-manifest.txt")
ROOTFS = os.path.join(HERE, "create-ext4-rootfs.sh")
ALIASES = os.path.join(HERE, "multicall-aliases-baseline.txt")

# Top-level directories, in the order the catalogue lists them, with the lane
# that owns each (roadmap.md, "The six lanes").
SECTIONS = [
    ("userspace", "Userland utilities and services", "B"),
    ("init", "The init system", "B"),
    ("apps", "Applications", "E"),
    ("gui", "Desktop, toolkit and graphics", "C, F"),
    ("services", "System services", "D"),
    ("posix", "POSIX layer", "D"),
    ("kernel", "Kernel", "A"),
]

SENTENCE_END = re.compile(r"(?<=[.!?])\s")


def first_sentence(text: str) -> str:
    """The first sentence of a doc paragraph, on one line."""
    text = " ".join(text.split())
    m = SENTENCE_END.search(text)
    if m:
        text = text[: m.start()]
    return text.strip()


def module_doc(path: str) -> str:
    """The first paragraph of a file's `//!` doc, or ''."""
    try:
        lines = open(path, encoding="utf-8").read().splitlines()
    except (OSError, UnicodeDecodeError):
        # A source file that is not UTF-8 does not compile either; its program
        # falls back to the crate's description rather than to a guessed text.
        return ""
    para: list[str] = []
    for line in lines:
        s = line.strip()
        if s.startswith("//!"):
            body = s[3:].strip()
            if not body:
                if para:
                    break
                continue
            if body.startswith("#") and para:
                break
            if body.startswith("```"):
                break
            para.append(body)
        elif s.startswith("#![") or not s:
            if para and not s:
                break
            continue
        else:
            break
    return first_sentence(" ".join(para))


def describe(doc: str, crate_desc: str) -> str:
    text = doc or crate_desc or ""
    text = text.replace("|", "\\|")
    # "Slate OS" in front of a crate's doc says nothing in a list of SlateOS's
    # own programs, and a doc line opening "`name` -- does X" says the name
    # twice in the table.
    text = re.sub(r"^Slate ?OS\s+", "", text)
    text = re.sub(r"^`[^`]+`\s*(?:--|—|-)\s*", "", text)
    text = re.sub(r"^\S+\s+(?:--|—|-)\s+", "", text)
    if text:
        text = text[0].upper() + text[1:]
    if len(text) > 160:
        text = text[:157].rstrip() + "..."
    return text or "*(no description in the source)*"


def on_image() -> set[str]:
    names: set[str] = set()
    try:
        for line in open(MANIFEST, encoding="utf-8"):
            line = line.split("#", 1)[0].strip()
            if not line:
                continue
            names.add(line.split("=", 1)[0].strip())
    except OSError:
        pass
    try:
        text = open(ROOTFS, encoding="utf-8").read()
        for m in re.finditer(r"\[fastpy-[^\]]+\]=(\S+)", text):
            names.add(m.group(1))
    except OSError:
        pass
    return names


def aliases() -> dict[str, list[str]]:
    """The names each crate answers to that nothing installs, by crate."""
    out: dict[str, list[str]] = {}
    try:
        for line in open(ALIASES, encoding="utf-8"):
            line = line.split("#", 1)[0].strip()
            if ":" not in line:
                continue
            crate, alias = line.split(":", 1)
            out.setdefault(crate.strip(), []).append(alias.strip())
    except OSError:
        pass
    return out


def installed_names() -> dict[str, list[str]]:
    """The second names the image installs, by the binary they name: the
    manifest's `name = producer` lines (design-decisions §1045)."""
    out: dict[str, list[str]] = {}
    try:
        for line in open(MANIFEST, encoding="utf-8"):
            line = line.split("#", 1)[0].strip()
            name, eq, producer = line.partition("=")
            if eq and name.strip() and producer.strip():
                out.setdefault(producer.strip(), []).append(name.strip())
    except OSError:
        pass
    return out


def kernel_embedded() -> set[str]:
    """The `services/` programs the kernel carries inside itself
    (`include_bytes!` of their build output), which are on every boot without
    being on the disk image."""
    names: set[str] = set()
    pat = re.compile(r'include_bytes!\("[./]*/services/([A-Za-z0-9_-]+)/target/')
    for base, _dirs, files in os.walk(os.path.join(ROOT, "kernel", "src")):
        for f in files:
            if not f.endswith(".rs"):
                continue
            try:
                text = open(os.path.join(base, f), encoding="utf-8").read()
            except (OSError, UnicodeDecodeError):
                continue
            names.update(pat.findall(text))
    return names


def standalone_services(known: set[str]) -> list[dict]:
    """The bare-metal `services/*` crates, which the workspace excludes (they
    build for `x86_64-unknown-none`, each on its own) and `cargo metadata`
    therefore does not list."""
    import tomllib

    out = []
    sdir = os.path.join(ROOT, "services")
    for name in sorted(os.listdir(sdir)):
        manifest = os.path.join(sdir, name, "Cargo.toml")
        main = os.path.join(sdir, name, "src", "main.rs")
        if not (os.path.isfile(manifest) and os.path.isfile(main)):
            continue
        with open(manifest, "rb") as f:
            pkg = tomllib.load(f).get("package", {})
        crate = pkg.get("name", name)
        if crate in known:
            continue
        out.append({
            "name": crate,
            "crate": crate,
            "dir": f"services/{name}",
            "desc": describe(module_doc(main), pkg.get("description") or ""),
        })
    return out


def targets() -> list[dict]:
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=ROOT, capture_output=True, encoding="utf-8", check=True,
    ).stdout
    meta = json.loads(out)
    root = meta["workspace_root"].replace("\\", "/").rstrip("/") + "/"
    progs = []
    for p in meta["packages"]:
        manifest = p["manifest_path"].replace("\\", "/")
        crate_dir = os.path.dirname(manifest)
        for t in p["targets"]:
            if "bin" not in t["kind"]:
                continue
            src = t["src_path"].replace("\\", "/")
            progs.append({
                "name": t["name"],
                "crate": p["name"],
                "dir": crate_dir[len(root):] if crate_dir.startswith(root) else crate_dir,
                "desc": describe(module_doc(src), p.get("description") or ""),
                # The crate's own program, whatever it is built as: the row
                # that carries the crate's other names.
                "main": src.endswith("/src/main.rs"),
            })
    progs += standalone_services({p["name"] for p in meta["packages"]})
    progs.sort(key=lambda x: (x["name"], x["dir"]))
    return progs


def render(progs: list[dict]) -> str:
    image = on_image()
    embedded = kernel_embedded()
    alias = aliases()
    installed = installed_names()

    def where(p: dict) -> str:
        if p["dir"].startswith("services/") and p["name"] in embedded:
            return "in the kernel"
        return "yes" if p["name"] in image else ""
    lines = [
        "# Every program SlateOS builds",
        "",
        "Generated by `scripts/program-catalogue.py` from the source -- do not edit by",
        "hand; regenerate. Each description is the first sentence of the program's",
        "own module doc, or its crate's `description`. **On image** says whether the",
        "program is on the disk image that boots; **Other names** lists the other",
        "names it answers to -- those the image installs it under, and, marked",
        "*(not installed)*, those nothing installs yet (design-decisions §1045).",
        "",
        "A new program is recorded here in the commit that creates it (§1053): run",
        "the script, commit `programs.md` with the program. `--check` fails when the",
        "two disagree.",
        "",
    ]
    total = len(progs)
    shipped = sum(1 for p in progs if where(p) == "yes")
    inside = sum(1 for p in progs if where(p) == "in the kernel")
    lines.append(
        f"**{total} programs; {shipped} on the image, {inside} carried inside the kernel.**"
    )
    lines.append("")
    placed = set()
    for top, title, lane in SECTIONS:
        group = [p for p in progs if p["dir"].split("/")[0] == top]
        if not group:
            continue
        lines += [f"## {title} (`{top}/`, lane {lane}) -- {len(group)}", ""]
        lines += ["| Program | What it does | On image | Crate | Other names |",
                  "|---|---|---|---|---|"]
        for p in group:
            names = [f"`{a}`" for a in installed.get(p["name"], [])]
            # The ledger names a crate's personalities by the crate, and a
            # coreutils binary's by the binary (multicall-aliases.py's
            # `survey`). A crate's go on the row of its own program, which
            # may be built under another name (`cgroup` as `lscgroup`).
            if p["crate"] == "coreutils":
                owned = alias.get(p["name"], [])
            elif p.get("main"):
                owned = alias.get(p["crate"], [])
            else:
                owned = []
            names += [f"`{a}` *(not installed)*" for a in owned]
            extra = ", ".join(names)
            crate = "" if p["crate"] == p["name"] else f"`{p['crate']}`"
            lines.append(
                f"| `{p['name']}` | {p['desc']} | {where(p)} | {crate} | {extra} |"
            )
            placed.add(id(p))
        lines.append("")
    rest = [p for p in progs if id(p) not in placed]
    if rest:
        lines += ["## Elsewhere", "", "| Program | What it does | Directory |", "|---|---|---|"]
        for p in rest:
            lines.append(f"| `{p['name']}` | {p['desc']} | `{p['dir']}` |")
        lines.append("")
    return "\n".join(lines)


def selftest() -> int:
    bad = 0

    def expect(label: str, got: object, want: object) -> None:
        nonlocal bad
        ok = got == want
        bad += 0 if ok else 1
        print(f"{'ok  ' if ok else 'FAIL'} {label}" + ("" if ok else f": got {got!r}, want {want!r}"))

    expect("first sentence stops at the first full stop",
           first_sentence("Lists files. In columns."), "Lists files.")
    expect("a version number is not a sentence end",
           first_sentence("A port of util-linux 2.39.3's lsblk: block devices."),
           "A port of util-linux 2.39.3's lsblk: block devices.")
    expect("the name-dash prefix is dropped",
           describe("`ls` -- list directory contents.", ""), "List directory contents.")
    expect("an em dash prefix too", describe("blockdev — call block device ioctls.", ""),
           "Call block device ioctls.")
    expect("the crate description stands in", describe("", "a thing"), "A thing")
    expect("nothing at all is said so", describe("", ""), "*(no description in the source)*")
    expect("a pipe cannot break the table", describe("a | b", ""), "A \\| b")
    expect("the Slate OS prefix goes", describe("Slate OS POSIX access control lists.", ""),
           "POSIX access control lists.")
    expect("...with a name and a dash after it",
           describe("SlateOS acpi - power management info", ""), "Power management info")
    expect("a hyphenated word is not a name-dash prefix",
           describe("Built-in commands.", ""), "Built-in commands.")
    print(f"selftest: {bad} failure(s)")
    return 1 if bad else 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Write or check programs.md.")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--selftest", "--self-test", action="store_true", dest="selftest")
    a = ap.parse_args()
    if a.selftest:
        return selftest()
    text = render(targets())
    if a.check:
        try:
            have = open(OUT, encoding="utf-8").read()
        except OSError:
            have = ""
        if have != text:
            print("programs.md is stale: a program was added, removed or re-described\n"
                  "without regenerating it. Run: python scripts/program-catalogue.py",
                  file=sys.stderr)
            return 1
        print("programs.md: current")
        return 0
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    print(f"wrote programs.md")
    return 0


if __name__ == "__main__":
    sys.exit(main())
