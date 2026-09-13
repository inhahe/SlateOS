"""One-shot: which `#![allow(dead_code)]` lines are masking nothing?

For each file carrying an inner allow, comment it out, type-check the crate,
count the dead-code warnings that appear, and restore. A file where nothing
appears is a file whose allow can simply go.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-gnu"
INNER = re.compile(r"^\s*#!\[allow\([^)]*dead_code[^)]*\)\]\s*$", re.MULTILINE)
DEAD = re.compile(r"never (?:used|read|constructed)")


def crate_of(path: pathlib.Path) -> str | None:
    """The package name owning `path`, from the nearest Cargo.toml."""
    for parent in path.parents:
        manifest = parent / "Cargo.toml"
        if manifest.is_file():
            for line in manifest.read_text(encoding="utf-8").splitlines():
                if line.startswith("name"):
                    return line.split("=", 1)[1].strip().strip('"')
    return None


def check(crate: str) -> int:
    out = subprocess.run(
        ["cargo", "check", "-p", crate, "--all-targets", "--target", TARGET],
        cwd=ROOT, capture_output=True, text=True, check=False,
    )
    return len(DEAD.findall(out.stdout + out.stderr))


files = sorted(
    p for top in ("gui", "apps")
    for p in (ROOT / top).rglob("*.rs")
    if "target" not in p.parts and INNER.search(p.read_text(encoding="utf-8", errors="replace"))
)
print(f"{len(files)} file(s) with an inner allow(dead_code)", flush=True)

clean, masking, skipped = [], [], []
for path in files:
    crate = crate_of(path)
    rel = path.relative_to(ROOT).as_posix()
    if crate is None:
        skipped.append((rel, "no crate"))
        continue
    original = path.read_text(encoding="utf-8")
    disabled = INNER.sub(lambda m: "// SWEEP-DISABLED" + m.group(0).strip(), original, count=1)
    try:
        path.write_text(disabled, encoding="utf-8", newline="\n")
        n = check(crate)
    finally:
        path.write_text(original, encoding="utf-8", newline="\n")
    (clean if n == 0 else masking).append((rel, crate, n))
    print(f"  {'clean ' if n == 0 else f'{n:>4} '} {rel}", flush=True)

print()
print(f"masking nothing ({len(clean)}): the allow can go")
for rel, crate, _ in clean:
    print(f"  {rel}")
print()
print(f"masking something ({len(masking)}):")
for rel, crate, n in sorted(masking, key=lambda x: -x[2]):
    print(f"  {n:>4}  {rel}")
if skipped:
    print(f"{chr(10)}skipped: {skipped}")
