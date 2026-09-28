#!/usr/bin/env python3
"""Say, where every lane can see it at once, that you have started a task.

WHY THIS EXISTS
---------------
The operator, answering C-Q20 (`design-decisions.md` 1425): "whenever you take
it upon yourself to do a task, add it to some text file that other agents read
... or maybe send that you're starting the task to all other agents ... only
when the roadmap file doesn't clearly say that the program or feature belongs to
your own lane." Two lanes building the same thing is the most expensive way to
waste a day here, and `requests/` cannot prevent it: a request lives on a
branch, so the lane it would warn does not see it until it merges `main` --
by which time both have built it.

So a claim lives where the halts do (`check-lane-signals.py`): in the git
*common* directory, which every worktree shares, visible to every lane the
moment it is written, with no commit, no merge and no push.

WHAT A CLAIM IS
---------------
One file per claim, `<git-common-dir>/coordination/claims/<lane>-<slug>.txt`:
who, when, what, and optionally which paths. It says "I am doing this"; it is
not a lock -- nothing refuses an edit because of it -- and not a conversation.
A lane about to start work outside its obvious territory lists the claims (or
checks the paths it means to touch) first, and claims its own.

A claim older than `STALE_AFTER` is shown as stale rather than hidden: an
abandoned claim is still information (somebody started this), and a claim that
silently vanished could not be told from one never made. Release your claim
when the task is done or dropped.

USAGE
-----
    python scripts/lane-claims.py --claim "Porting the VP9 decoder" --paths gui/video
    python scripts/lane-claims.py --list
    python scripts/lane-claims.py --check gui/video/src/lib.rs apps/videoplayer
    python scripts/lane-claims.py --release porting-the-vp9-decoder
    python scripts/lane-claims.py --self-test
"""

from __future__ import annotations

import argparse
import datetime as _dt
import importlib.util
import re
import sys
from pathlib import Path

#: A claim this old is listed as stale: probably done or abandoned without
#: being released. Long enough for a task interrupted by a rate-limit window
#: (the longest seen ran about three days) to keep its claim.
STALE_AFTER = _dt.timedelta(days=7)

CLAIMS = "claims"


def _signals():
    """`check-lane-signals.py` as a module: the one opinion about where the
    shared directory is and which lane this session is."""
    here = Path(__file__).resolve().parent / "check-lane-signals.py"
    spec = importlib.util.spec_from_file_location("check_lane_signals", here)
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load check-lane-signals.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def slug(text: str) -> str:
    """A file-name-safe form of a claim's words: lower case, words joined by
    hyphens, at most sixty characters."""
    words = re.findall(r"[a-z0-9]+", text.lower())
    out = "-".join(words)[:60].strip("-")
    return out or "claim"


def _now() -> _dt.datetime:
    return _dt.datetime.now(_dt.timezone.utc).replace(microsecond=0)


def claims_dir(signal_dir: Path) -> Path:
    return signal_dir / CLAIMS


def make_claim(signal_dir: Path, lane: str, what: str, paths: list[str],
               now: _dt.datetime | None = None) -> Path:
    """Record that `lane` has started `what`, touching `paths`."""
    d = claims_dir(signal_dir)
    d.mkdir(parents=True, exist_ok=True)
    stamp = (now or _now()).strftime("%Y-%m-%dT%H:%M:%SZ")
    p = d / f"{lane.lower()}-{slug(what)}.txt"
    lines = [f"lane: {lane.upper()}", f"since: {stamp}", f"what: {what.strip()}"]
    lines += [f"path: {path}" for path in paths]
    p.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    return p


def read_claim(p: Path) -> dict:
    """A claim file's fields; `paths` a list. Unparseable lines are ignored:
    a hand-edited claim is read for what it can say."""
    out = {"file": p.name, "lane": "?", "since": None, "what": "", "paths": []}
    for line in p.read_text(encoding="utf-8", errors="replace").splitlines():
        key, sep, value = line.partition(":")
        if not sep:
            continue
        key, value = key.strip(), value.strip()
        if key == "path":
            out["paths"].append(value)
        elif key == "since":
            try:
                out["since"] = _dt.datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(
                    tzinfo=_dt.timezone.utc)
            except ValueError:
                out["since"] = None
        elif key in ("lane", "what"):
            out[key] = value
    return out


def all_claims(signal_dir: Path) -> list[dict]:
    d = claims_dir(signal_dir)
    if not d.is_dir():
        return []
    claims = [read_claim(p) for p in sorted(d.glob("*.txt"))]
    epoch = _dt.datetime.min.replace(tzinfo=_dt.timezone.utc)
    return sorted(claims, key=lambda c: c["since"] or epoch)


def release(signal_dir: Path, lane: str, name: str) -> list[Path]:
    """Remove `lane`'s claim called `name` (its slug, or the words it was
    made with), or every one of `lane`'s claims for `name == "all"`.
    Another lane's claims are never removed."""
    d = claims_dir(signal_dir)
    if not d.is_dir():
        return []
    prefix = f"{lane.lower()}-"
    removed = []
    for p in sorted(d.glob(prefix + "*.txt")):
        if name == "all" or p.stem == prefix + slug(name) or p.stem == prefix + name:
            p.unlink()
            removed.append(p)
    return removed


def _norm(path: str) -> str:
    return path.replace("\\", "/").strip("/")


def overlapping(claims: list[dict], paths: list[str]) -> list[dict]:
    """The claims whose paths contain, or are contained by, any of `paths`."""
    wanted = [_norm(p) for p in paths]
    hits = []
    for c in claims:
        for mine in (_norm(p) for p in c["paths"]):
            if any(w == mine or w.startswith(mine + "/") or mine.startswith(w + "/")
                   for w in wanted):
                hits.append(c)
                break
    return hits


def describe(c: dict, now: _dt.datetime | None = None) -> str:
    now = now or _now()
    if c["since"] is None:
        age = "since an unreadable time"
    else:
        days = (now - c["since"]).days
        age = "today" if days == 0 else f"{days} day(s) ago"
        if now - c["since"] > STALE_AFTER:
            age += " -- STALE: probably done or abandoned; ask the lane"
    paths = f" [{', '.join(c['paths'])}]" if c["paths"] else ""
    return f"lane {c['lane']}, {age}: {c['what']}{paths}  ({c['file']})"


def _self_test() -> int:
    import tempfile

    failures = []

    def check(ok, what):
        if not ok:
            failures.append(what)

    with tempfile.TemporaryDirectory(prefix="lane_claims_selftest_") as tmp:
        d = Path(tmp)
        t0 = _dt.datetime(2026, 9, 27, 12, 0, tzinfo=_dt.timezone.utc)
        check(all_claims(d) == [], "an empty directory has claims")
        a = make_claim(d, "c", "Port the VP9 decoder!", ["gui/video", "apps/videoplayer/"], t0)
        check(a.name == "c-port-the-vp9-decoder.txt", f"file name {a.name}")
        make_claim(d, "E", "Recycle bin drops", [], t0 + _dt.timedelta(hours=1))
        claims = all_claims(d)
        check([c["lane"] for c in claims] == ["C", "E"], "claims are not oldest first")
        check(claims[0]["paths"] == ["gui/video", "apps/videoplayer/"], "paths lost")
        check(claims[0]["what"] == "Port the VP9 decoder!", "words lost")

        hits = overlapping(claims, ["gui/video/src/lib.rs"])
        check([c["lane"] for c in hits] == ["C"], "a file inside a claimed folder is not found")
        check(overlapping(claims, ["gui"]) == [claims[0]], "a folder around a claim is not found")
        check(overlapping(claims, ["gui/videos"]) == [], "a sibling with a longer name matched")
        check(overlapping(claims, ["apps\\videoplayer\\src"]) == [claims[0]],
              "a backslashed path is not matched")

        fresh = describe(claims[0], t0 + _dt.timedelta(days=1))
        stale = describe(claims[0], t0 + _dt.timedelta(days=8))
        check("STALE" not in fresh and "1 day(s) ago" in fresh, fresh)
        check("STALE" in stale, stale)

        check(release(d, "E", "Port the VP9 decoder") == [], "a lane released another's claim")
        check(len(release(d, "c", "port-the-vp9-decoder")) == 1, "release by slug failed")
        check([c["lane"] for c in all_claims(d)] == ["E"], "the wrong claim went")
        make_claim(d, "E", "Another", [], t0)
        check(len(release(d, "E", "all")) == 2, "release all left some")
        check(all_claims(d) == [], "claims left after releasing all")

        bad = claims_dir(d) / "a-hand-edited.txt"
        bad.write_text("lane: A\nsince: yesterday\nnonsense line\nwhat: x\n",
                       encoding="utf-8", newline="\n")
        got = all_claims(d)
        check(len(got) == 1 and got[0]["since"] is None and got[0]["what"] == "x",
              "a hand-edited claim is not read for what it can say")
        check("unreadable time" in describe(got[0], t0), "an unreadable time is not said")
        check(slug("   ") == "claim", "an empty slug")

    if failures:
        print("lane-claims self-test FAILED:")
        for f in failures:
            print("  " + f)
        return 1
    print("lane-claims: self-test passed")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Claim a task where every lane can see it.")
    ap.add_argument("--self-test", "--selftest", dest="selftest", action="store_true",
                    help="run this script's own fixtures and exit")
    ap.add_argument("--claim", metavar="WHAT", help="record that your lane has started WHAT")
    ap.add_argument("--paths", nargs="*", default=[], metavar="PATH",
                    help="with --claim: the paths the task touches")
    ap.add_argument("--release", metavar="NAME",
                    help="remove your claim NAME (its slug or its words), or 'all'")
    ap.add_argument("--list", action="store_true", help="every claim, oldest first")
    ap.add_argument("--check", nargs="+", metavar="PATH",
                    help="the claims touching any of these paths (exit 1 if there are any)")
    args = ap.parse_args(argv)

    if args.selftest:
        return _self_test()
    signals = _signals()
    try:
        d = signals.signal_dir()
    except RuntimeError as e:
        print(f"lane-claims: {e}", file=sys.stderr)
        return 2
    lane = signals._detect_lane()

    if args.claim or args.release:
        if lane is None:
            print("lane-claims: cannot tell which lane this is; run it from your own "
                  "worktree (python scripts/which-lane.py says how)", file=sys.stderr)
            return 2
        if args.claim:
            p = make_claim(d, lane, args.claim, args.paths)
            print(f"claimed for lane {lane}: {p}")
        else:
            gone = release(d, lane, args.release)
            print(f"released {len(gone)} claim(s)" if gone else
                  f"lane {lane} has no claim called {args.release!r}")
        return 0

    claims = all_claims(d)
    if args.check:
        hits = overlapping(claims, args.check)
        for c in hits:
            print(describe(c))
        if not hits:
            print("lane-claims: nobody has claimed those paths")
        return 1 if hits else 0

    if not claims:
        print("lane-claims: no claims")
        return 0
    for c in claims:
        print(describe(c))
    return 0


if __name__ == "__main__":
    sys.exit(main())
