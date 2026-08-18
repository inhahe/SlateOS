"""Count and list distinct clippy warning sites.

Raw `cargo clippy` output roughly doubles the real figure, because a file
compiled for both the lib and the test target has each of its sites reported
once per target. This deduplicates by `(file, line, column, lint)`, so the
number it prints is the number of places in the source that need an edit —
which is the number worth tracking a sweep against.

Usage::

    cargo clippy -p <crate> --all-targets --message-format=json > clippy.json
    python scripts/clippy-sites.py clippy.json                  # crate summary
    python scripts/clippy-sites.py clippy.json pathbar          # one file
    python scripts/clippy-sites.py clippy.json pathbar --sites  # every site

`--sites` prints `line:col lint | source` in line order, which is the form you
want when working a file down to zero.
"""

import collections
import json
import sys


def load(path):
    """Every distinct warning site in a clippy JSON stream.

    Yields `(file, line, column, lint, source_text)` once per site, in the
    order clippy first reported it.
    """
    seen = set()
    with open(path, encoding="utf-8") as fh:
        for raw in fh:
            raw = raw.strip()
            if not raw.startswith("{"):
                continue
            try:
                rec = json.loads(raw)
            except json.JSONDecodeError:
                # Cargo interleaves non-JSON progress lines into the stream.
                continue
            msg = rec.get("message")
            if not msg or msg.get("level") != "warning":
                continue
            code = (msg.get("code") or {}).get("code")
            if not code:
                continue
            primary = [s for s in msg.get("spans", []) if s.get("is_primary")]
            if not primary:
                continue
            span = primary[0]
            key = (span["file_name"], span["line_start"], span["column_start"], code)
            if key in seen:
                continue
            seen.add(key)
            text = (span.get("text") or [{}])[0].get("text", "").strip()
            yield (*key, text)


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    path = argv[1]
    rest = [a for a in argv[2:] if a != "--sites"]
    list_sites = "--sites" in argv[2:]
    only = rest[0] if rest else None

    sites = [s for s in load(path) if not only or only in s[0]]

    if list_sites:
        for name, line, col, code, text in sorted(sites, key=lambda s: (s[0], s[1])):
            print(f"{name}:{line}:{col} {code} | {text}")
        return 0

    by_lint = collections.Counter(s[3] for s in sites)
    by_file = collections.Counter(s[0] for s in sites)
    print(f"total distinct sites: {len(sites)}")
    print("\nby lint:")
    for lint, n in by_lint.most_common():
        print(f"  {n:5}  {lint}")
    print("\nby file:")
    for name, n in by_file.most_common(30):
        print(f"  {n:5}  {name}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
