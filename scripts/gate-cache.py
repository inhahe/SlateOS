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
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import gatecache_trace as gt  # noqa: E402

#: Bumped when the entry format changes; also part of every key.
FORMAT = 1
#: A file modified this close to the recording is always re-hashed.
RACY_NS = 2_000_000_000
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
    env["GATE_CACHE_INTERNAL_ENV"] = json.dumps(["PYTHONPATH"])
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


def merge(trace_dir: str) -> tuple[dict | None, list[str]]:
    """The union of every traced process's inputs, or None and the reasons."""
    why: list[str] = []
    procs = []
    records = {}
    try:
        names = sorted(os.listdir(trace_dir))
    except FileNotFoundError:
        # Something removed the temp directory while the checker ran (seen on
        # 2026-10-02, rq38: another session's sweep, most likely). The run is
        # simply untraced: not stored, and its verdict -- which the caller
        # already has -- stands. Raising here turned a passing gate into a
        # boot refused for "never reached a verdict".
        return None, ["the trace directory vanished before it was read -- "
                      "something else removed it"]
    for name in names:
        path = os.path.join(trace_dir, name)
        if name.endswith(".tmp"):
            why.append(f"a trace file was never finished: {name}")
        elif name.startswith("proc-"):
            with open(path, encoding="utf-8") as fh:
                procs.append(json.load(fh))
        elif name.startswith("git-"):
            with open(path, encoding="utf-8") as fh:
                records[gt.norm(path)] = json.load(fh)
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

    inputs: dict = {"reads": {}, "lists": {}, "stats": {}, "env": {},
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
    path = os.path.join(store_dir, key + ".json")
    tmp = path + f".{os.getpid()}.tmp"
    with open(tmp, "w", encoding="utf-8", newline="\n") as fh:
        json.dump(entry, fh)
    os.replace(tmp, path)


def note(text: str) -> None:
    sys.stderr.write(f"gate-cache: {text}\n")
    sys.stderr.flush()


def log(outcome: str, label: str, started: float) -> None:
    """One `<label>\\t<outcome>\\t<seconds>` row to GATE_CACHE_LOG, for the boot
    test's end-of-gates summary. `>>` of a short line is atomic."""
    path = os.environ.get("GATE_CACHE_LOG")
    if not path:
        return
    try:
        with open(path, "a", encoding="utf-8", newline="\n") as fh:
            fh.write(f"{label}\t{outcome}\t{time.monotonic() - started:.1f}\n")
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
            log("SKIP", label, started)
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
                 f"{len(inputs['stats'])} paths, {len(inputs['env'])} variables and "
                 f"{len(inputs['git_reads'])} git answers it read are unchanged.")
            log("HIT", label, started)
            return entry["rc"]

    started_ns = time.time_ns()
    # `ignore_cleanup_errors`: a directory already gone (see `merge`) must not
    # fail the gate on the way out either.
    with tempfile.TemporaryDirectory(prefix="gate-trace-", ignore_cleanup_errors=True) as trace_dir:
        rc, output = run_traced(command, trace_dir)
        inputs, why = merge(trace_dir)

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
        log("VERIFY-FAILED", label, started)
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
            log("TRACED-RUN-DIFFERED", label, started)
            return untraced
        note(f"{head}; exit {rc} (also untraced), not stored (only passes are).")
        log(outcome, label, started)
        return rc
    try:
        if inputs is None:
            note(f"{head}; not stored: " + "; ".join(why[:3])
                 + (f" (+{len(why) - 3} more)" if len(why) > 3 else ""))
            store(store_dir, key, {"format": FORMAT, "label": label, "argv": command,
                                   "utc_date": today(), "uncacheable": why,
                                   "scripts": script_digests(command)})
            log("UNCACHEABLE", label, started)
            return rc
        store(store_dir, key, {
            "format": FORMAT, "label": label, "argv": command, "cwd": cwd,
            "recorded": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"),
            "utc_date": today(), "recorded_ns": started_ns, "rc": rc,
            "output": base64.b64encode(output).decode("ascii"), "inputs": inputs,
        })
        note(f"{head}; stored: {len(inputs['reads'])} files, "
             f"{len(inputs['lists'])} listings, {len(inputs['stats'])} paths, "
             f"{len(inputs['env'])} variables, {len(inputs['git_reads'])} git answers.")
    except OSError as exc:
        note(f"{head}; could not store ({exc}).")
    log(outcome, label, started)
    return rc


if __name__ == "__main__":
    sys.exit(main())
