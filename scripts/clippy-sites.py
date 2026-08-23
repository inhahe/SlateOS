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

`--by-context` splits the count into production and test code::

    python scripts/clippy-sites.py clippy.json --by-context
    python scripts/clippy-sites.py clippy.json --by-context --lint clippy::panic

That split is the one that decides how much of a backlog is worth sweeping.
A `.unwrap()` in a boot-time self-test is a test asserting on its own fixture,
which is what `CLAUDE.md` explicitly permits; the same call in a syscall path
is a denial of service if an attacker can shape the input. Counting them
together produces a number that means nothing.

Getting the split right needs the *outermost* enclosing function, not the
nearest one above the line -- see `scripts/rust_scopes.py` for why the obvious
regex misclassifies thousands of sites. `--lint` lists the production sites of
one lint, which is the form you want when adjudicating them one by one.
"""

import collections
import json
import sys
from pathlib import Path

import rust_scopes

REPO_ROOT = Path(__file__).resolve().parent.parent


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


_SCOPE_CACHE: dict[str, list[list[rust_scopes.Scope]]] = {}


def context_of(file_name, line):
    """Bucket one site by the *outermost* item enclosing it."""
    stacks = _SCOPE_CACHE.get(file_name)
    if stacks is None:
        try:
            text = (REPO_ROOT / file_name).read_text(encoding="utf-8", errors="replace")
            stacks = rust_scopes.scope_stack_per_line(text.splitlines())
        except OSError:
            stacks = []
        _SCOPE_CACHE[file_name] = stacks
    idx = line - 1
    stack = stacks[idx] if 0 <= idx < len(stacks) else []
    return rust_scopes.classify(stack, file_name), (stack[0].name if stack else "")


def report_by_context(sites, want_lint):
    """Print the production/test split, or list one lint's production sites."""
    tagged = [(s, *context_of(s[0], s[1])) for s in sites]

    if want_lint:
        rows = [t for t in tagged if t[0][3] == want_lint and t[1] == "production"]
        for (name, line, col, code, text), _, fn in sorted(rows, key=lambda t: t[0]):
            print(f"{name}:{line}:{col} [{fn}] | {text}")
        print(f"\n{len(rows)} production site(s) of {want_lint}")
        return 0

    by_ctx = collections.Counter(t[1] for t in tagged)
    print(f"total distinct sites: {len(tagged)}")
    print("\nby context:")
    for ctx, n in by_ctx.most_common():
        pct = 100.0 * n / len(tagged) if tagged else 0.0
        print(f"  {n:6}  {pct:5.1f}%  {ctx}")

    for ctx, _ in by_ctx.most_common():
        lints = collections.Counter(t[0][3] for t in tagged if t[1] == ctx)
        print(f"\n{ctx} -- by lint:")
        for lint, n in lints.most_common(12):
            print(f"  {n:6}  {lint}")
    return 0


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    path = argv[1]
    flags = argv[2:]
    by_context = "--by-context" in flags
    list_sites = "--sites" in flags
    want_lint = None
    if "--lint" in flags:
        i = flags.index("--lint")
        if i + 1 >= len(flags):
            print("--lint needs a lint name, e.g. --lint clippy::panic")
            return 2
        want_lint = flags[i + 1]
        flags = flags[:i] + flags[i + 2 :]
    rest = [a for a in flags if not a.startswith("--")]
    only = rest[0] if rest else None

    sites = [s for s in load(path) if not only or only in s[0]]

    if by_context:
        return report_by_context(sites, want_lint)

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
