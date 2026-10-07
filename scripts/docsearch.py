#!/usr/bin/env python3
"""Search the shared documents, by words and by meaning, and get the entries back.

    python scripts/docsearch.py "why do we push a SHA to main instead of merging"
    python scripts/docsearch.py getopt --kind issue --lane B --status open
    python scripts/docsearch.py 538 --kind decision      # or "§538": numbers and ids are looked up directly
    python scripts/docsearch.py B-HOSTNAME-RESOLVES-THE-DOMAIN-WITHOUT-ETC-HOSTS --full

WHY IT EXISTS. The shared documents are too large to read and were being searched
by hand: between June and October 2026, agents spent ~44,000 tool calls grepping and
reading `known-issues.md`, `design-decisions.md`, `roadmap.md`, `todo.txt` and the
question files, pulling ~17 M tokens of them into context. grep finds exact wording
only, returns lines rather than entries, and cannot tell a fixed issue from an open
one. This returns *entries*, ranked, filtered, each with its file and line, so the
reader goes on to read the real text -- the index is never a second source of truth.

WHAT IT SEARCHES. Every entry `doc_entries.read_tree()` sees: issues (open and
resolved), decisions, open and deferred questions, roadmap items (open and done),
`todo.txt` paragraphs, requests, and passages of the spec files. Both document
layouts are understood, so a lane branch that has not merged the per-entry cutover
searches correctly too.

HOW IT RANKS. Words: SQLite FTS5 (BM25, Porter stemming), weighting the heading and
the id above the body. Meaning: when the shared cache holds embeddings for the
entries (filled in batch by the local-LLM tooling, `agent-knowledge`), the query is
embedded too and the two rankings are fused by reciprocal rank. A query that looks
like an id (`TD-B-FOO`, `§538`, `A-Q14`, `DQ3`) is first looked up exactly.

SUMMARY CARDS. The same batch job writes a card per entry -- a one-line summary, a
status read from the text, subject tags, and the questions the entry answers -- into
the shared cache. Cards are searched as an extra field and shown under each hit;
they are labelled as summaries, and are never written into a document.

WHERE THINGS LIVE.
  <git-dir>/docsearch/index.sqlite     this checkout's index, refreshed per changed file
  <git-common-dir>/docsearch-cache.sqlite   cards and vectors, shared by every worktree
Both are derived: delete them and the next search rebuilds what it needs.

SEMANTIC QUERIES need vectors for the entries and the query embedded. Meaning joins
the ranking only once at least 90% of the entries being searched have a vector --
until then, the few that do would crowd out better keyword matches -- and that is
decided before any server is started. If no embedding server answers on
127.0.0.1:8093, one is started on the CPU (no VRAM, ~1 GB RAM) and left running for
later searches; `--stop-server` stops it (only if that PID is still the server it
started). Without the model on this machine, search is keyword-only and says so; the
last line of the output always says whether meaning was used, and if not, why.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sqlite3
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import doc_entries as D  # noqa: E402

INDEX_VERSION = "1"  # bump when the schema or the parser's output changes
EMBED_MODEL_NAME = "Qwen3-Embedding-0.6B-Q8_0"
EMBED_INPUT_CHARS = 2000
QUERY_INSTRUCTION = ("Given a question about a software project's issues, design decisions, roadmap, notes and "
                     "requests, retrieve the entries that answer it")
SERVER_PORT = int(os.environ.get("DOCSEARCH_EMBED_PORT", "8093"))
LLAMA_DIR = Path(os.environ.get("DOCSEARCH_LLAMA_DIR", "D:/ai/llama.cpp/b11344"))
SERVER_EXE = LLAMA_DIR / "llama-server.exe"
EMBED_MODEL = Path(os.environ.get("DOCSEARCH_EMBED_MODEL",
                                  "D:/ai/Qwen/Qwen3-Embedding-0.6B-GGUF/Qwen3-Embedding-0.6B-Q8_0.gguf"))
ID_QUERY = re.compile(r"^(?:\u00a7\s*\d+|\d{1,4}|(?:[A-F]-)?Q\d+|DQ\d+|[A-Za-z0-9]+(?:-[A-Za-z0-9.]+){1,})$")

INDEX_SCHEMA = """
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE IF NOT EXISTS sources (path TEXT PRIMARY KEY, size INTEGER, mtime_ns INTEGER);
CREATE TABLE IF NOT EXISTS entries (
  id INTEGER PRIMARY KEY, source TEXT NOT NULL, kind TEXT, key TEXT, title TEXT, path TEXT,
  line INTEGER, end_line INTEGER, lane TEXT, status TEXT, date TEXT, number INTEGER,
  section TEXT, text_hash TEXT, text TEXT
);
CREATE INDEX IF NOT EXISTS entries_source ON entries(source);
CREATE INDEX IF NOT EXISTS entries_key ON entries(key COLLATE NOCASE);
CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(title, key, card, body, tokenize='porter unicode61');
"""

CACHE_SCHEMA = """
CREATE TABLE IF NOT EXISTS cards (
  text_hash TEXT PRIMARY KEY, model TEXT, summary TEXT, status TEXT, tags TEXT, questions TEXT, created TEXT
);
CREATE TABLE IF NOT EXISTS vectors (
  text_hash TEXT NOT NULL, model TEXT NOT NULL, dim INTEGER, vec BLOB, PRIMARY KEY (text_hash, model)
);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT);
"""


# --- the shared cache (also written by agent-knowledge's batch job) ------------------

def cache_path(root: Path) -> Path:
    return D.git_common_dir(root) / "docsearch-cache.sqlite"


def open_cache(root: Path) -> sqlite3.Connection:
    db = sqlite3.connect(cache_path(root), timeout=30)
    db.execute("PRAGMA journal_mode=WAL")
    db.executescript(CACHE_SCHEMA)
    return db


def card_text(card: tuple | None) -> str:
    """The searchable text of a card: summary, tags, and the questions it answers."""
    if not card:
        return ""
    summary, _status, tags, questions = card
    parts = [summary or ""]
    for blob in (tags, questions):
        try:
            parts.extend(json.loads(blob or "[]"))
        except ValueError:
            pass
    return "\n".join(p for p in parts if p)


def embedding_input(text: str) -> str:
    """What gets embedded for an entry: its heading and the start of its body."""
    return text[:EMBED_INPUT_CHARS]


# --- the per-checkout index -------------------------------------------------------------

def index_dir(root: Path) -> Path:
    d = D.git_dir(root) / "docsearch"
    d.mkdir(parents=True, exist_ok=True)
    return d


def open_index(root: Path, rebuild: bool = False) -> sqlite3.Connection:
    path = index_dir(root) / "index.sqlite"
    if rebuild:
        # The WAL and shared-memory files too: a fresh database next to an old
        # write-ahead log would have the old pages replayed into it.
        for p in (path, path.with_name(path.name + "-wal"), path.with_name(path.name + "-shm")):
            p.unlink(missing_ok=True)
    db = sqlite3.connect(path, timeout=30)
    db.execute("PRAGMA journal_mode=WAL")
    db.executescript(INDEX_SCHEMA)
    ver = db.execute("SELECT value FROM meta WHERE key='version'").fetchone()
    if ver is None or ver[0] != INDEX_VERSION:
        db.executescript("DELETE FROM entries; DELETE FROM fts; DELETE FROM sources;")
        db.execute("INSERT OR REPLACE INTO meta VALUES ('version', ?)", (INDEX_VERSION,))
        db.commit()
    return db


def refresh(root: Path, db: sqlite3.Connection, cache: sqlite3.Connection | None) -> dict:
    """Bring the index up to date with the files, one changed file at a time."""
    t0 = time.time()
    current: dict[str, tuple[int, int]] = {}
    for rel in D.source_files(root):
        try:
            st = (root / rel).stat()
        except OSError:
            continue
        current[rel] = (st.st_size, st.st_mtime_ns)
    known = {r[0]: (r[1], r[2]) for r in db.execute("SELECT path, size, mtime_ns FROM sources")}
    changed = [p for p, fp in current.items() if known.get(p) != fp]
    removed = [p for p in known if p not in current]
    # The band table feeds every decision's lane: when it changes, re-read them all.
    if f"{D.DECISIONS_DIR}/README.md" in changed:
        changed += [p for p in current if p.startswith(D.DECISIONS_DIR + "/") and p not in changed]
    cards = {}
    if cache is not None and changed:
        cards = {r[0]: r[1:] for r in cache.execute("SELECT text_hash, summary, status, tags, questions FROM cards")}
    n_new = 0
    for rel in removed + changed:
        ids = [r[0] for r in db.execute("SELECT id FROM entries WHERE source=?", (rel,))]
        db.executemany("DELETE FROM fts WHERE rowid=?", [(i,) for i in ids])
        db.execute("DELETE FROM entries WHERE source=?", (rel,))
        db.execute("DELETE FROM sources WHERE path=?", (rel,))
    for rel in changed:
        try:
            entries = D.read_file(root, rel)
        except ValueError as exc:  # an unbalanced fence: index nothing from this file, loudly
            print(f"docsearch: skipping {rel}: {exc}", file=sys.stderr)
            entries = []
        for e in entries:
            cur = db.execute(
                "INSERT INTO entries (source, kind, key, title, path, line, end_line, lane, status, date, number,"
                " section, text_hash, text) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                (rel, e.kind, e.key, e.title, e.path, e.line, e.end_line, e.lane, e.status, e.date, e.number,
                 e.section, e.text_hash, e.text),
            )
            db.execute("INSERT INTO fts (rowid, title, key, card, body) VALUES (?,?,?,?,?)",
                       (cur.lastrowid, e.title, e.key.replace("-", " "), card_text(cards.get(e.text_hash)), e.text))
            n_new += 1
        size, mtime = current[rel]
        db.execute("INSERT OR REPLACE INTO sources VALUES (?,?,?)", (rel, size, mtime))
    _sync_cards(db, cache)
    db.commit()
    return {"files_changed": len(changed), "files_removed": len(removed), "entries_indexed": n_new,
            "seconds": round(time.time() - t0, 2)}


def _sync_cards(db: sqlite3.Connection, cache: sqlite3.Connection | None) -> None:
    """Copy cards written since the last sync into the card column."""
    if cache is None:
        return
    stamp = (cache.execute("SELECT value FROM meta WHERE key='cards_updated'").fetchone() or ("",))[0]
    seen = (db.execute("SELECT value FROM meta WHERE key='cards_synced'").fetchone() or ("",))[0]
    if not stamp or stamp == seen:
        return
    cards = {r[0]: r[1:] for r in cache.execute("SELECT text_hash, summary, status, tags, questions FROM cards")}
    for eid, th in db.execute("SELECT id, text_hash FROM entries").fetchall():
        if th in cards:
            db.execute("UPDATE fts SET card=? WHERE rowid=?", (card_text(cards[th]), eid))
    db.execute("INSERT OR REPLACE INTO meta VALUES ('cards_synced', ?)", (stamp,))


# --- semantic search ---------------------------------------------------------------------

def _server_up() -> bool:
    import urllib.request
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{SERVER_PORT}/health", timeout=2) as r:
            return r.status == 200
    except OSError:
        return False


def _pid_file(root: Path) -> Path:
    return D.git_common_dir(root) / "docsearch-server.pid"


def ensure_server(root: Path, quiet: bool) -> bool:
    """An embedding server on SERVER_PORT, started on the CPU if none is running."""
    if _server_up():
        return True
    exe = SERVER_EXE
    if not exe.exists() or not EMBED_MODEL.exists():
        return False
    log = open(D.git_common_dir(root) / "docsearch-server.log", "ab")  # noqa: SIM115 - handed to the child
    args = [str(exe), "-m", str(EMBED_MODEL), "--embedding", "--pooling", "last", "-ngl", "0", "-c", "4096",
            "-np", "1", "--host", "127.0.0.1", "--port", str(SERVER_PORT), "--no-webui"]
    flags = 0
    if sys.platform == "win32":
        flags = subprocess.CREATE_NEW_PROCESS_GROUP | subprocess.DETACHED_PROCESS
    proc = None
    for extra in (0x01000000, 0):  # CREATE_BREAKAWAY_FROM_JOB if allowed, else without
        try:
            proc = subprocess.Popen(args, stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
                                    creationflags=flags | extra, cwd=str(LLAMA_DIR))
            break
        except OSError:
            continue
    if proc is None:
        return False
    _pid_file(root).write_text(str(proc.pid), encoding="utf-8", newline="")
    for _ in range(60):
        if _server_up():
            if not quiet:
                print(f"(started a CPU embedding server, pid {proc.pid}, on :{SERVER_PORT}; it stays up for "
                      "later searches -- `docsearch.py --stop-server` stops it)", file=sys.stderr)
            return True
        if proc.poll() is not None:
            return False
        time.sleep(0.5)
    return False


def _same_file(a: Path | str, b: Path | str) -> bool:
    return os.path.normcase(str(Path(a).resolve())) == os.path.normcase(str(Path(b).resolve()))


def _terminate_if_ours(pid: int, exe: Path | None = None) -> str:
    """Terminate `pid` if, and only if, it is still running `exe` (by default the
    llama-server this script starts).

    The pid file can outlive its process, and Windows reuses PIDs, so the number
    alone may by now name somebody else's program. The check and the kill go
    through one process handle, which pins the process: it cannot exit and have its
    PID handed to another between the two. Returns "stopped", "gone" or "not ours".
    (`taskkill /PID` without /F cannot do this job at all: it asks the process to
    close its windows, and a server started detached has none.)"""
    exe = exe or SERVER_EXE
    if sys.platform != "win32":
        # No handle to pin the process with here (short of pidfd), so this is the
        # best-effort version: check /proc, then signal.
        try:
            target = os.readlink(f"/proc/{pid}/exe")
        except FileNotFoundError:
            return "gone"
        except OSError:  # e.g. another user's process: certainly not ours
            return "not ours"
        if not _same_file(target, exe):
            return "not ours"
        try:
            os.kill(pid, 15)
        except ProcessLookupError:
            return "gone"
        return "stopped"
    import ctypes
    from ctypes import wintypes
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.OpenProcess.restype = wintypes.HANDLE
    k32.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
    k32.QueryFullProcessImageNameW.argtypes = (wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR,
                                               ctypes.POINTER(wintypes.DWORD))
    k32.GetExitCodeProcess.argtypes = (wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD))
    k32.TerminateProcess.argtypes = (wintypes.HANDLE, wintypes.UINT)
    k32.WaitForSingleObject.argtypes = (wintypes.HANDLE, wintypes.DWORD)
    k32.CloseHandle.argtypes = (wintypes.HANDLE,)
    # PROCESS_TERMINATE | SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION
    h = k32.OpenProcess(0x0001 | 0x00100000 | 0x1000, False, pid)
    if not h:
        return "gone"
    try:
        code = wintypes.DWORD()
        if not k32.GetExitCodeProcess(h, ctypes.byref(code)) or code.value != 259:  # STILL_ACTIVE
            return "gone"
        buf = ctypes.create_unicode_buffer(32768)
        size = wintypes.DWORD(len(buf))
        if not k32.QueryFullProcessImageNameW(h, 0, buf, ctypes.byref(size)) or not _same_file(buf.value, exe):
            return "not ours"
        if not k32.TerminateProcess(h, 1):
            raise OSError(ctypes.get_last_error(), f"TerminateProcess({pid}) failed")
        k32.WaitForSingleObject(h, 10_000)
        return "stopped"
    finally:
        k32.CloseHandle(h)


def stop_server(root: Path) -> str:
    pf = _pid_file(root)
    if not pf.exists():
        return "no embedding server was started by docsearch"
    try:
        pid = int(pf.read_text(encoding="utf-8").strip() or 0)
    except ValueError:
        pid = 0
    outcome = _terminate_if_ours(pid) if pid else "gone"
    # Forget the pid only once it no longer names our server; if termination
    # raised, the file stays and a second --stop-server can try again.
    pf.unlink(missing_ok=True)
    if outcome == "stopped":
        return f"stopped the embedding server (pid {pid})"
    if outcome == "not ours":
        return (f"pid {pid} is no longer the embedding server docsearch started (another program has that "
                "number now); left it alone")
    return f"the embedding server docsearch started (pid {pid}) had already exited"


def embed_query(q: str) -> list[float] | None:
    import urllib.request
    body = json.dumps({"input": [f"Instruct: {QUERY_INSTRUCTION}\nQuery: {q}"], "model": "local"}).encode()
    req = urllib.request.Request(f"http://127.0.0.1:{SERVER_PORT}/v1/embeddings", data=body,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return json.loads(r.read())["data"][0]["embedding"]
    except (OSError, ValueError, KeyError, IndexError):
        return None


MIN_VECTOR_COVERAGE = 0.9


def embedded_candidates(db: sqlite3.Connection, cache: sqlite3.Connection,
                        allowed: set[int] | None) -> tuple[list[int], object] | str:
    """The candidate entries' ids and their vectors as a matrix -- or, when meaning
    cannot be used for this search, a short reason why.

    Meaning is only used once nearly every candidate has a vector. While the cache
    is being filled, the entries that happen to be embedded would outrank better
    keyword matches that are not: measured, with 200 of 1,046 decisions embedded,
    the decision a query was about fell out of the top three. Entries added since
    the last nightly run are the few percent this threshold tolerates; they still
    rank by their words."""
    try:
        import numpy as np
    except ImportError:
        return "numpy is not installed"
    vecs = dict(cache.execute("SELECT text_hash, vec FROM vectors WHERE model=?", (EMBED_MODEL_NAME,)))
    if not vecs:
        return "no vectors yet"
    ids, rows, candidates = [], [], 0
    for eid, th in db.execute("SELECT id, text_hash FROM entries"):
        if allowed is None or eid in allowed:
            candidates += 1
            if th in vecs:
                ids.append(eid)
                rows.append(np.frombuffer(vecs[th], dtype=np.float32))
    if not rows or len(rows) < MIN_VECTOR_COVERAGE * candidates:
        return f"only {len(rows)} of {candidates} entries embedded so far"
    return ids, np.vstack(rows)


def semantic_ranks(ids: list[int], mat, qvec: list[float]) -> dict[int, int]:
    """entry id -> rank by cosine similarity to the query (vectors are unit length)."""
    import numpy as np
    q = np.asarray(qvec, dtype=np.float32)
    q /= np.linalg.norm(q) or 1.0
    sims = mat @ q
    order = np.argsort(-sims)[:300]
    return {ids[i]: r for r, i in enumerate(order)}


# --- querying -------------------------------------------------------------------------------

def fts_query(q: str) -> str:
    """User words -> an FTS5 OR-query of quoted tokens (operators in the input are not honoured)."""
    toks = [t for t in re.findall(r"[\w\u00c0-\uffff]+", q.lower()) if len(t) > 1 or t.isdigit()]
    return " OR ".join(f'"{t}"' for t in dict.fromkeys(toks))


def search(root: Path, q: str, *, kinds: list[str], lane: str, status: str, since: str, n: int,
           semantic: bool, quiet: bool) -> tuple[list[sqlite3.Row], dict]:
    """The best `n` entries for `q` and a dict saying how they were found. The rows
    are fetched in full, so they stay readable after the databases are closed."""
    db = open_index(root)
    db.row_factory = sqlite3.Row
    try:
        cache = open_cache(root)
    except sqlite3.Error:
        cache = None
    try:
        return _search(root, db, cache, q, kinds=kinds, lane=lane, status=status, since=since, n=n,
                       semantic=semantic, quiet=quiet)
    finally:
        # Closed here, not left to the garbage collector: on Windows an open
        # database file cannot be deleted, which a caller cleaning up may need.
        db.close()
        if cache is not None:
            cache.close()


def _search(root: Path, db: sqlite3.Connection, cache: sqlite3.Connection | None, q: str, *, kinds: list[str],
            lane: str, status: str, since: str, n: int, semantic: bool,
            quiet: bool) -> tuple[list[sqlite3.Row], dict]:
    info = refresh(root, db, cache)

    def filters(prefix: str) -> tuple[str, list]:
        """' AND ...' restricting entries (columns qualified with `prefix`)."""
        where, params = [], []
        if kinds:
            where.append(f"{prefix}kind IN ({','.join('?' * len(kinds))})")
            params += kinds
        if lane:
            where.append(f"{prefix}lane = ?")
            params.append(lane.upper())
        if status:
            where.append(f"{prefix}status = ?")
            params.append(status)
        if since:
            where.append(f"{prefix}date >= ?")
            params.append(since)
        return ((" AND " + " AND ".join(where)) if where else ""), params

    wsql, params = filters("")
    allowed = None
    if wsql:
        allowed = {r[0] for r in db.execute(f"SELECT id FROM entries WHERE 1=1{wsql}", params)}

    hits: dict[int, float] = {}
    # 1. exact id / section number
    qs = q.strip()
    if ID_QUERY.match(qs):
        num = re.sub(r"\D", "", qs) if re.match(r"^\u00a7?\s*\d+$", qs) else ""
        rows = (db.execute(f"SELECT id FROM entries WHERE number = ?{wsql}", [int(num), *params]).fetchall() if num
                else db.execute(f"SELECT id FROM entries WHERE key = ? COLLATE NOCASE{wsql}", [qs, *params]).fetchall())
        for r in rows:
            hits[r[0]] = 10.0  # exact matches outrank any fused score
    # 2. words
    fq = fts_query(qs)
    bm25 = {}
    if fq:
        ewsql, eparams = filters("e.")
        sql = (f"SELECT fts.rowid FROM fts JOIN entries e ON e.id = fts.rowid WHERE fts MATCH ?{ewsql}"
               " ORDER BY bm25(fts, 10.0, 8.0, 3.0, 1.0) LIMIT 300")
        try:
            bm25 = {r[0]: i for i, r in enumerate(db.execute(sql, [fq, *eparams]))}
        except sqlite3.OperationalError:
            bm25 = {}
    # 3. meaning
    sem = {}
    info["semantic"] = "off"
    if semantic and cache is not None:
        # Decide from the cache first: starting the server costs seconds, and is
        # wasted if the vectors it would be compared with are not there yet.
        emb = embedded_candidates(db, cache, allowed)
        if isinstance(emb, str):
            info["semantic"] = f"off ({emb})"
        elif ensure_server(root, quiet):
            qv = embed_query(qs)
            if qv:
                sem = semantic_ranks(*emb, qv)
                info["semantic"] = "on"
            else:
                info["semantic"] = "off (the embedding server did not answer)"
        else:
            info["semantic"] = "off (no embedding model on this machine)"
    for eid, rank in bm25.items():
        hits[eid] = hits.get(eid, 0.0) + 1.0 / (60 + rank)
    for eid, rank in sem.items():
        hits[eid] = hits.get(eid, 0.0) + 1.0 / (60 + rank)
    top = sorted(hits, key=lambda i: -hits[i])[:n]
    rows = []
    for eid in top:
        r = db.execute("SELECT * FROM entries WHERE id=?", (eid,)).fetchone()
        if r:
            rows.append(r)
    info["cards"] = {}
    if cache is not None and rows:
        hs = [r["text_hash"] for r in rows]
        q2 = f"SELECT text_hash, summary, status FROM cards WHERE text_hash IN ({','.join('?' * len(hs))})"
        info["cards"] = {h: (s, st) for h, s, st in cache.execute(q2, hs)}
    return rows, info


def _first_body_line(text: str) -> str:
    for ln in text.splitlines()[1:]:
        s = ln.strip()
        if s and not s.startswith(("---", "```", "**Status", "**Date", "**Lane", "**Decided")):
            return s
    return ""


def _snippet(text: str, q: str, width: int = 220) -> str:
    """The stretch of the body (not the heading, which is already shown) where the
    query's words first appear."""
    words = [w for w in re.findall(r"\w+", q.lower()) if len(w) > 2]
    nl = text.find("\n")
    body = text[nl + 1 :] if nl >= 0 else ""
    low = body.lower()
    pos = min((p for w in words if (p := low.find(w)) >= 0), default=-1)
    if pos < 0:
        return ""
    start = max(0, pos - width // 3)
    s = " ".join(body[start : start + width].split())
    return ("..." if start else "") + s + "..."


def main(argv: list[str] | None = None) -> int:
    # The entries are UTF-8 (arrows, em dashes, accented names), and Python on
    # Windows writes to a console or a redirect in cp1252: printing a title
    # with a `→` in it raised UnicodeEncodeError after the first hit or two.
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="backslashreplace")
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0],
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("query", nargs="*")
    ap.add_argument("--kind", action="append", choices=D.KINDS, help="repeatable")
    ap.add_argument("--lane", default="")
    ap.add_argument("--status", default="", help="e.g. open, closed, decided, done, in-progress")
    ap.add_argument("--since", default="", help="entries dated on or after YYYY-MM-DD")
    ap.add_argument("-n", type=int, default=8)
    ap.add_argument("--full", action="store_true", help="print the whole text of each hit")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--no-semantic", action="store_true")
    ap.add_argument("--rebuild", action="store_true", help="drop this checkout's index and rebuild it")
    ap.add_argument("--stats", action="store_true")
    ap.add_argument("--stop-server", action="store_true")
    ap.add_argument("--root", default=None, help="checkout to search (default: the one you are in)")
    args = ap.parse_args(argv)
    root = D.repo_root(Path(args.root) if args.root else None)
    if args.stop_server:
        print(stop_server(root))
        return 0
    if args.rebuild:
        db = open_index(root, rebuild=True)
        print(json.dumps(refresh(root, db, open_cache(root))))
        if not args.query:
            return 0
    if args.stats:
        db = open_index(root)
        info = refresh(root, db, open_cache(root))
        rows = db.execute("SELECT kind, status, COUNT(*) FROM entries GROUP BY kind, status ORDER BY kind, status")
        print(json.dumps(info))
        for k, s, c in rows:
            print(f"  {k:9s} {s:12s} {c:6d}")
        cache = open_cache(root)
        nc = cache.execute("SELECT COUNT(*) FROM cards").fetchone()[0]
        nv = cache.execute("SELECT COUNT(*) FROM vectors").fetchone()[0]
        print(f"  shared cache: {nc} cards, {nv} vectors ({cache_path(root)})")
        return 0
    q = " ".join(args.query).strip()
    if not q:
        ap.error("give a query (or --stats / --rebuild)")
    rows, info = search(root, q, kinds=args.kind or [], lane=args.lane, status=args.status, since=args.since,
                        n=args.n, semantic=not args.no_semantic, quiet=args.json)
    if args.json:
        print(json.dumps([{k: r[k] for k in r.keys() if k != "text"} | ({"text": r["text"]} if args.full else {})
                          | {"card": info["cards"].get(r["text_hash"], (None, None))[0]} for r in rows],
                         ensure_ascii=False, indent=1))
        return 0 if rows else 1
    if not rows:
        print(f"no entries match (semantic: {info['semantic']})")
        return 1
    for i, r in enumerate(rows, 1):
        tags = " | ".join(x for x in (r["kind"], r["lane"] and f"lane {r['lane']}", r["status"], r["date"]) if x)
        print(f"{i}. {r['path']}:{r['line']}  [{tags}]")
        print(f"   {r['title'][:200]}")
        card = info["cards"].get(r["text_hash"])
        if card and card[0]:
            print(f"   summary: {card[0]}")
        else:
            line = _first_body_line(r["text"])
            if line:
                print(f"   {line[:200]}")
        snip = _snippet(r["text"], q)
        if snip and not args.full:
            print(f"   ~ {snip}")
        if args.full:
            print()
            print(r["text"].rstrip())
            print()
    print(f"({len(rows)} shown; semantic: {info['semantic']}; index refresh {info['seconds']}s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
