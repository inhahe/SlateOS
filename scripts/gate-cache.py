#!/usr/bin/env python3
"""Replay a gate's passing verdict when nothing it read has changed.

    python scripts/gate-cache.py [--label L] [--store DIR] [--force-miss] -- \\
        <python> <checker.py> [args...]

The boot test spends most of its time in gates (design-decisions 974: 133.9
gate-hours across 395 boots, 55% of it in gates that never refused), and most
gates read files that most changes do not touch. This runs a checker once with
`gatecache_trace.py` watching every Python process it starts, records exactly
what they read, and on the next call replays the stored output and exit status
instead of running it -- but only when every recorded input is what it was.
It is C-Q11 idea 2 (`requests/a-c-testing-without-a-full-boot-lane-a-takes-both.md`):
decide what a change needs by what each checker actually read. The rules and
their limits are design-decisions 979.

THE RULES THAT KEEP IT HONEST
-----------------------------
* Only passes are stored. A failing checker always runs again.
* A run whose inputs cannot all be seen is never stored: a non-Python child that
  is not read-only git, a shell command line, a socket, native code, a write
  outside the directories the run itself created, a child that was never
  traced, a directory left behind, or a walk over the whole environment. It is
  remembered for the rest of the UTC day, while its scripts are unchanged, and
  run untraced, so it does not pay for tracing that stores nothing.
* Every input is re-validated on every lookup, cheapest first; the first
  mismatch is a miss, and the miss says which input moved.
* An entry is valid only on the UTC day it was recorded, which bounds any
  verdict that depends on the date, and re-verifies every gate daily.
* A random fraction of lookups that would hit run fresh anyway
  (`GATE_CACHE_VERIFY_RATE`, default 0.1). If the fresh verdict differs, the
  cache would have lied: it says so loudly, deletes the entry, and writes
  `<store>/DISABLED`, which turns the cache off -- for every lane, since the
  store is shared -- until someone reads it, finds why, and deletes it. The
  fresh verdict stands.
* A hit says it is a hit, on the last line of the gate's output, so no reader
  mistakes a replay for a run.

The store is `<git-common-dir>/gate-cache`, shared by every worktree (its
entries are keyed by worktree path, so they do not collide), or
`GATE_CACHE_STORE` / `--store`.

Off switches: `GATE_CACHE=0`, a `DISABLED` file in the store, or not being
routed here at all (`run_checker` routes only when the boot test sets
`GATE_CACHE=1`; release and bench boots and `--no-gate-cache` do not).

Exit status: the checker's, on a hit or a run. `2` for a usage error of this
driver itself, before any checker ran.
"""

from __future__ import annotations

import argparse
import base64
import datetime as dt
import hashlib
import json
import os
import random
import re
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import gatecache_trace as gt  # noqa: E402

#: Bumped when the entry format changes; also part of every key. 2: imports'
#: lookups (`inputs["lookups"]`) in place of the listings the import system
#: takes.
FORMAT = 2
#: A file modified this close to the recording is always re-hashed.
RACY_NS = 2_000_000_000
#: The most of a reason a GATE_CACHE_LOG row keeps.
LOG_REASON_WIDTH = 300
#: The files whose behaviour an entry depends on besides its inputs.
TOOL_FILES = ("gate-cache.py", "gatecache_trace.py", "gatecache_tee.py",
              os.path.join("gatecache_site", "sitecustomize.py"))


def tool_digest() -> str:
    h = hashlib.sha256()
    for name in TOOL_FILES:
        with open(os.path.join(HERE, name), "rb") as fh:
            h.update(name.encode() + b"\0" + fh.read() + b"\0")
    return h.hexdigest()


def cache_key(argv: list[str], cwd: str) -> str:
    material = [FORMAT, tool_digest(), sys.version, gt.norm(sys.executable),
                gt.norm(cwd), argv]
    return hashlib.sha256(json.dumps(material).encode("utf-8", "surrogateescape")).hexdigest()


def default_store() -> str | None:
    out = subprocess.run(["git", "-C", HERE, "rev-parse", "--git-common-dir"],
                         capture_output=True, text=True)
    common = out.stdout.strip()
    if out.returncode != 0 or not common:
        return None
    return os.path.join(os.path.abspath(os.path.join(HERE, common)), "gate-cache")


def today() -> str:
    return dt.datetime.now(dt.timezone.utc).date().isoformat()


# --- git answers, recomputed at lookup ---------------------------------------
def _git_env(recorded: dict[str, str | None]) -> dict[str, str]:
    env = dict(os.environ)
    for name, value in recorded.items():
        if value is None:
            env.pop(name, None)
        else:
            env[name] = value
    env.pop("GATE_CACHE_TRACE_DIR", None)
    return env


def git_fingerprint(env: dict[str, str | None]) -> dict[str, str]:
    """What a git confined to a scratch repository still depends on: which git,
    and its global and system configuration."""
    e = _git_env(env)
    out = {"exe": shutil.which("git", path=e.get("PATH")) or ""}
    for name, cmd in (("version", ["git", "--version"]),
                      ("global", ["git", "config", "--global", "--list"]),
                      ("system", ["git", "config", "--system", "--list"])):
        p = subprocess.run(cmd, capture_output=True, env=e, cwd=tempfile.gettempdir())
        out[name] = hashlib.sha256(p.stdout + b"\0" + str(p.returncode).encode()).hexdigest()
    return out


def rerun_git(rec: dict, env: dict[str, str | None]) -> tuple[str, str, int]:
    stdin = base64.b64decode(rec.get("stdin") or "") if rec.get("piped") else None
    p = subprocess.run(rec["argv"], cwd=rec["cwd"], env=_git_env(env),
                       input=stdin,
                       stdin=None if stdin is not None else subprocess.DEVNULL,
                       capture_output=True)
    return (hashlib.sha256(p.stdout).hexdigest(), hashlib.sha256(p.stderr).hexdigest(),
            p.returncode)


# --- validation --------------------------------------------------------------
def validate(entry: dict) -> str | None:
    """None when every recorded input still holds, else the first that moved."""
    if entry.get("format") != FORMAT:
        return "the entry is from another format"
    if entry.get("utc_date") != today():
        return f"recorded on {entry.get('utc_date')}, and entries last one UTC day"
    inputs = entry["inputs"]
    for name, value in inputs["env"].items():
        if os.environ.get(name) != value:
            return f"environment variable {name} changed"
    for path, kinds in inputs["stats"].items():
        for kind, value in kinds.items():
            now = gt.stat_answer(path, kind)
            if json.loads(json.dumps(now)) != value:
                return f"{kind} of {path} changed"
    for path, digest in inputs["lists"].items():
        if gt.listing_digest(path) != digest:
            return f"the listing of {path} changed"
    for path, names in inputs["lookups"].items():
        entries = gt.module_entries(path)
        for name, digest in names.items():
            if gt.module_digest(entries, name) != digest:
                return f"what an import of {name} finds in {path} changed"
    # A file whose (size, mtime, id) is what it was when it was hashed is not
    # read again: git's index makes the same bet. The exception is git's too:
    # a file modified within RACY_NS of the recording could have changed again
    # inside the same clock tick, so its stamp proves nothing and it is hashed.
    # What this does not see is an edit that keeps the size and puts the old
    # mtime back (`touch -r`), which no tool here does (design-decisions 979).
    recorded_ns = entry.get("recorded_ns", 0)
    for path, (digest, stamp) in inputs["reads"].items():
        if (stamp is not None and stamp[1] < recorded_ns - RACY_NS
                and gt.file_stamp(path) == stamp):
            continue
        if gt.sha256_file(path) != digest:
            return f"{path} changed"
    for fp in inputs["git_fingerprints"]:
        if git_fingerprint(fp["env"]) != fp["value"]:
            return "git's version or global/system configuration changed"
    for rec in inputs["git_reads"]:
        out, err, rc = rerun_git(rec, rec["env"])
        if (out, err, rc) != (rec["stdout_sha"], rec["stderr_sha"], rec["rc"]):
            return f"git {' '.join(rec['argv'][1:3])} answers differently"
    return None


# --- recording ---------------------------------------------------------------
def run_traced(argv: list[str], trace_dir: str) -> tuple[int, bytes]:
    """Run the checker traced; relay its merged output live and keep a copy."""
    env = dict(os.environ)
    env["GATE_CACHE_TRACE_DIR"] = trace_dir
    env["GATE_CACHE_SPAWN"] = "root"
    # Bytecode is a cache of a source the tracer records as read whether or
    # not a .pyc answered the import (`_read_module_sources`); but writing
    # it is a write outside the run, which refused to store any gate
    # whose imported module had changed since its .pyc -- and the refusal is
    # remembered for the day, so the gate then ran untraced however often it
    # booted. Traced runs compile in memory instead; untraced ones keep the
    # bytecode cache warm.
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    # The interpreter the key names (`cache_key`): the tracer leaves only its
    # standard library out of the inputs (`gatecache_trace.is_installed_module`).
    env["GATE_CACHE_KEY_PYTHON"] = gt.INTERPRETER
    env["GATE_CACHE_INTERNAL_ENV"] = json.dumps(
        ["PYTHONPATH", "PYTHONDONTWRITEBYTECODE", "GATE_CACHE_KEY_PYTHON"])
    env["GATE_CACHE_INTERNAL_ROOTS"] = "[]"
    site = os.path.join(HERE, "gatecache_site")
    env["PYTHONPATH"] = site + (os.pathsep + env["PYTHONPATH"] if env.get("PYTHONPATH") else "")
    proc = subprocess.Popen(argv, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    kept = bytearray()
    out = sys.stdout.buffer
    while True:
        chunk = proc.stdout.read1(1 << 16)
        if not chunk:
            break
        kept.extend(chunk)
        out.write(chunk)
        out.flush()
    return proc.wait(), bytes(kept)


#: Begins a reason that is about the trace, not the checker: it was lost or
#: damaged on its way here. Such a run is not stored, and the checker is not
#: remembered as uncacheable for the day either -- the next run may trace fine.
TRANSIENT = "the trace was lost: "


def merge(trace_dir: str) -> tuple[dict | None, list[str]]:
    """The union of every traced process's inputs, or None and the reasons."""
    why: list[str] = []
    procs = []
    records = {}
    try:
        names = sorted(os.listdir(trace_dir))
    except FileNotFoundError:
        # Something removed the temp directory while the checker ran (another
        # session's sweep, most likely). The run is simply untraced: not
        # stored, and its verdict -- which the caller already has -- stands.
        return None, [TRANSIENT + "the trace directory vanished before it was read -- "
                      "something else removed it"]
    unreadable: list[str] = []
    for name in names:
        path = os.path.join(trace_dir, name)
        if name.endswith(".tmp"):
            why.append(f"a trace file was never finished: {name}")
            continue
        if not name.startswith(("proc-", "git-")):
            continue
        try:
            with open(path, encoding="utf-8") as fh:
                rec = json.load(fh)
        except (OSError, ValueError) as exc:
            unreadable.append(TRANSIENT + f"a trace file could not be read: {name} ({exc})")
            continue
        if name.startswith("proc-"):
            procs.append(rec)
        else:
            records[gt.norm(path)] = rec
    if unreadable:
        # Untraced, like a vanished directory. Returned before the checks
        # below, which would read the gap as the checker's doing ("its own
        # trace is missing") and so remember it as uncacheable.
        return None, unreadable
    spawned = {p["spawn"] for p in procs}
    if "root" not in spawned:
        why.append("the checker's own trace is missing (it exited before its exit handlers)")
    roots: set[str] = set()
    for p in procs:
        roots.update(p["roots"])
        why.extend(p["uncacheable"])
        for where in p["env_bulk"]:
            why.append(f"walks the whole environment at {where}")
        for d in p["made_dirs"]:
            if os.path.exists(d):
                why.append(f"left {d} behind, which a replay would not recreate")

    def inside(path: str) -> bool:
        return any(path == r or path.startswith(r.rstrip("\\/") + os.sep) for r in roots)

    inputs: dict = {"reads": {}, "lists": {}, "lookups": {}, "stats": {}, "env": {},
                    "git_fingerprints": [], "git_reads": []}
    for p in procs:
        for path, rec in p["reads"].items():
            if inside(path):
                continue
            seen = inputs["reads"].setdefault(path, rec)
            if seen[0] != rec[0]:
                why.append(f"{path} changed while the gate ran")
        for path, digest in p["lists"].items():
            if inside(path):
                continue
            if inputs["lists"].setdefault(path, digest) != digest:
                why.append(f"the listing of {path} changed while the gate ran")
        for path, names in p["lookups"].items():
            if inside(path):
                continue
            slot = inputs["lookups"].setdefault(path, {})
            for name, digest in names.items():
                if slot.setdefault(name, digest) != digest:
                    why.append(f"what an import of {name} finds in {path} "
                               "changed while the gate ran")
        for path, kinds in p["stats"].items():
            if inside(path):
                continue
            slot = inputs["stats"].setdefault(path, {})
            for kind, value in kinds.items():
                if slot.setdefault(kind, value) != value:
                    why.append(f"{kind} of {path} changed while the gate ran")
        for name, value in p["env"].items():
            if inputs["env"].setdefault(name, value) != value:
                why.append(f"environment variable {name} differed between processes")
        for s in p["spawns"]:
            if s["kind"] == "python" and s["id"] not in spawned:
                why.append(f"a Python child ran untraced: {' '.join(s['argv'][:3])}")
            elif s["kind"] == "git-internal":
                fp_env = s["env"]
                if all(fp["env"] != fp_env for fp in inputs["git_fingerprints"]):
                    inputs["git_fingerprints"].append({"env": fp_env, "value": None})
            elif s["kind"] == "git-read":
                rec = records.get(gt.norm(s["record"]))
                if rec is None:
                    why.append(f"git {' '.join(s['argv'][1:3])} left no record")
                elif rec.get("stdin_overflow"):
                    why.append("a git command read more stdin than is kept")
                else:
                    rec = dict(rec)
                    rec["env"] = s["env"]
                    inputs["git_reads"].append(rec)
    for fp in inputs["git_fingerprints"]:
        fp["value"] = git_fingerprint(fp["env"])
    return (None if why else inputs), why


def store(store_dir: str, key: str, entry: dict) -> None:
    os.makedirs(store_dir, exist_ok=True)
    gt.write_json(os.path.join(store_dir, key + ".json"), entry)


#: What a reason a gate ran is counted as: `(pattern, kind)`, the first whose
#: pattern matches the start of the reason winning, with `\N` in the kind
#: taken from the pattern's group N. A kind keeps the words that say what to
#: fix -- which variable, which program, which git command -- and never a
#: path, a date or a value, so that the same cause reads the same on every
#: boot and the boot test can add them up.
REASON_KINDS: tuple[tuple[str, str], ...] = (
    # Why a lookup found no usable entry (`validate`, `main`).
    (r"no entry$", "no entry"),
    (r"the entry is from another format$", "an entry of another format"),
    (r"recorded on .*, and entries last one UTC day$", "recorded on another UTC day"),
    (r"an earlier refusal to cache has expired$", "an earlier refusal expired"),
    (r"unreadable entry", "an unreadable entry"),
    (r"environment variable (\S+) (?:changed|differed)", r"environment variable \1"),
    (r"the listing of .* changed while the gate ran$", "an input changed while the gate ran"),
    (r"what an import of .* changed while the gate ran$", "an input changed while the gate ran"),
    (r"the listing of .* changed$", "a directory listing changed"),
    (r"what an import of \S+ finds in .* changed$", "what an import finds changed"),
    (r"git's version or global/system configuration changed$", "git's configuration changed"),
    (r"git (\S+(?: \S+)?) answers differently$", r"git \1 answers differently"),
    (r".* changed while the gate ran$", "an input changed while the gate ran"),
    (r"(exists|lexists|isfile|isdir|islink|full|lfull|access:\d+) of .* changed$",
     r"\1 of a path changed"),
    (r".* changed$", "a file it read changed"),
    # Why a run could not be stored (`merge`, the tracer's refusals).
    (r"runs '([^']+?)(?:\.exe)?'", r"runs \1"),
    (r'runs "([^"]+?)(?:\.exe)?"', r"runs \1"),
    (r"a shell command line", "a shell command line"),
    (r"writes .*, outside the directories the run created$", "writes outside its own directories"),
    (r"(\S+) on .*, outside the directories the run created$", r"\1 outside its own directories"),
    (r"(\S+): an input the tracer cannot see$", r"\1: an input the tracer cannot see"),
    (r"tracer fault on (\S+?):", r"a tracer fault on \1"),
    (r"git (\S+) against a repository the run did not create$",
     r"git \1 on a repository the run did not create"),
    (r"git (\S+) reads an inherited stdin", r"git \1 reads an inherited stdin"),
    (r"git .* left no record$", "a git command left no record"),
    (r"a git command read more stdin", "a git command read more stdin than is kept"),
    (r"a Python child ran untraced", "a Python child ran untraced"),
    (r"walks the whole environment", "walks the whole environment"),
    (r"left .* behind", "left a directory behind"),
    (re.escape(gt.SHADOWED), "a file would shadow a module the tracer loaded"),
    (r"a trace file was never finished", "a trace file was never finished"),
    (r"a trace file could not be read", "a trace file could not be read"),
    (r"the checker's own trace is missing", "the checker's own trace is missing"),
    (r"the trace directory vanished", "the trace directory vanished"),
    (r"merging the trace failed", "merging the trace failed"),
    (r"traced exit -?\d+, untraced -?\d+$", "traced and untraced runs disagreed"),
    (r"cached exit -?\d+, fresh exit -?\d+$", "a verification disagreed"),
)


def reason_kind(reason: str) -> str:
    """The kind `reason` is counted under (`REASON_KINDS`); "other" when no
    pattern knows it, the reason itself still in the log beside it. A lost
    trace's reason is counted by what was lost, past its `TRANSIENT` mark."""
    reason = reason.removeprefix(TRANSIENT)
    for pattern, kind in REASON_KINDS:
        m = re.match(pattern, reason)
        if m:
            return m.expand(kind)
    return "other"


def note(text: str) -> None:
    sys.stderr.write(f"gate-cache: {text}\n")
    sys.stderr.flush()


def log(outcome: str, label: str, started: float, reason: str = "") -> None:
    """One `<label>\\t<outcome>\\t<seconds>\\t<kind>\\t<reason>` row to
    GATE_CACHE_LOG, for the boot test's end-of-gates summary and the boot's
    history row. `reason` says why the gate ran rather than replayed (a
    miss's, a refusal's) and `kind` is what it is counted as (`reason_kind`);
    both are empty for a run that needs no reason. `>>` of a short line is
    atomic."""
    path = os.environ.get("GATE_CACHE_LOG")
    if not path:
        return
    # Classified whole: a long path cut short would lose the tail the
    # patterns anchor on, and count as "other".
    whole = " ".join(reason.split())
    detail = whole[:LOG_REASON_WIDTH]
    kind = reason_kind(whole) if whole else ""
    try:
        with open(path, "a", encoding="utf-8", newline="\n") as fh:
            fh.write(f"{label}\t{outcome}\t{time.monotonic() - started:.1f}\t{kind}\t{detail}\n")
    except OSError:
        pass


def script_digests(command: list[str]) -> dict[str, str | None]:
    """The scripts a command names, by content: what a remembered refusal to
    cache is valid for."""
    return {gt.norm(a) or a: gt.sha256_file(a) for a in command[1:]
            if a.endswith(".py") and os.path.isfile(a)}


def main(argv: list[str] | None = None) -> int:
    started = time.monotonic()
    args = sys.argv[1:] if argv is None else argv
    if "--" not in args:
        print(__doc__.split("\n\n")[0], file=sys.stderr)
        return 2
    cut = args.index("--")
    ap = argparse.ArgumentParser(prog="gate-cache.py")
    ap.add_argument("--label", default="")
    ap.add_argument("--store")
    ap.add_argument("--force-miss", action="store_true")
    opts = ap.parse_args(args[:cut])
    command = args[cut + 1:]
    if not command:
        print("gate-cache.py: no command after --", file=sys.stderr)
        return 2
    label = opts.label or os.path.basename(command[1] if len(command) > 1 else command[0])

    store_dir = opts.store or os.environ.get("GATE_CACHE_STORE") or default_store()
    if os.environ.get("GATE_CACHE", "1") == "0" or store_dir is None or \
            os.path.exists(os.path.join(store_dir, "DISABLED")):
        rc = subprocess.run(command).returncode
        log("OFF", label, started)
        return rc

    cwd = os.getcwd()
    key = cache_key(command, cwd)
    path = os.path.join(store_dir, key + ".json")

    entry = None
    reason = "no entry"
    try:
        with open(path, encoding="utf-8") as fh:
            entry = json.load(fh)
    except FileNotFoundError:
        entry = None
    except (OSError, ValueError) as exc:
        entry, reason = None, f"unreadable entry ({type(exc).__name__})"

    # A run found uncacheable today, whose scripts have not changed since, is
    # run untraced: tracing it again would cost the tracing and store nothing.
    if entry is not None and entry.get("uncacheable"):
        if entry.get("utc_date") == today() and entry.get("scripts") == script_digests(command):
            rc = subprocess.run(command).returncode
            note(f"SKIP {label} -- found uncacheable earlier today "
                 f"({entry['uncacheable'][0]}); run untraced.")
            log("SKIP", label, started, entry["uncacheable"][0])
            return rc
        entry, reason = None, "an earlier refusal to cache has expired"

    if entry is not None:
        try:
            reason = validate(entry) or ""
        except (OSError, ValueError, KeyError, TypeError) as exc:
            reason = f"unreadable entry ({type(exc).__name__})"

    verify = False
    if entry is not None and not reason:
        rate = float(os.environ.get("GATE_CACHE_VERIFY_RATE", "0.1") or 0)
        verify = opts.force_miss or random.random() < rate
        if not verify:
            sys.stdout.buffer.write(base64.b64decode(entry["output"]))
            sys.stdout.buffer.flush()
            inputs = entry["inputs"]
            note(f"HIT {label} -- replayed the passing run recorded {entry['recorded']}; "
                 f"all {len(inputs['reads'])} files, {len(inputs['lists'])} listings, "
                 f"{sum(map(len, inputs['lookups'].values()))} imports, "
                 f"{len(inputs['stats'])} paths, {len(inputs['env'])} variables and "
                 f"{len(inputs['git_reads'])} git answers it read are unchanged.")
            log("HIT", label, started)
            return entry["rc"]

    started_ns = time.time_ns()
    # `ignore_cleanup_errors`: a directory already gone (see `merge`) must not
    # fail the gate on the way out either.
    with tempfile.TemporaryDirectory(prefix="gate-trace-", ignore_cleanup_errors=True) as trace_dir:
        rc, output = run_traced(command, trace_dir)
        # The checker has its verdict; nothing merge() trips on may change it.
        # A record of a shape it did not expect, or a git it cannot start, is
        # the trace's fault, so the run is merely untraced.
        try:
            inputs, why = merge(trace_dir)
        except Exception as exc:  # noqa: BLE001 -- see above
            inputs, why = None, [TRANSIENT + f"merging the trace failed: {exc!r}"]

    if verify and rc != entry["rc"]:
        note(f"VERIFY FAILED for {label}: the cache would have replayed exit "
             f"{entry['rc']}, and a fresh run exits {rc}. The cache is wrong. It is off, "
             f"for every lane, until someone reads {store_dir}/DISABLED, finds why, and "
             f"deletes it; this entry is deleted. The fresh verdict stands.")
        try:
            os.remove(path)
            with open(os.path.join(store_dir, "DISABLED"), "w", encoding="utf-8",
                      newline="\n") as fh:
                fh.write(f"verify failed for {label} ({' '.join(command)}) at "
                         f"{dt.datetime.now().isoformat()}: cached exit {entry['rc']}, "
                         f"fresh exit {rc}\n")
        except OSError:
            pass
        log("VERIFY-FAILED", label, started, f"cached exit {entry['rc']}, fresh exit {rc}")
        return rc
    head = (f"VERIFIED {label} -- a fresh run agreed with the cached verdict (exit {rc})"
            if verify else f"MISS {label} ({reason})")
    outcome = "VERIFIED" if verify else "MISS"

    if rc != 0:
        # The tracer must never be why a gate fails. A traced run that failed
        # is run once more untraced, and the untraced verdict is the gate's.
        # If they disagree, either the tracer broke the checker or the checker
        # is flaky; the cache cannot tell which, so it says both, remembers the
        # checker as uncacheable, and the boot summary counts it.
        retry = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        untraced = retry.returncode
        if untraced != rc:
            # The verdict that stands is the untraced one, so its output is
            # the one shown, after the traced run's for comparison.
            sys.stdout.write("\n--- gate-cache: the untraced re-run's output ---\n")
            sys.stdout.flush()
            sys.stdout.buffer.write(retry.stdout)
            sys.stdout.buffer.flush()
            note(f"TRACED-RUN-DIFFERED {label}: exit {rc} traced, {untraced} untraced. "
                 f"Either the tracer changed what the checker did, or the checker is "
                 f"flaky; the untraced verdict stands, and the checker is not cached "
                 f"today.")
            try:
                store(store_dir, key, {"format": FORMAT, "label": label, "argv": command,
                                       "utc_date": today(),
                                       "uncacheable": [f"traced exit {rc}, untraced {untraced}"],
                                       "scripts": script_digests(command)})
            except OSError:
                pass
            log("TRACED-RUN-DIFFERED", label, started, f"traced exit {rc}, untraced {untraced}")
            return untraced
        note(f"{head}; exit {rc} (also untraced), not stored (only passes are).")
        log(outcome, label, started, "" if verify else reason)
        return rc
    try:
        if inputs is None:
            verdict = rc
            rerun = ""
            if any(w.startswith(gt.SHADOWED) for w in why):
                # The traced run used a module the untraced one would not:
                # its pass is not the gate's verdict. The untraced run's is.
                retry = subprocess.run(command, stdout=subprocess.PIPE,
                                       stderr=subprocess.STDOUT)
                verdict = retry.returncode
                if verdict != rc:
                    sys.stdout.write("\n--- gate-cache: the untraced re-run's output ---\n")
                    sys.stdout.flush()
                    sys.stdout.buffer.write(retry.stdout)
                    sys.stdout.buffer.flush()
                rerun = f"; run again untraced, whose exit {verdict} stands"
            note(f"{head}; not stored: " + "; ".join(why[:3])
                 + (f" (+{len(why) - 3} more)" if len(why) > 3 else "") + rerun)
            # A lost trace says nothing about the gate: the next run tries again.
            if not all(w.startswith(TRANSIENT) for w in why):
                store(store_dir, key, {"format": FORMAT, "label": label, "argv": command,
                                       "utc_date": today(), "uncacheable": why,
                                       "scripts": script_digests(command)})
            log("UNCACHEABLE", label, started, why[0] if why else "")
            return verdict
        store(store_dir, key, {
            "format": FORMAT, "label": label, "argv": command, "cwd": cwd,
            "recorded": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"),
            "utc_date": today(), "recorded_ns": started_ns, "rc": rc,
            "output": base64.b64encode(output).decode("ascii"), "inputs": inputs,
        })
        note(f"{head}; stored: {len(inputs['reads'])} files, "
             f"{len(inputs['lists'])} listings, "
             f"{sum(map(len, inputs['lookups'].values()))} imports, "
             f"{len(inputs['stats'])} paths, "
             f"{len(inputs['env'])} variables, {len(inputs['git_reads'])} git answers.")
    except OSError as exc:
        note(f"{head}; could not store ({exc}).")
    log(outcome, label, started, "" if verify else reason)
    return rc


if __name__ == "__main__":
    sys.exit(main())
