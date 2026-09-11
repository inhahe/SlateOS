"""Find and classify every draw site that fills a surface role.

Scripts the *finding*, not the deciding. It reports what it thinks each site is
and why, so the classification can be read before anything is rewritten -- the
one thing a blind sweep would get wrong is a control's groove, which must stay
filled in both themes.
"""
import re
import pathlib
import collections
import sys

ROOT = pathlib.Path(r"E:\visual studio projects\os-lane-c")
ROLES = ("surface0", "surface1", "surface2", "mantle", "crust")

# Words near a site that say it is a control's groove rather than a container.
# A switch track outlined instead of filled reads as an empty box; a scrollbar
# trough vanishes entirely.
CHROME = re.compile(
    r"track|trough|groove|scrollbar|scroll_bar|thumb|slider|progress|gauge|"
    r"meter|switch|toggle|knob|handle|tick|grip",
    re.I,
)
# Words that say it is a floating panel rather than a card in a list.
PANEL = re.compile(r"menu|popup|dropdown|dialog|modal|tooltip|notification|toast|flyout", re.I)
SELECTED = re.compile(r"select|active|current|highlight|hover|focus|chosen|pressed", re.I)
SIDEBAR = re.compile(r"sidebar|side_bar|nav_|navigation|rail|gutter", re.I)

site_re = re.compile(r"(FillRect|fill_rect|fill_rounded|rounded_rect)\b")
role_re = re.compile(r"\b(?:pal|palette|p|self\.palette|theme)\s*\.\s*(" + "|".join(ROLES) + r")\b")


def classify(window: str) -> str:
    """What this site is, from the words around it. Order matters: a
    'selected menu item' is selection, not a panel."""
    if CHROME.search(window):
        return "ControlTrack"
    if SELECTED.search(window):
        return "Selected"
    if SIDEBAR.search(window):
        return "Sidebar"
    if PANEL.search(window):
        return "Panel"
    return "Card"


def main() -> int:
    counts = collections.Counter()
    by_file = collections.Counter()
    examples = collections.defaultdict(list)

    for path in sorted(ROOT.glob("gui/**/*.rs")) + sorted(ROOT.glob("apps/**/*.rs")):
        rel = path.relative_to(ROOT).as_posix()
        try:
            lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
        except OSError:
            continue
        in_test = False
        for i, line in enumerate(lines):
            if line.strip().startswith("#[cfg(test)]"):
                in_test = True
            stripped = line.strip()
            # Comments are not draw sites. Without this the survey reported a
            # doc comment in palette_check.rs as a control track, which is how
            # the false-positive rate got noticed at all.
            if stripped.startswith("//"):
                continue
            if not site_re.search(line):
                continue
            # Scope the role lookup to this site own literal, by tracking
            # brace depth from the opening line rather than taking a fixed
            # window: a window runs into the next site and attributes its
            # role and its naming words to this one.
            body = []
            depth = 0
            for probe in lines[i : i + 14]:
                body.append(probe)
                depth += probe.count("{") - probe.count("}")
                if depth <= 0 and len(body) > 1:
                    break
            window = chr(10).join(lines[max(0, i - 4) : i] + body)
            m = role_re.search(window)
            if not m:
                continue
            kind = classify(window)
            key = (kind, m.group(1))
            counts[key] += 1
            by_file[rel] += 1
            if len(examples[kind]) < 3:
                examples[kind].append(f"{rel}:{i+1}  ({m.group(1)})  {line.strip()[:64]}")
            if in_test:
                counts[("(in a test module)", m.group(1))] += 0

    total = sum(counts.values())
    print(f"{total} fill sites naming a surface role\n")
    print(f"{'what it looks like':<16} {'role filled':<10} {'count'}")
    for (kind, role), n in sorted(counts.items(), key=lambda kv: (-kv[1], kv[0])):
        print(f"{kind:<16} {role:<10} {n}")

    print("\nby kind:")
    per_kind = collections.Counter()
    for (kind, _role), n in counts.items():
        per_kind[kind] += n
    for kind, n in per_kind.most_common():
        pct = 100.0 * n / total if total else 0.0
        print(f"  {kind:<16} {n:>4}  ({pct:.0f}%)")

    print("\nthe 12 files with the most sites:")
    for rel, n in by_file.most_common(12):
        print(f"  {n:>4}  {rel}")

    print("\nsamples, to sanity-check the classification:")
    for kind in per_kind:
        print(f"  --- {kind}")
        for ex in examples[kind]:
            print(f"      {ex}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
