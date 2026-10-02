#!/usr/bin/env python3
"""Carry a lane's in-flight edits to the old single-file documents across the cutover.

    git fetch origin && git merge origin/main      # conflicts in known-issues.md etc.
    python scripts/docs-carry-forward.py           # resolves them into the new files
    git status && python scripts/check-docs.py     # review, then commit the merge

WHY. On 2026-10-02 the entry documents became one file per entry
(`docs-migrate.py`): `known-issues.md` and the rest became short signposts, and
fully done items left `roadmap.md` for `roadmap-done.md`. A lane branch that edited
the old files since it last merged `main` gets conflicts when it merges the cutover:
its edits are to text that no longer exists in that shape. Resolving them by hand
means finding each edited entry in a 180,000-line file and its new home among 2,600
files. This does it mechanically.

HOW. The conversion is deterministic, so it can be replayed: this script converts
the merge base's copies and this branch's copies of the old files exactly as the
cutover converted the trunk's, and the difference between those two conversions is
precisely this lane's edits, expressed per entry. Each changed entry is then merged
three ways into the file the trunk now has (`git merge-file`):

* an entry the lane added is created;
* an entry the lane changed is merged with whatever the trunk did to it since;
* an entry whose status the lane changed moves between `known-issues/` and
  `known-issues-resolved/`, wherever the merged text says it belongs;
* an entry the lane deleted is deleted, unless the trunk changed it meanwhile.

`roadmap.md` / `roadmap-done.md` are merged the same way: both sides' copies are
archived with the cutover's rule and the results merged three ways.

Nothing is resolved silently. A genuine conflict -- the trunk and the lane changed
the same lines of one entry -- is left with standard conflict markers in that
entry's file and listed at the end; resolve it as any merge conflict. The old
files are restored to the trunk's signposts and staged, with everything else this
script wrote. It refuses to run outside a merge, and refuses if MERGE_HEAD has no
signposts (nothing to carry across).

    --dry-run   report what would be written, write nothing
    --stray     no merge in progress: an old file holds entries again after the
                merge (rule S1); move them into their own files
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import doc_entries as D  # noqa: E402
import docs_layout as L  # noqa: E402

ISSUE_DIRS = (D.ISSUES_OPEN_DIR, D.ISSUES_CLOSED_DIR)
GROUPS = (
    (D.ISSUES_MONO, D.ISSUES_CLOSED_MONO),
    (D.DECISIONS_MONO,),
    (D.QUESTIONS_MONO,),
    (D.DEFERRED_MONO,),
)


def git(root: Path, *args: str, check: bool = True, binary: bool = False) -> subprocess.CompletedProcess:
    r = subprocess.run(["git", "-C", str(root), *args], capture_output=True, text=not binary)
    if check and r.returncode != 0:
        raise SystemExit(f"git {' '.join(args)} failed: {(r.stderr or b'')[:400]}")
    return r


def show(root: Path, rev: str, path: str) -> str | None:
    r = subprocess.run(["git", "-C", str(root), "show", f"{rev}:{path}"], capture_output=True)
    if r.returncode != 0:
        return None
    return r.stdout.decode("utf-8")


def merge3(ours: str, base: str, theirs: str, label: str) -> tuple[str, bool]:
    """`git merge-file` on three texts: (result, had conflicts)."""
    with tempfile.TemporaryDirectory() as t:
        po, pb, pt = (Path(t) / n for n in ("ours", "base", "theirs"))
        for p, text in ((po, ours), (pb, base), (pt, theirs)):
            p.write_bytes(text.encode("utf-8"))
        r = subprocess.run(["git", "merge-file", "-p", "-L", f"{label} (this lane)", "-L", "merge base",
                            "-L", f"{label} (main)", str(po), str(pb), str(pt)], capture_output=True)
        if r.returncode < 0 or r.returncode > 127:
            raise SystemExit(f"git merge-file failed on {label}")
        return r.stdout.decode("utf-8"), r.returncode > 0


def by_stem(files: dict[str, str]) -> dict[str, tuple[str, str]]:
    """stem-or-path key -> (path, text), with issues keyed by stem across both directories,
    so an entry that moved between them is still the same entry."""
    out = {}
    for path, text in files.items():
        top = path.split("/", 1)[0]
        key = "issue:" + Path(path).stem.lower() if top in ISSUE_DIRS and not path.endswith("README.md") else path
        out[key] = (path, text)
    return out


def issue_home(path_hint: str, text: str) -> str:
    """Where an issue's merged text belongs: open or resolved, by its own status."""
    lines = text.splitlines(keepends=True)
    heads = D.fence_aware_headings(lines) if lines else []
    if not heads:
        return path_hint
    status = D.issue_status(heads[0][2], lines, archived=False)
    folder = D.ISSUES_CLOSED_DIR if status == "closed" else D.ISSUES_OPEN_DIR
    return f"{folder}/{Path(path_hint).name}"


class Plan:
    def __init__(self) -> None:
        self.write: dict[str, str] = {}
        self.delete: set[str] = set()
        self.conflicts: list[str] = []
        self.notes: list[str] = []
        self.counts: dict[str, int] = {}

    def bump(self, k: str) -> None:
        self.counts[k] = self.counts.get(k, 0) + 1


def current(root: Path, path: str, plan: Plan) -> str | None:
    if path in plan.write:
        return plan.write[path]
    if path in plan.delete:
        return None
    p = root / path
    return p.read_text(encoding="utf-8") if p.is_file() else None


def carry_group(root: Path, names: tuple[str, ...], base: dict[str, str], ours: dict[str, str], plan: Plan) -> None:
    """Apply this lane's per-entry edits for one document group (the issue pair, or one doc)."""
    bconv = L.convert({n: t for n, t in base.items() if n in names})
    oconv = L.convert({n: t for n, t in ours.items() if n in names})
    b, o = by_stem(bconv.files), by_stem(oconv.files)
    for key in sorted(set(b) | set(o)):
        bp, bt = b.get(key, (None, None))
        op, ot = o.get(key, (None, None))
        if bt == ot:
            continue  # this lane did not touch it
        # Where main has it now (an issue may have moved between the two folders).
        if key.startswith("issue:"):
            stem = key[len("issue:"):]
            tp = next((f"{d}/{p.name}" for d in ISSUE_DIRS for p in [Path(op or bp)]
                       if current(root, f"{d}/{p.name}", plan) is not None), None)
            if tp is None:  # case-insensitive filesystems: look the stem up by listing
                tp = next((f"{d}/{f.name}" for d in ISSUE_DIRS if (root / d).is_dir()
                           for f in (root / d).iterdir() if f.stem.lower() == stem), None)
        else:
            tp = op or bp
        tt = current(root, tp, plan) if tp else None
        label = (op or bp)
        if ot is None:  # this lane removed (or archived away) the entry
            if tt is None:
                continue
            if tt == bt:
                plan.delete.add(tp)
                plan.bump("deleted")
            else:
                plan.conflicts.append(f"{tp}: this lane removed the entry, and main changed it since; kept main's "
                                      "copy -- delete it by hand if the removal still stands")
            continue
        if tt is None:
            if bt is not None and tp is not None:
                plan.notes.append(f"{label}: main no longer has this entry; this lane's edited copy is restored")
            dest = issue_home(op, ot) if key.startswith("issue:") else op
            plan.write[dest] = ot
            plan.bump("added" if bt is None else "restored")
            continue
        merged, conflicted = merge3(ot, bt or "", tt, label)
        dest = issue_home(tp, merged) if key.startswith("issue:") and not conflicted else tp
        if dest != tp:
            plan.delete.add(tp)
            plan.bump("moved")
        plan.write[dest] = merged
        plan.bump("merged")
        if conflicted:
            plan.conflicts.append(f"{dest}: this lane and main both changed this entry; conflict markers left in it")


def carry_roadmap(root: Path, base_rev: str, ours_rev: str, theirs_rev: str, plan: Plan) -> None:
    bt, ot = show(root, base_rev, "roadmap.md"), show(root, ours_rev, "roadmap.md")
    tr, td = show(root, theirs_rev, "roadmap.md"), show(root, theirs_rev, "roadmap-done.md")
    if bt is None or ot is None or tr is None or td is None or bt == ot:
        if tr is not None:
            plan.write["roadmap.md"] = tr if bt == ot else plan.write.get("roadmap.md", tr)
        return
    b_road, b_done, _ = L.archive_roadmap(bt)
    o_road, o_done, _ = L.archive_roadmap(ot)
    for name, o, b, t in (("roadmap.md", o_road, b_road, tr), ("roadmap-done.md", o_done, b_done, td)):
        merged, conflicted = merge3(o, b, t, name)
        plan.write[name] = merged
        if conflicted:
            plan.conflicts.append(f"{name}: this lane and main both changed the same lines; conflict markers left")
    plan.bump("roadmap merged")


def apply(root: Path, plan: Plan, signposts: dict[str, str]) -> None:
    for path in sorted(plan.delete - set(plan.write)):
        if (root / path).exists():
            git(root, "rm", "-q", "--cached", "--ignore-unmatch", "--", path)
            (root / path).unlink()
    for path, text in plan.write.items():
        p = root / path
        p.parent.mkdir(parents=True, exist_ok=True)
        with open(p, "w", encoding="utf-8", newline="") as fh:
            fh.write(text)
    for name, text in signposts.items():
        with open(root / name, "w", encoding="utf-8", newline="") as fh:
            fh.write(text)
    clean = [p for p in [*plan.write, *signposts] if not any(c.startswith(p + ":") for c in plan.conflicts)]
    if clean:
        git(root, "add", "--", *clean)


def report(plan: Plan, dry: bool) -> int:
    verb = "would write" if dry else "wrote"
    print(f"docs-carry-forward: {verb} {len(plan.write)} file(s), "
          f"{'would delete' if dry else 'deleted'} {len(plan.delete - set(plan.write))}; "
          + ", ".join(f"{k}={v}" for k, v in sorted(plan.counts.items())))
    for n in plan.notes:
        print("  note:", n)
    if plan.conflicts:
        print(f"\n{len(plan.conflicts)} conflict(s) need you -- resolve the markers, then `git add` each file:")
        for c in plan.conflicts:
            print("  -", c)
        return 1
    if not dry:
        print("\nall carried across and staged. Next: `python scripts/check-docs.py`, review `git status`, "
              "commit the merge.")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--stray", action="store_true")
    ap.add_argument("--lane", default="", help="names the grandfather file (default: this worktree's lane)")
    args = ap.parse_args(argv)
    root = D.repo_root()
    gitdir = D.git_dir(root)
    plan = Plan()

    if args.stray:
        # An old file holds entries again after the cutover: everything in it is new to
        # the per-entry tree (base = nothing), merged against what is there now.
        texts = {n: (root / n).read_text(encoding="utf-8") for n in D.MONOLITHS
                 if (root / n).is_file() and not D.is_signpost(root / n)}
        if not texts:
            print("no old-format document holds entries; nothing to do")
            return 0
        for names in GROUPS:
            if any(n in texts for n in names):
                carry_group(root, names, {}, {n: texts[n] for n in names if n in texts}, plan)
        for f in [f for f in list(plan.write) if f.endswith("README.md")]:
            del plan.write[f]  # never replace a README from a stray copy
        signposts = {n: L.signpost(n, "2026-10-02") for n in texts}
        if not args.dry_run:
            apply(root, plan, signposts)
        return report(plan, args.dry_run)

    if not (gitdir / "MERGE_HEAD").exists():
        print("docs-carry-forward: no merge in progress. Run it after `git merge origin/main` stops on "
              "conflicts in the old documents (or use --stray).", file=sys.stderr)
        return 2
    theirs = git(root, "rev-parse", "MERGE_HEAD").stdout.strip()
    ours = git(root, "rev-parse", "HEAD").stdout.strip()
    base = git(root, "merge-base", ours, theirs).stdout.strip()
    their_signposts = {n: t for n in D.MONOLITHS if (t := show(root, theirs, n)) is not None
                       and t.startswith(D.SIGNPOST_MARK)}
    if not their_signposts:
        print("docs-carry-forward: the branch being merged has no signposts, so it does not contain the "
              "cutover; nothing to carry", file=sys.stderr)
        return 2
    base_texts = {n: t for n in D.MONOLITHS if (t := show(root, base, n)) is not None
                  and not t.startswith(D.SIGNPOST_MARK)}
    our_texts = {n: t for n in D.MONOLITHS if (t := show(root, ours, n)) is not None
                 and not t.startswith(D.SIGNPOST_MARK)}
    for names in GROUPS:
        if any(base_texts.get(n) != our_texts.get(n) for n in names):
            carry_group(root, names, base_texts, our_texts, plan)
    carry_roadmap(root, base, ours, theirs, plan)
    # A README generated from an old header (the decisions' band table, the
    # queue's rules) is merged like any entry above: a lane that opened a new band
    # on its branch (lane A opened §1500 on 2026-10-01) must not lose it, or every
    # decision it numbered there reads as outside any band.
    lane = (args.lane or running_lane(root) or "unknown").lower()
    grandfather(root, plan, base, ours, lane)
    if not args.dry_run:
        apply(root, plan, their_signposts)
        resolve_leftovers(root, plan)
    return report(plan, args.dry_run)


# Scripts the cutover deleted. A lane that changed one meanwhile gets a
# modify/delete conflict; the replacement is check-docs.py, so the conflict is
# resolved as deleted and the lane is told its change needs porting.
RETIRED = (
    "scripts/check-known-issues-index.py", "scripts/check-open-questions.py",
    "scripts/check-design-decisions-bands.py", "scripts/test-check-design-decisions-bands.py",
    "scripts/design-decisions-baseline.json", "scripts/ki_dupes.py", "scripts/test-ki-dupes.py",
    "scripts/ki_archive.py", "scripts/backfill-lane-fields.py", "scripts/test-backfill-lane-fields.py",
)


def running_lane(root: Path) -> str | None:
    try:
        import srcload
        wl = srcload.load(str(Path(__file__).resolve().parent / "which-lane.py"), "which_lane")
        letter, _how = wl.lane_of_worktree(root)
        return letter if letter and letter != "!" else None
    except Exception:  # noqa: BLE001 - only names the grandfather file
        return None


def grandfather(root: Path, plan: Plan, base: str, ours: str, lane: str) -> None:
    """Record what this lane wrote *before* the cutover as inherited, the way the
    cutover's own baseline records the trunk's: an entry carried across was written
    under the old rules (status in the heading, no Trigger line), and it is not new
    work for check-docs.py to refuse. Kept in a per-lane file,
    `scripts/docs-baseline-carried-<lane>.json`, so lanes never edit one file."""
    import srcload
    cd = srcload.load(str(Path(__file__).resolve().parent / "check-docs.py"), "check_docs")
    add: dict[str, set] = {k: set() for k in ("issue_without_status", "lowercase_marker", "decision_files",
                                              "deferred_without_trigger", "todo_done_paragraphs")}
    for path, text in plan.write.items():
        top = path.split("/", 1)[0]
        if path.endswith("README.md") or "/" not in path:
            continue
        lines = text.splitlines(keepends=True)
        if top in ISSUE_DIRS and lines:
            title = lines[0].lstrip("#").strip()
            if not any(ln.lstrip().startswith("**Status") for ln in lines[1:8]):
                add["issue_without_status"].add(cd._entry_key(path))  # noqa: SLF001
            if any(t != t.upper() for t in cd.status_marker_tokens(title, lines)):
                add["lowercase_marker"].add(cd._entry_key(path))  # noqa: SLF001
        elif top == D.DECISIONS_DIR:
            add["decision_files"].add(path)
        elif top == D.DEFERRED_DIR and not cd._TRIGGER.search(text):  # noqa: SLF001
            add["deferred_without_trigger"].add(path)
    b_todo, o_todo = show(root, base, "todo.txt"), show(root, ours, "todo.txt")
    if b_todo is not None and o_todo is not None and b_todo != o_todo:
        before = {cd._hash(t) for _l, _o, t in cd.todo_done_paragraphs(b_todo)}  # noqa: SLF001
        add["todo_done_paragraphs"] = {cd._hash(t) for _l, _o, t in cd.todo_done_paragraphs(o_todo)} - before  # noqa: SLF001
    if not any(add.values()):
        return
    rel = f"scripts/docs-baseline-carried-{lane}.json"
    existing = {}
    if (root / rel).is_file():
        existing = json.loads((root / rel).read_text(encoding="utf-8"))
    merged = {k: sorted(set(existing.get(k, [])) | v) for k, v in add.items()}
    plan.write[rel] = json.dumps(merged, indent=1, ensure_ascii=False) + "\n"
    plan.notes.append(f"{rel}: {sum(len(v) for v in add.values())} entr(ies) this lane wrote before the cutover "
                      "are grandfathered, as the trunk's were")


def resolve_leftovers(root: Path, plan: Plan) -> None:
    """Conflicts the cutover causes outside the documents: retired scripts this lane
    changed (resolved as deleted, with a note), and the generated script index."""
    unmerged = git(root, "diff", "--name-only", "--diff-filter=U", check=False).stdout.split()
    for path in unmerged:
        if path in RETIRED:
            git(root, "rm", "-q", "--", path, check=False)
            plan.notes.append(f"{path}: this lane changed it, and the cutover retired it (check-docs.py replaces "
                              "it). Resolved as deleted -- port the change to check-docs.py if it still matters.")
    if "scripts/INDEX.md" in unmerged and (root / "scripts/gen-script-index.py").is_file():
        r = subprocess.run([sys.executable, "scripts/gen-script-index.py"], cwd=root, capture_output=True, text=True)
        if r.returncode == 0:
            git(root, "add", "--", "scripts/INDEX.md")
            plan.notes.append("scripts/INDEX.md: regenerated (it is generated; both sides' versions were stale)")


if __name__ == "__main__":
    sys.exit(main())
