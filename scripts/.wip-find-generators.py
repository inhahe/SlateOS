"""Find production functions that manufacture the data their app displays.

Report-only. Looks for definitions whose name says they invent or preload
content, and reports the ones that are NOT `#[cfg(test)]` -- i.e. the ones a
shipping build can reach.
"""

import pathlib
import re

WORDS = (
    "simulate", "simulated", "fake", "dummy", "mock", "sample", "samples",
    "demo", "placeholder", "populate", "preload", "seed_", "stub",
    "synthes", "generate_sample", "example_",
)

FN = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+|async\s+)?fn\s+([a-z_0-9]+)", re.M)
CONST = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:static|const)\s+([A-Z_0-9]+)\s*:", re.M)

hits = {}
for path in sorted(pathlib.Path("apps").glob("*/src/**/*.rs")):
    text = path.read_text(encoding="utf-8", errors="replace")
    lines = text.splitlines()
    for pat in (FN, CONST):
        for m in pat.finditer(text):
            name = m.group(1)
            low = name.lower()
            if not any(w in low for w in WORDS):
                continue
            line_no = text.count("\n", 0, m.start())
            # Walk back over attributes and doc comments to find a cfg(test).
            gated = False
            i = line_no - 1
            while i >= 0:
                stripped = lines[i].strip()
                if stripped.startswith("///") or stripped.startswith("//"):
                    i -= 1
                    continue
                if stripped.startswith("#["):
                    if "cfg(test)" in stripped:
                        gated = True
                        break
                    i -= 1
                    continue
                break
            # A whole module can be gated too; cheap approximation.
            if not gated and "#[cfg(test)]\nmod tests" in text:
                tests_at = text.count("\n", 0, text.index("#[cfg(test)]\nmod tests"))
                if line_no > tests_at:
                    gated = True
            if not gated:
                hits.setdefault(str(path).replace("\\", "/"), []).append((line_no + 1, name))

total = sum(len(v) for v in hits.values())
print(f"{total} production definition(s) that name themselves as invented, in {len(hits)} file(s)\n")
for path, items in sorted(hits.items(), key=lambda kv: -len(kv[1])):
    print(f"{path}  ({len(items)})")
    for line_no, name in items[:6]:
        print(f"    {line_no}: {name}")
    if len(items) > 6:
        print(f"    ... and {len(items) - 6} more")
