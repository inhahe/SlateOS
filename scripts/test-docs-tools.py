#!/usr/bin/env python3
"""Tests for the shared-document tooling: doc_entries.py, docs_layout.py,
docsearch.py, docs-migrate.py and docs-carry-forward.py.

Run: `python scripts/test-docs-tools.py` (0 = pass, 1 = fail). No pytest, like the
other suites here, so it runs from a bare checkout and from boot-test.sh's
`check_python_suites`. Everything happens in temporary directories; nothing in
this repository is read or written except the scripts under test.

What the cases are aimed at, in order of how much it would cost to get wrong:

* **The carry-forward.** Six lanes will merge the cutover with edits in flight.
  A lane's edit that does not arrive in the new files is lost silently -- the
  merge "succeeds". So the end-to-end cases build a real repository with a real
  merge and check each kind of edit arrives: changed, added, status-moved,
  deleted, and both sides changing one entry (which must stop, not pick a side).
* **The conversion.** Byte-identical entries, placement by status, the archived
  entry that says OPEN, shared ids, and that `verify()` catches a conversion that
  lost a line -- a verifier tested only on good input passes when it checks nothing.
* **The parser's edges**, each one found in the real documents: fences holding
  `#` lines, `### [E]` entries, backtick ids, `-- FIXED` tails, `B-` meaning "bug".
"""

from __future__ import annotations

import contextlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import doc_entries as D  # noqa: E402
import docs_layout as L  # noqa: E402

failures: list[str] = []
CASES = []


def case(fn):
    CASES.append(fn)
    return fn


def check(label: str, got, want) -> None:
    ok = got == want
    print(f"{'PASS' if ok else 'FAIL'}  {label}")
    if not ok:
        print(f"        got : {got!r}"[:400])
        print(f"        want: {want!r}"[:400])
        failures.append(label)


# --- doc_entries ---------------------------------------------------------------------------

@case
def parser_edges() -> None:
    fenced = "# KI\n\n## TD-B-ONE (lane B) — OPEN\nbody\n```\n## not a heading\n# bash: comment\n```\nmore\n"
    _pre, ents, _d = D.parse_issues_monolith(fenced, "known-issues.md", archived=False)
    check("a heading inside a code fence does not start an entry", [e.key for e in ents], ["TD-B-ONE"])
    check("...and stays in its entry's text", "## not a heading" in ents[0].text, True)

    tagged = "# KI\n\n## TD-A-X (lane A) — OPEN\nx\n\n### [E] The terminal's shell ran on pipes -- 2026-09-24\ny\n"
    _pre, ents, _d = D.parse_issues_monolith(tagged, "known-issues.md", archived=False)
    check("a `### [E]` prose heading is an entry, not a subsection", len(ents), 2)
    check("...filed under its lane and its prose, without the status tail",
          ents[1].key, "E-the-terminals-shell-ran-on-pipes")
    sub = "# KI\n\n## TD-A-X (lane A) — OPEN\nx\n\n### The shape of the fix\ny\n"
    check("a prose `###` without a tag is still a subsection",
          len(D.parse_issues_monolith(sub, "known-issues.md", archived=False)[1]), 1)

    check("an id inside backticks is read", D.issue_id("`TD-C-TWENTY-ONE-APPS` (lane C) -- **ALL EXAMINED**"),
          "TD-C-TWENTY-ONE-APPS")
    check("a prose heading has no id", D.issue_id("The renderer and the hit test disagree"), "")


@case
def statuses() -> None:
    def st(title: str, body: str = "") -> str:
        lines = [f"## {title}\n", *[ln + "\n" for ln in body.splitlines()]]
        return D.issue_status(title, lines, archived=False)

    check("an em-dash FIXED tail is closed", st("TD-B-X (lane B) — FIXED 2026-10-01"), "closed")
    check("a `--` FIXED tail is closed (ki_split read it as open)", st("TD-B-X (lane B) -- FIXED 2026-10-01"), "closed")
    check("a bold FIXED line opening the body is closed", st("TD-B-X (lane B)", "**FIXED 2026-09-14 — all four.**"),
          "closed")
    check("an explicit OPEN wins over a RESOLVED upstream", st("TD-B-X — RESOLVED upstream — OPEN (host-blocked)"),
          "open")
    check("a hedge keeps it open", st("TD-B-X — FIXED? partly"), "open")
    check("CLOSED inside the id is not a status", st("TD-B-THE-SOCKET-THAT-CLOSED — 2026-09-01"), "open")
    check("no marker at all is open", st("TD-B-X (lane B, 2026-09-01)"), "open")


@case
def lanes() -> None:
    check("a `[E]` tag", D.lane_of("[E] Something", ""), "E")
    check("a tag after a question id", D.lane_of("D-Q7 — [D] The C library ...", ""), "D")
    check("a **Lane:** field", D.lane_of("Title", "**Date:** x\n**Lane:** C\n"), "C")
    check("\"(lane B, ...)\" in the heading", D.lane_of("B-HOSTNAME-X (lane B, 2026-09-14)", "", "B-HOSTNAME-X"), "B")
    check("TD-B- is lane B", D.lane_of("TD-B-X", "", "TD-B-X"), "B")
    check("a bare B- is not lane B (it meant 'bug')", D.lane_of("B-MOUNT-ACCEPTS-X", "", "B-MOUNT-ACCEPTS-X"), "")
    check("a question id is its lane", D.lane_of("A-Q14: When ...", "", "A-Q14"), "A")
    bands = D.parse_bands("| §900–§999 | **lane A** | **open** | x |\n| §200–§299 | **lane A** | closed | y |\n")
    check("the band table is read", bands, [(900, 999, "A", "open"), (200, 299, "A", "closed")])
    check("§217-§220 are lane C's whatever the table says", D.band_lane(218, [(200, 299, "A", "closed")]), "C")


# --- docs_layout ------------------------------------------------------------------------------

KI = """# Known Issues

Header text.

---

## TD-A-OPEN-ONE (lane A, 2026-09-01) — OPEN
**Status:** OPEN

body one

---

## TD-B-FIXED-ONE (lane B, 2026-09-01) — FIXED 2026-09-02

body two

---

## TD-B-SHARED (lane B, 2026-09-01) — OPEN
first copy

---

## TD-B-SHARED (lane B, 2026-09-03) — OPEN
follow-up with the same id
"""

KIR = """# Known Issues — Resolved Archive

# Lane A

### TD-A-OLD — FIXED 2026-08-01
old

### TD-A-REOPENED — 2026-08-01 — OPEN (host-blocked)
archived by mistake
"""

DD = """# Design Decisions Log

## Numbering and file order

| Band | Owner | Status | Region |
|---|---|---|---|
| §100–§199 | **lane A** | **open** | x |

## 100. First
**Lane:** A
text

## §101 — Second
**Lane:** A
## A stray level-two heading inside the entry
still the second entry
"""

OQ = """# Open Questions

intro

## A-Q2 — [A] Which way? — Status: OPEN (raised 2026-10-01)
question text

# Resolved

## Resolved — lane A

- A-Q1 — answered 2026-09-01, design-decisions §100
"""

RM = """# Roadmap

## Phase 1
- [x] `[A]` finished
  - [x] its done step
- [x] `[B]` batch, stale mark
  - [ ] still open
- [ ] `[A]` open task
"""


@case
def conversion() -> None:
    conv = L.convert({D.ISSUES_MONO: KI, D.ISSUES_CLOSED_MONO: KIR, D.DECISIONS_MONO: DD,
                      D.QUESTIONS_MONO: OQ}, roadmap=RM)
    files = conv.files
    check("an open issue is filed open", "known-issues/TD-A-OPEN-ONE.md" in files, True)
    check("a fixed issue left in the open file is filed resolved", "known-issues-resolved/TD-B-FIXED-ONE.md" in files,
          True)
    check("an archived issue that says OPEN is filed open", "known-issues/TD-A-REOPENED.md" in files, True)
    check("a shared id gets -2 for the later entry", "known-issues/TD-B-SHARED-2.md" in files, True)
    check("entry text is carried byte for byte",
          files["known-issues/TD-A-OPEN-ONE.md"], "## TD-A-OPEN-ONE (lane A, 2026-09-01) — OPEN\n"
          "**Status:** OPEN\n\nbody one\n")
    check("decisions are NNNN-<slug>", sorted(p for p in files if p.startswith("design-decisions/0")),
          ["design-decisions/0100-first.md", "design-decisions/0101-second.md"])
    check("a stray `##` inside a decision stays in it", "still the second entry" in files["design-decisions/0101-second.md"],
          True)
    check("the band table reaches the README", "| §100–§199 | **lane A** | **open** | x |" in
          files["design-decisions/README.md"], True)
    check("a question is its own file", "open-questions/A-Q2.md" in files, True)
    check("the resolved index is split per lane", "- A-Q1" in files["open-questions-resolved/lane-a.md"], True)
    check("verify() finds a faithful conversion clean", L.verify(conv), [])
    road = files["roadmap.md"]
    check("a fully done item leaves roadmap.md", "finished" in road, False)
    check("an [x] item with an open sub-item stays", "batch, stale mark" in road and "still open" in road, True)
    check("...and the done item lands under its heading", "## Phase 1\n" in files["roadmap-done.md"] and
          "finished" in files["roadmap-done.md"], True)
    # The verifier must fail when the conversion is wrong, not only pass when it is right.
    conv.files["known-issues/TD-A-OPEN-ONE.md"] = conv.files["known-issues/TD-A-OPEN-ONE.md"].replace("body one\n", "")
    check("verify() catches an entry that lost a line", any("line multiset" in p for p in L.verify(conv)), True)
    conv2 = L.convert({D.ISSUES_MONO: KI})
    conv2.files["known-issues/TD-B-MOVED.md"] = conv2.files.pop("known-issues-resolved/TD-B-FIXED-ONE.md")
    conv2.placed["known-issues/TD-B-MOVED.md"] = conv2.placed.pop("known-issues-resolved/TD-B-FIXED-ONE.md")
    check("verify() catches a closed issue filed as open",
          any("filed as open" in p for p in L.verify(conv2)), True)
    check("the conversion is deterministic", L.convert({D.ISSUES_MONO: KI}).files, L.convert({D.ISSUES_MONO: KI}).files)


@case
def roadmap_merge_into_existing_done_file() -> None:
    road, done, _n = L.archive_roadmap(RM)
    more = road.replace("- [ ] `[A]` open task", "- [x] `[A]` open task, now done")
    road2, done2, moved = L.archive_roadmap(more, done)
    check("--fix-roadmap moves a newly done item", moved, 1)
    check("...into the existing section, not a second copy of the heading", done2.count("## Phase 1"), 1)
    check("...after what was already there", done2.index("finished") < done2.index("now done"), True)


# --- the tools end to end, in scratch repositories ----------------------------------------------

def run(cwd: Path, *args: str, check_rc: bool = True) -> subprocess.CompletedProcess:
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    env["PYTHONIOENCODING"] = "utf-8"
    r = subprocess.run(args, cwd=cwd, capture_output=True, text=True, encoding="utf-8", env=env)
    if check_rc and r.returncode != 0:
        raise AssertionError(f"{args} -> {r.returncode}\n{r.stdout}\n{r.stderr}")
    return r


def script(name: str) -> str:
    return str(HERE / name)


def new_repo(tmp: Path) -> Path:
    root = tmp / "repo"
    root.mkdir()
    run(root, "git", "init", "-q", "-b", "main")
    run(root, "git", "config", "user.email", "t@example.com")
    run(root, "git", "config", "user.name", "t")
    run(root, "git", "config", "commit.gpgsign", "false")
    return root


def write(root: Path, rel: str, text: str) -> None:
    p = root / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_bytes(text.encode("utf-8"))


def commit(root: Path, msg: str) -> None:
    run(root, "git", "add", "-A")
    run(root, "git", "commit", "-q", "-m", msg)


def seed(root: Path) -> None:
    # Fifty more closed entries so check-docs.py will give a verdict at all.
    extra = "".join(f"\n---\n\n### TD-A-FILLER-{i} — FIXED 2026-08-01\nfiller\n" for i in range(55))
    write(root, D.ISSUES_MONO, KI)
    write(root, D.ISSUES_CLOSED_MONO, KIR + extra)
    write(root, D.DECISIONS_MONO, DD)
    write(root, D.QUESTIONS_MONO, OQ)
    write(root, "roadmap.md", RM)
    write(root, "todo.txt", "## Lane A — kernel\n## * a note\n##\n")
    commit(root, "base")


@case
def migrate_then_search() -> None:
    with tempfile.TemporaryDirectory() as t:
        root = new_repo(Path(t))
        seed(root)
        r = run(root, sys.executable, script("docs-migrate.py"))
        check("docs-migrate.py verifies and writes", "parse-back" in r.stdout, True)
        check("the old file is a signpost", D.is_signpost(root / D.ISSUES_MONO), True)
        run(root, sys.executable, script("check-docs.py"), "--write-baseline")
        check("check-docs passes the migrated tree",
              run(root, sys.executable, script("check-docs.py"), check_rc=False).returncode, 0)
        commit(root, "cutover")
        s = run(root, sys.executable, script("docsearch.py"), "TD-B-FIXED-ONE", "--json", "--no-semantic")
        hits = json.loads(s.stdout)
        check("docsearch finds an id exactly", hits[0]["path"], "known-issues-resolved/TD-B-FIXED-ONE.md")
        s = run(root, sys.executable, script("docsearch.py"), "body", "--kind", "issue", "--status", "open",
                "--json", "--no-semantic")
        check("...filters by status", {h["status"] for h in json.loads(s.stdout)}, {"open"})
        write(root, "known-issues/TD-A-NEW.md", "## TD-A-NEW (lane A) — OPEN\n**Status:** OPEN\nzebra\n")
        s = run(root, sys.executable, script("docsearch.py"), "zebra", "--json", "--no-semantic")
        check("...and sees a file added since the index was built", json.loads(s.stdout)[0]["path"],
              "known-issues/TD-A-NEW.md")
        # Printed, not as JSON, through a pipe in the platform's own encoding --
        # `run` sets PYTHONIOENCODING, so this one does not: a title with a `→`
        # raised UnicodeEncodeError under Windows' cp1252 and the search stopped
        # after its first hit or two.
        write(root, "known-issues/TD-A-ARROW.md",
              "## TD-A-ARROW (lane A) left → right — OPEN\n**Status:** OPEN\nleft → right\n")
        bare = {k: v for k, v in os.environ.items()
                if not k.startswith("GIT_") and k not in ("PYTHONIOENCODING", "PYTHONUTF8")}
        r = subprocess.run([sys.executable, script("docsearch.py"), "TD-A-ARROW", "--no-semantic"],
                           cwd=root, capture_output=True, env=bare)
        # (The label is ASCII: this file prints its own labels through the
        # same kind of pipe.)
        check("...and prints an arrow whatever the console's code page",
              (r.returncode, b"Traceback" in r.stderr), (0, False))


@contextlib.contextmanager
def no_git_env():
    """For cases that call the tools in this process. They run git with this
    process's environment, and a hook's GIT_DIR there would point them at the real
    repository -- its shared cache, its server pid file -- instead of the scratch one."""
    saved = {k: os.environ.pop(k) for k in list(os.environ) if k.startswith("GIT_")}
    try:
        yield
    finally:
        os.environ.update(saved)


@case
def semantic_search_waits_for_the_vectors() -> None:
    """Meaning joins the ranking only once nearly every candidate has a vector: with
    200 of 1,046 decisions embedded, the embedded ones crowded out the decision a
    query was about (2026-10-02). And that is decided before a server is started."""
    import docsearch as S
    try:
        import numpy as np
    except ImportError:
        print("SKIP  semantic_search_waits_for_the_vectors (numpy is not installed)")
        return
    started: list[Path] = []
    real_ensure = S.ensure_server
    S.ensure_server = lambda root, quiet: started.append(root) or False  # never a real server here
    try:
        with no_git_env(), tempfile.TemporaryDirectory() as t:
            root = new_repo(Path(t))
            seed(root)
            run(root, sys.executable, script("docs-migrate.py"))
            commit(root, "cutover")

            def search() -> dict:
                return S.search(root, "body", kinds=[], lane="", status="", since="", n=3, semantic=True,
                                quiet=True)[1]

            db, cache = S.open_index(root), S.open_cache(root)
            try:
                S.refresh(root, db, cache)
                ents = db.execute("SELECT id, text_hash FROM entries ORDER BY id").fetchall()
                target = ents[3][0]

                def embed(subset) -> None:
                    cache.executemany("INSERT OR REPLACE INTO vectors VALUES (?,?,?,?)", [
                        (th, S.EMBED_MODEL_NAME, 4,
                         np.asarray([1, 0, 0, 0] if eid == target else [0, 1, 0, 0], dtype=np.float32).tobytes())
                        for eid, th in subset])
                    cache.commit()

                check("no vectors: meaning is off, and says why", S.embedded_candidates(db, cache, None),
                      "no vectors yet")
                half = ents[: len(ents) // 2]
                embed(half)
                got = S.embedded_candidates(db, cache, None)
                check("half embedded: still off", isinstance(got, str) and "embedded so far" in got, True)
                info = search()
                check("...which a search reports, without starting a server",
                      (info["semantic"].startswith("off (only "), started), (True, []))
                check("a filter whose entries are all embedded: on",
                      isinstance(S.embedded_candidates(db, cache, {eid for eid, _ in half}), tuple), True)
                embed(ents)
                got = S.embedded_candidates(db, cache, None)
                check("all embedded: on", isinstance(got, tuple) and len(got[0]) == len(ents), True)
                ranks = S.semantic_ranks(*got, [0.9, 0.1, 0.0, 0.0])
                check("the nearest vector ranks first", min(ranks, key=ranks.get), target)
                info = search()
                check("...and only now is a server wanted", (len(started), info["semantic"]),
                      (1, "off (no embedding model on this machine)"))
            finally:
                db.close()
                cache.close()
    finally:
        S.ensure_server = real_ensure


@case
def stop_server_stops_only_its_own_server() -> None:
    """`--stop-server` once reported "not running" about a server that was running
    (taskkill without /F cannot close a process that has no window) and forgot its
    pid. It must stop the process it started, and leave alone any other program --
    including one that has since been given the same PID."""
    import docsearch as S
    sleeper = [sys.executable, "-c", "import time; time.sleep(120)"]
    flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    real_exe = S.SERVER_EXE
    with no_git_env(), tempfile.TemporaryDirectory() as t:
        root = new_repo(Path(t))
        mine = subprocess.Popen(sleeper, creationflags=flags)
        other = subprocess.Popen(sleeper, creationflags=flags)
        try:
            check("a live process running another program is not ours",
                  S._terminate_if_ours(other.pid, exe=Path(t) / "llama-server.exe"), "not ours")
            check("...and is left running", other.poll(), None)
            pid_file = S._pid_file(root)
            pid_file.write_text(str(other.pid), encoding="utf-8", newline="")
            msg = S.stop_server(root)
            check("--stop-server leaves a program that is not its server alone",
                  ("left it alone" in msg, other.poll()), (True, None))
            check("...and forgets that pid", pid_file.exists(), False)
            S.SERVER_EXE = Path(sys.executable)  # make `mine` the server, as far as the check can tell
            pid_file.write_text(str(mine.pid), encoding="utf-8", newline="")
            msg = S.stop_server(root)
            check("--stop-server stops the server it started", msg.startswith("stopped the embedding server"), True)
            check("...which has exited", mine.wait(timeout=10) is not None, True)
            check("a pid whose process has exited is gone", S._terminate_if_ours(mine.pid), "gone")
            check("no pid file: nothing to stop", S.stop_server(root), "no embedding server was started by docsearch")
            check("the other program is still running", other.poll(), None)
        finally:
            S.SERVER_EXE = real_exe
            for p in (mine, other):
                if p.poll() is None:
                    p.kill()
                p.wait()


def lane_and_cutover(tmp: Path) -> Path:
    """main: base -> cutover (+ a later edit of one entry). lane: base -> its own edits."""
    root = new_repo(tmp)
    seed(root)
    run(root, "git", "branch", "lane")
    run(root, sys.executable, script("docs-migrate.py"))
    run(root, sys.executable, script("check-docs.py"), "--write-baseline")
    commit(root, "cutover")
    write(root, "known-issues/TD-A-OPEN-ONE.md",
          "## TD-A-OPEN-ONE (lane A, 2026-09-01) — OPEN\n**Status:** OPEN\n\nbody one\n\nmain added this line\n")
    commit(root, "main edits an entry")
    run(root, "git", "checkout", "-q", "lane")
    return root


@case
def carry_forward_every_kind_of_edit() -> None:
    with tempfile.TemporaryDirectory() as t:
        root = lane_and_cutover(Path(t))
        ki = KI.replace("first copy", "first copy, edited by the lane")  # changed
        ki = ki.replace("## TD-A-OPEN-ONE (lane A, 2026-09-01) — OPEN\n**Status:** OPEN\n",
                        "## TD-A-OPEN-ONE (lane A, 2026-09-01) — OPEN\n**Status:** OPEN\n")  # untouched by lane
        ki += "\n---\n\n## TD-B-BRAND-NEW (lane B, 2026-10-01) — OPEN\n**Status:** OPEN\nnew\n"  # added
        ki = ki.replace("## TD-B-SHARED (lane B, 2026-09-03) — OPEN", "## TD-B-SHARED (lane B, 2026-09-03) — FIXED 2026-10-01")
        write(root, D.ISSUES_MONO, ki)
        rm = RM.replace("- [ ] `[A]` open task", "- [ ] `[A]` open task, re-worded by the lane")
        write(root, "roadmap.md", rm)
        commit(root, "lane edits the old files")
        merge = run(root, "git", "merge", "--no-edit", "main", check_rc=False)
        check("merging the cutover conflicts in the old file", merge.returncode != 0, True)
        r = run(root, sys.executable, script("docs-carry-forward.py"), "--lane", "b", check_rc=False)
        check("the carry-forward reports no conflict", r.returncode, 0)
        check("...and never writes the shared fallback name",
              (root / "scripts/docs-baseline-carried-unknown.json").exists(), False)
        shared = (root / "known-issues/TD-B-SHARED.md").read_text(encoding="utf-8")
        check("a changed entry carries the lane's edit", "edited by the lane" in shared, True)
        check("a new entry gets its own file", (root / "known-issues/TD-B-BRAND-NEW.md").is_file(), True)
        check("an entry the lane marked FIXED moves to resolved",
              ((root / "known-issues-resolved/TD-B-SHARED-2.md").is_file(),
               (root / "known-issues/TD-B-SHARED-2.md").exists()), (True, False))
        check("main's own later edit survives",
              "main added this line" in (root / "known-issues/TD-A-OPEN-ONE.md").read_text(encoding="utf-8"), True)
        check("the old file is the trunk's signpost again", D.is_signpost(root / D.ISSUES_MONO), True)
        check("the lane's roadmap edit arrives",
              "re-worded by the lane" in (root / "roadmap.md").read_text(encoding="utf-8"), True)
        check("check-docs passes the result",
              run(root, sys.executable, script("check-docs.py"), "--strict", check_rc=False).returncode, 0)
        run(root, "git", "commit", "-q", "--no-edit")
        check("the merge commits", run(root, "git", "status", "--porcelain").stdout.strip(), "")


@case
def carry_forward_stops_on_a_real_conflict() -> None:
    with tempfile.TemporaryDirectory() as t:
        root = lane_and_cutover(Path(t))
        write(root, D.ISSUES_MONO, KI.replace("body one\n", "body one\n\nthe lane added a different line\n"))
        commit(root, "lane edits the entry main also edited")
        run(root, "git", "merge", "--no-edit", "main", check_rc=False)
        r = run(root, sys.executable, script("docs-carry-forward.py"), "--lane", "a", check_rc=False)
        check("a two-sided edit of one entry is reported, not resolved", r.returncode, 1)
        text = (root / "known-issues/TD-A-OPEN-ONE.md").read_text(encoding="utf-8")
        check("...with conflict markers holding both sides",
              "<<<<<<<" in text and "the lane added" in text and "main added" in text, True)
        bad = run(root, sys.executable, script("check-docs.py"), check_rc=False)
        check("...which check-docs refuses until resolved (S4)", (bad.returncode, "S4" in bad.stdout), (1, True))


@case
def baseline_keys_survive_dots_in_ids() -> None:
    """An id with a dot (`B-CP.RS`) must stay grandfathered: the key is the file name
    without `.md`, and stripping a second "extension" lost 13 entries on 2026-10-02."""
    import srcload  # the repo's loader: by path, from source, registered (dataclasses need that)
    cd = srcload.load(str(HERE / "check-docs.py"), "check_docs")
    check("a dotted id keeps its dots", cd._entry_key("known-issues/B-CP.RS.md"), "b-cp.rs")
    check("...whether given a path or a key", cd._entry_key("b-cp.rs"), "b-cp.rs")


@case
def carry_forward_will_not_guess_its_lane() -> None:
    """A worktree which-lane.py does not know, and no --lane: refused, and nothing
    written -- not a shared `docs-baseline-carried-unknown.json` two such lanes
    would both write (os-81, 2026-10-02). A --lane that is no lane is refused too."""
    with tempfile.TemporaryDirectory() as t:
        root = lane_and_cutover(Path(t))
        write(root, D.ISSUES_MONO, KI.replace("first copy", "first copy, edited by the lane"))
        commit(root, "lane edits the old file")
        run(root, "git", "merge", "--no-edit", "main", check_rc=False)
        before = run(root, "git", "status", "--porcelain").stdout
        r = run(root, sys.executable, script("docs-carry-forward.py"), check_rc=False)
        check("no lane and no --lane is refused", r.returncode, 2)
        check("...saying how to give one", "--lane" in r.stderr, True)
        check("...having written nothing",
              (run(root, "git", "status", "--porcelain").stdout == before,
               any((root / "scripts").glob("docs-baseline-carried-*.json"))), (True, False))
        r = run(root, sys.executable, script("docs-carry-forward.py"), "--lane", "unknown",
                check_rc=False)
        check("a --lane that is no lane letter is refused", r.returncode, 2)


@case
def carry_forward_refuses_outside_a_merge() -> None:
    with tempfile.TemporaryDirectory() as t:
        root = new_repo(Path(t))
        seed(root)
        r = run(root, sys.executable, script("docs-carry-forward.py"), check_rc=False)
        check("no merge in progress is refused", r.returncode, 2)


def main() -> int:
    if len(CASES) < 12:
        print(f"FATAL: only {len(CASES)} cases registered")
        return 1
    for fn in CASES:
        try:
            fn()
        except Exception as exc:  # noqa: BLE001 - report and keep going
            check(f"{fn.__name__} ran without an exception", repr(exc)[:300], None)
    print(f"\n{len(failures)} failure(s)" if failures else "\nall docs-tools tests passed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
