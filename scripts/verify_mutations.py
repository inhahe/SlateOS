"""Check a `mutate.py` table against the source it claims to break.

A sweep costs half an hour.  Every row whose `old_string` no longer appears in
the file -- because the production code was rewritten under it -- is a `[skip]`
at best and a silent hole at worst, and the sweep is the most expensive place to
find that out.  This reads the table without running it and reports:

* anchors that match zero times (dead) or more than once (ambiguous);
* expected test names that no longer exist in the source;
* duplicate row names, which make `only=` filtering ambiguous.

Usage:  python scripts/verify_mutations.py apps/<app>/mutate.py
"""

import re
import sys
from pathlib import Path


def main(argv):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if len(argv) != 2:
        print(__doc__)
        return 2
    mut = Path(argv[1]).resolve()
    src = mut.parent / "src" / "main.rs"
    text = src.read_text(encoding="utf-8", newline="")
    tests = set(re.findall(r"\bfn ([a-z_0-9]+)\(\)", text))

    ns = {"__name__": "not_main", "__file__": str(mut)}
    exec(compile(mut.read_text(encoding="utf-8"), str(mut), "exec"), ns)
    rows = ns["MUTATIONS"]

    bad = 0
    seen = set()
    for name, old, new, expect in rows:
        if name in seen:
            print(f"DUPLICATE ROW NAME: {name}")
            bad += 1
        seen.add(name)
        n = text.count(old.replace("\n", "\r\n") if "\r\n" in text else old)
        if n != 1:
            print(f"ANCHOR x{n}: {name}")
            bad += 1
        for t in expect:
            if t not in tests:
                print(f"NO SUCH TEST {t}: {name}")
                bad += 1
    print(f"{len(rows)} rows, {bad} problems")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
