"""How many fields are declared, initialised, and never read?

A measurement, not a gate. `check-fields-written-never-read.py` needs an
*assignment* to notice a field; a field that is only ever initialised in a
struct literal has none, which is how `explorer`'s `tree_expanded` survived.
This asks the cruder question -- does `.name` appear anywhere at all -- to
find out whether extending the real gate is viable or would drown in noise.
"""
import io
import os
import re
import sys

FIELD = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:\s*[A-Za-z_&<(\[]")
STRUCT = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?struct\s+([A-Z]\w*)")

roots = sys.argv[1:] or ["gui", "apps"]
files = []
for root in roots:
    for dirpath, dirnames, names in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in ("target", "__pycache__")]
        files.extend(os.path.join(dirpath, n) for n in names if n.endswith(".rs"))

# One text per crate, so a field read in another module of the same crate counts.
by_crate = {}
for path in files:
    parts = path.replace("\\", "/").split("/")
    crate = "/".join(parts[:2])
    by_crate.setdefault(crate, []).append(path)

suspects = []
for crate, paths in sorted(by_crate.items()):
    whole = []
    for p in paths:
        whole.append(io.open(p, encoding="utf-8", errors="replace").read())
    text = "\n".join(whole)
    for p in paths:
        src = io.open(p, encoding="utf-8", errors="replace").read()
        in_struct = False
        depth = 0
        for line in src.split("\n"):
            if STRUCT.match(line):
                in_struct = "{" in line
                depth = line.count("{") - line.count("}")
                continue
            if in_struct:
                depth += line.count("{") - line.count("}")
                if depth <= 0:
                    in_struct = False
                    continue
                m = FIELD.match(line)
                if not m:
                    continue
                name = m.group(1)
                # Read through a method call or a field access anywhere in the
                # crate, or bound by a destructuring pattern.
                if ("." + name) in text:
                    continue
                if re.search(r"\b" + re.escape(name) + r"\s*(,|\})", text.replace(name + ":", "")):
                    continue
                suspects.append((p, name))

print("fields declared and never read as `.name`: %d" % len(suspects))
# All of them, not a sample. This exists to be triaged from, and a list that
# stops at twenty-five sends the reader back to the script to find the rest --
# which is how the per-crate counts came out wrong the first time they were
# asked for.
for p, name in suspects:
    print("  %-46s %s" % (p.replace("\\", "/"), name))
