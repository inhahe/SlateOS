#!/usr/bin/env python3
"""Tests for the gate cache: `gate-cache.py`, `gatecache_trace.py`, `gatecache_tee.py`.

Run: `python scripts/test-gate-cache.py` (0 = pass, 1 = fail).

The cache's one unforgivable failure is a hit it should not have had: a gate
that would have refused, replayed as a pass, with nothing downstream to notice.
So most of these cases change one input of one kind and assert the next lookup
MISSES and names that input -- one case per kind of input the tracer records --
and the rest assert that each kind of run the cache must not store is not
stored. The happy path (a hit, byte-exact) is here too, but it is the least
interesting thing tested.

Every case runs the real driver as a subprocess, against fixture checkers and
fixture git repositories in a scratch directory, with a scratch cache store.
Nothing here touches this repository or its real store.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import textwrap

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import gitenv  # noqa: E402

DRIVER = os.path.join(HERE, "gate-cache.py")
failures: list[str] = []


def check(label: str, got: object, want: object) -> None:
    if got == want:
        print(f"PASS  {label}")
    else:
        print(f"FAIL  {label}\n        got : {got!r}\n        want: {want!r}", file=sys.stderr)
        failures.append(label)


class Fixture:
    def __init__(self) -> None:
        self.root = tempfile.mkdtemp(prefix="gate-cache-test-")
        self.store = os.path.join(self.root, "store")
        self.n = 0

    def path(self, *parts: str) -> str:
        return os.path.join(self.root, *parts)

    def write(self, rel: str, text: str) -> str:
        p = self.path(rel)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w", encoding="utf-8", newline="\n") as fh:
            fh.write(text)
        return p

    def checker(self, body: str) -> str:
        self.n += 1
        return self.write(f"checker{self.n}.py", textwrap.dedent(body))

    def run(self, script: str, *args: str, env: dict[str, str] | None = None,
            verify: str = "0") -> tuple[int, bytes, str]:
        e = dict(os.environ if env is None else env)
        e["GATE_CACHE_VERIFY_RATE"] = verify
        e.pop("GATE_CACHE", None)
        p = subprocess.run([sys.executable, DRIVER, "--store", self.store, "--label", "t",
                            "--", sys.executable, script, *args],
                           capture_output=True, env=e, cwd=self.root)
        notes = [ln for ln in p.stderr.decode("utf-8", "replace").splitlines()
                 if ln.startswith("gate-cache:")]
        return p.returncode, p.stdout, notes[-1] if notes else ""

    def cleanup(self) -> None:
        gitenv.remove_tree(self.root)


def git(repo: str, *args: str) -> str:
    return subprocess.run(["git", *args], cwd=repo, env=gitenv.clean_env(),
                          capture_output=True, text=True, check=True).stdout.strip()


def make_repo(path: str) -> None:
    os.makedirs(path, exist_ok=True)
    git(path, "init", "-q")
    git(path, "config", "user.name", "gate-cache fixture")
    git(path, "config", "user.email", "fixture@invalid")
    with open(os.path.join(path, "a.txt"), "w", encoding="utf-8", newline="\n") as fh:
        fh.write("a\n")
    git(path, "add", "a.txt")
    git(path, "commit", "-q", "-m", "one")


def kind(note: str) -> str:
    return note.split()[1] if note.startswith("gate-cache:") else note


def vanished_trace() -> None:
    """A trace directory something else removed mid-run leaves the run
    untraced -- not stored, its verdict standing -- rather than raising. It
    raised on 2026-10-02 (rq38), and a gate that had passed was reported as
    never reaching a verdict, refusing the boot."""
    import srcload
    gc = srcload.load(DRIVER, "gate_cache")
    gone = os.path.join(tempfile.gettempdir(), "gate-trace-never-made-by-this-test")
    inputs, why = gc.merge(gone)
    check("a vanished trace directory is an untraced run, not an error",
          (inputs, any("vanished" in w for w in why)), (None, True))
    check("...and a lost trace, not the checker's doing",
          all(w.startswith(gc.TRANSIENT) for w in why), True)


def unreadable_trace() -> None:
    """A trace file that does not parse -- two writers' output in one file,
    say -- leaves the run untraced, as a vanished directory does, rather than
    raising and turning the gate's verdict into a refused boot. Even when it
    is the checker's own record, the only reason given is the lost trace."""
    import srcload
    gc = srcload.load(DRIVER, "gate_cache")
    d = tempfile.mkdtemp(prefix="gate-trace-unreadable-")
    try:
        for name in ("git-root.1.json", "proc-1-ab.json"):
            with open(os.path.join(d, name), "w", encoding="utf-8", newline="\n") as fh:
                fh.write('{"rc": 0}{"rc": 0}')
        inputs, why = gc.merge(d)
        check("an unreadable trace file is an untraced run, not an error",
              (inputs, any("could not be read: git-root.1.json" in w for w in why)),
              (None, True))
        check("...a lost trace, and nothing else: not 'its own trace is missing'",
              (len(why), all(w.startswith(gc.TRANSIENT) for w in why)), (2, True))
    finally:
        shutil.rmtree(d, ignore_errors=True)


def _tracer_module():
    """gatecache_trace, loaded under a name of its own. Under the boot test
    this suite is itself traced, and `sys.modules["gatecache_trace"]` is the
    live tracer that gitenv consults; loading over it would unhook it."""
    import srcload
    return srcload.load(os.path.join(HERE, "gatecache_trace.py"), "gatecache_trace_under_test")


def ids_are_unique_across_processes() -> None:
    """Two traced processes that share one inherited spawn id -- each reached
    through an untraced intermediary -- must still name their children and
    their git records apart. Alike, two records shared one file: one's answer
    replaced the other's, or both landed in it and the trace would not parse."""
    gt = _tracer_module()
    d = tempfile.mkdtemp(prefix="gate-trace-ids-")
    # Outside the trace directory, which `_classify` treats as the run's own:
    # a git command there is internal and gets no record at all.
    elsewhere = tempfile.mkdtemp(prefix="gate-trace-ids-repo-")
    try:
        repo = os.path.join(elsewhere, "some-repo")
        a, b = gt._Tracer(d, "root.7"), gt._Tracer(d, "root.7")

        def git_record(t) -> str:
            args, _ = gt._classify(t, ["git", "-C", repo, "rev-parse", "HEAD"], (), {},
                                   "site", "tee.py")
            return args[2]

        def python_child(t) -> str:
            _, k = gt._classify(t, [sys.executable, "-c", "pass"], (), {}, "site", "tee.py")
            return k["env"]["GATE_CACHE_SPAWN"]

        ra, rb = git_record(a), git_record(b)
        check("two processes with one inherited spawn id name their git records apart",
              (os.path.basename(ra).startswith("git-root.7."), ra != rb), (True, True))
        check("...and their Python children apart",
              python_child(a) != python_child(b), True)
        check("one process still numbers its own children apart",
              python_child(a) != python_child(a), True)
    finally:
        shutil.rmtree(d, ignore_errors=True)
        shutil.rmtree(elsewhere, ignore_errors=True)


def descendants_cannot_claim_a_listed_id() -> None:
    """A Python process reached through something untraced inherits whatever
    GATE_CACHE_SPAWN its traced ancestor's environment holds. That must not be
    an id the ancestor's parent listed -- the root's, above all -- or the
    descendant's record would stand in for a listed process whose own record
    is missing, and the run be stored without that process's inputs."""
    d = tempfile.mkdtemp(prefix="gate-trace-spawn-")
    try:
        env = {k: v for k, v in os.environ.items() if not k.startswith("GATE_CACHE_")}
        env.update(GATE_CACHE_TRACE_DIR=d, GATE_CACHE_SPAWN="root",
                   GATE_CACHE_INTERNAL_ENV='["PYTHONPATH"]', GATE_CACHE_INTERNAL_ROOTS="[]")
        site = os.path.join(HERE, "gatecache_site")
        env["PYTHONPATH"] = site + (os.pathsep + env["PYTHONPATH"] if env.get("PYTHONPATH") else "")
        p = subprocess.run([sys.executable, "-c",
                            "import os; print(os.environ['GATE_CACHE_SPAWN'])"],
                           capture_output=True, text=True, encoding="utf-8", env=env)
        handed_down = p.stdout.strip()
        claimed = []
        for name in os.listdir(d):
            if name.startswith("proc-") and name.endswith(".json"):
                with open(os.path.join(d, name), encoding="utf-8") as fh:
                    claimed.append(json.load(fh)["spawn"])
        check("the traced process still claims the id it was given", claimed, ["root"])
        check("...and hands its untraced descendants one nobody listed",
              (handed_down.startswith("root~"), handed_down != "root"), (True, True))
    finally:
        shutil.rmtree(d, ignore_errors=True)


def main() -> int:
    # The checkers these cases trace run `git -C <scratch repo>` with the
    # environment they inherit, and under a git hook that environment binds a
    # repository: git exports GIT_DIR to its hooks, and GIT_DIR outranks -C.
    # So under the pre-push hook every checker's git read the REAL repository
    # -- the same answer before and after a case changed the scratch one --
    # and "the repository moved, so git answers differently" got a HIT and
    # failed a push (2026-10-01). Run by hand it passed, because nothing was
    # bound. Scrubbing here is what the other git-touching suites do.
    removed = gitenv.scrub_environ()
    if removed:
        print(f"(ignoring the repository this suite was started in: {', '.join(removed)})")
    # The same for the boot test's outcome log. This suite runs inside a boot's
    # tooling phase, and every driver it starts appended its fixture outcomes
    # -- label "t", one of them a deliberate VERIFY-FAILED -- to the log that
    # boot summarises. rq43's summary said a verification had failed and the
    # cache was off, when neither was true (2026-10-02). The one case that
    # wants a log passes its own.
    if os.environ.pop("GATE_CACHE_LOG", None) is not None:
        print("(ignoring the outcome log of the boot this suite runs in)")
    fx = Fixture()
    try:
        cases(fx)
    finally:
        fx.cleanup()
    vanished_trace()
    unreadable_trace()
    ids_are_unique_across_processes()
    descendants_cannot_claim_a_listed_id()
    print()
    if failures:
        print(f"{len(failures)} FAILED: {', '.join(failures)}")
        return 1
    print("all gate-cache tests passed")
    return 0


def cases(fx: Fixture) -> None:
    data = fx.write("data/input.txt", "hello\n")
    ck = fx.checker(f"""
        import sys
        text = open({data!r}, encoding="utf-8").read()
        print("read:", text.strip())
        sys.exit(0 if "hello" in text else 1)
    """)

    # --- the happy path ---------------------------------------------------
    rc, out, note = fx.run(ck)
    check("a first run is a miss, and a pass is stored", (rc, kind(note), "stored" in note),
          (0, "MISS", True))
    rc, out2, note = fx.run(ck)
    check("an unchanged second run is a hit", (rc, kind(note)), (0, "HIT"))
    check("...and its output is replayed exactly", out2, out)

    # --- one changed input of each kind is a miss, naming it ---------------
    fx.write("data/input.txt", "hello, world\n")
    rc, _, note = fx.run(ck)
    check("a file it read changed: miss", kind(note), "MISS")
    check("...and the miss names the file", "input.txt changed" in note, True)

    fx.write("data/input.txt", "goodbye\n")
    rc, _, note = fx.run(ck)
    check("a failing run is never stored", (rc, "not stored" in note), (1, True))
    rc, _, note = fx.run(ck)
    check("...so the next run is not a hit either", (rc, kind(note)), (1, "MISS"))
    fx.write("data/input.txt", "hello\n")

    lister = fx.checker(f"""
        import os
        print(sorted(os.listdir({fx.path('data')!r})))
    """)
    fx.run(lister)
    fx.write("data/new.txt", "x\n")
    _, _, note = fx.run(lister)
    check("a directory it listed gained an entry: miss",
          (kind(note), "listing of" in note), ("MISS", True))

    probe = fx.path("data", "maybe.txt")
    asker = fx.checker(f"""
        import os
        print("exists" if os.path.exists({probe!r}) else "absent")
    """)
    fx.run(asker)
    rc, _, note = fx.run(asker)
    check("an unchanged exists() answer hits", kind(note), "HIT")
    fx.write("data/maybe.txt", "now it does\n")
    _, _, note = fx.run(asker)
    check("an exists() answer changed: miss", (kind(note), "exists of" in note), ("MISS", True))

    envck = fx.checker("""
        import os
        print("mode:", os.environ.get("GATE_TEST_MODE", "unset"))
    """)
    env = dict(os.environ, GATE_TEST_MODE="one")
    fx.run(envck, env=env)
    _, _, note = fx.run(envck, env=env)
    check("an unchanged environment variable hits", kind(note), "HIT")
    _, _, note = fx.run(envck, env=dict(os.environ, GATE_TEST_MODE="two"))
    check("an environment variable it read changed: miss",
          (kind(note), "GATE_TEST_MODE" in note), ("MISS", True))

    setter = fx.checker("""
        import os
        os.environ["GATE_TEST_OWN"] = "set by the run"
        print(os.environ["GATE_TEST_OWN"])
    """)
    fx.run(setter, env=dict(os.environ, GATE_TEST_OWN="outer"))
    _, _, note = fx.run(setter, env=dict(os.environ, GATE_TEST_OWN="different outer"))
    check("a variable the run set itself is not an input", kind(note), "HIT")

    # --- children ------------------------------------------------------------
    child_data = fx.write("data/child.txt", "child\n")
    child = fx.write("child.py", f"print(open({child_data!r}).read().strip())\n")
    parent = fx.checker(f"""
        import subprocess, sys
        subprocess.run([sys.executable, {child!r}], check=True)
    """)
    fx.run(parent)
    _, _, note = fx.run(parent)
    check("a Python child's run hits when nothing changed", kind(note), "HIT")
    fx.write("data/child.txt", "child, changed\n")
    _, _, note = fx.run(parent)
    check("a file only the child read changed: miss",
          (kind(note), "child.txt changed" in note), ("MISS", True))

    untraced = fx.checker(f"""
        import subprocess, sys
        subprocess.run([sys.executable, "-I", {child!r}], check=True)
    """)
    _, _, note = fx.run(untraced)
    check("a Python child started with -I (untraced) is not stored",
          ("not stored" in note, "ran untraced" in note), (True, True))

    shell = fx.checker("""
        import subprocess
        subprocess.run("echo hi", shell=True, check=True)
    """)
    _, _, note = fx.run(shell)
    check("a shell command line is not stored", ("not stored" in note, "shell" in note),
          (True, True))

    other = fx.checker("""
        import subprocess, shutil
        subprocess.run([shutil.which("where") or shutil.which("which") or "hostname", "python"],
                       capture_output=True)
    """)
    _, _, note = fx.run(other)
    check("another executable is not stored", ("not stored" in note, "cannot be seen" in note),
          (True, True))

    # --- writes --------------------------------------------------------------
    outside = fx.path("data", "written.txt")
    writer = fx.checker(f"""
        open({outside!r}, "w").write("x")
    """)
    _, _, note = fx.run(writer)
    check("a write outside the run's own directories is not stored",
          ("not stored" in note, "written.txt" in note), (True, True))

    scratch = fx.checker("""
        import os, tempfile
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "f.txt")
            open(p, "w").write("x")
            print(open(p).read())
    """)
    fx.run(scratch)
    _, _, note = fx.run(scratch)
    check("writes and reads inside a run-created temp dir are the run's own: hit",
          kind(note), "HIT")

    made = fx.path("made-by-run")
    leaver = fx.checker(f"""
        import os
        os.mkdir({made!r})
        open(os.path.join({made!r}, "f"), "w").write("x")
    """)
    _, _, note = fx.run(leaver)
    check("a directory the run made and left behind is not stored",
          ("not stored" in note, "left" in note), (True, True))
    gitenv.remove_tree(made)

    # --- the tracer is never why a gate fails -----------------------------------
    # A real difference the tracer makes: its DirEntry stand-in (which is how
    # it sees `entry.stat()`) is not an `os.DirEntry`, and cannot be one.
    fussy = fx.checker(f"""
        import os, sys
        with os.scandir({fx.path('data')!r}) as it:
            entry = next(it)
        sys.exit(0 if isinstance(entry, os.DirEntry) else 1)
    """)
    rc, out, note = fx.run(fussy)
    check("a checker the tracer breaks is re-run untraced, and that verdict stands",
          (rc, kind(note)), (0, "TRACED-RUN-DIFFERED"))
    check("...showing the untraced run's output", b"untraced re-run's output" in out, True)
    _, _, note = fx.run(fussy)
    check("...and it is not cached, so it runs untraced from then on", kind(note), "SKIP")

    # --- a refusal to cache is remembered, and runs untraced ------------------
    _, _, note = fx.run(writer)
    check("an uncacheable run is remembered for the day and run untraced",
          (kind(note), "found uncacheable earlier today" in note), ("SKIP", True))
    with open(writer, "a", encoding="utf-8", newline="\n") as fh:
        fh.write("# edited\n")
    _, _, note = fx.run(writer)
    check("...until its script changes, when it is traced again",
          ("not stored" in note, "refusal to cache has expired" in note), (True, True))

    # --- the invocation log ---------------------------------------------------------
    logf = fx.path("gate-cache.log")
    env = dict(os.environ, GATE_CACHE_LOG=logf)
    fx.run(scratch, env=env)  # its entry is current, so this is a hit
    fx.run(writer, env=env)
    with open(logf, encoding="utf-8") as fh:
        rows = [ln.rstrip("\n").split("\t") for ln in fh]
    check("each invocation logs its label and outcome",
          [r[:2] for r in rows], [["t", "HIT"], ["t", "SKIP"]])
    check("...and how long it took", all(float(r[2]) >= 0 for r in rows), True)

    # --- run_checker routes a Python gate through the cache, and nothing else ----
    import msysbash
    rcsh = os.path.join(HERE, "run-checker.sh").replace("\\", "/")
    gate = fx.checker("""
        print("gate output")
    """).replace("\\", "/")

    def via_run_checker(cache: str) -> str:
        script = (f'. "{rcsh}"\nCHECKER_LOGDIR="{fx.root}"\n'
                  f'run_checker t-gate "{sys.executable.replace(chr(92), "/")}" "{gate}"\n'
                  'echo "rc=$?"\n')
        env = dict(os.environ, GATE_CACHE=cache, GATE_CACHE_VERIFY_RATE="0",
                   GATE_CACHE_DRIVER=DRIVER, GATE_CACHE_STORE=fx.store)
        env.pop("GATE_CACHE_LOG", None)
        p = subprocess.run([msysbash.bash(), "-c", script], capture_output=True, text=True,
                           env=env, cwd=fx.root)
        return p.stdout + p.stderr

    first = via_run_checker("1")
    second = via_run_checker("1")
    check("run_checker routes a Python gate through the cache when it is on",
          ("gate output" in first, "gate-cache: MISS" in first), (True, True))
    check("...and the second call is a hit, still passing",
          ("gate-cache: HIT" in second, "rc=0" in second), (True, True))
    off = via_run_checker("0")
    check("with the cache off, run_checker runs the gate as before",
          ("gate output" in off, "gate-cache:" in off, "rc=0" in off), (True, False, True))

    # --- the environment as a whole -----------------------------------------
    walker = fx.checker("""
        import os
        print(sum(1 for _ in os.environ))
    """)
    _, _, note = fx.run(walker)
    check("walking the whole environment is not stored",
          ("not stored" in note, "whole environment" in note), (True, True))

    copier = fx.checker(f"""
        import subprocess, sys
        sys.path.insert(0, {HERE!r})
        import gitenv
        env = gitenv.clean_env()
        subprocess.run([sys.executable, {child!r}], env=env, check=True)
    """)
    fx.run(copier)
    _, _, note = fx.run(copier)
    check("gitenv.clean_env's copy for a child is not a whole-environment read: hit",
          kind(note), "HIT")

    # --- git ------------------------------------------------------------------
    repo = fx.path("repo")
    make_repo(repo)
    gitck = fx.checker(f"""
        import subprocess
        out = subprocess.run(["git", "-C", {repo!r}, "log", "--format=%s"],
                             capture_output=True, text=True, check=True).stdout
        print(out.strip())
    """)
    fx.run(gitck)
    _, _, note = fx.run(gitck)
    check("a read-only git answer that did not change hits", kind(note), "HIT")
    with open(os.path.join(repo, "b.txt"), "w", encoding="utf-8", newline="\n") as fh:
        fh.write("b\n")
    git(repo, "add", "b.txt")
    git(repo, "commit", "-q", "-m", "two")
    _, _, note = fx.run(gitck)
    check("the repository moved, so git answers differently: miss",
          (kind(note), "answers differently" in note), ("MISS", True))

    batch = fx.checker(f"""
        import subprocess
        p = subprocess.run(["git", "-C", {repo!r}, "cat-file", "--batch"],
                           input=b"HEAD:a.txt\\n", capture_output=True, check=True)
        print(p.stdout.splitlines()[-1].decode())
    """)
    fx.run(batch)
    _, _, note = fx.run(batch)
    check("git cat-file --batch with piped stdin is recorded and replayed: hit",
          kind(note), "HIT")

    gitwrite = fx.checker(f"""
        import subprocess
        subprocess.run(["git", "-C", {repo!r}, "tag", "-f", "t1"], check=True)
    """)
    _, _, note = fx.run(gitwrite)
    check("a git write to a repository the run did not create is not stored",
          ("not stored" in note, "did not create" in note), (True, True))

    scratchgit = fx.checker(f"""
        import os, subprocess, sys, tempfile
        sys.path.insert(0, {HERE!r})
        import gitenv
        env = gitenv.clean_env()
        with tempfile.TemporaryDirectory() as d:
            for args in (["init", "-q"], ["config", "user.name", "x"],
                         ["config", "user.email", "x@invalid"]):
                subprocess.run(["git", *args], cwd=d, env=env, check=True)
            open(os.path.join(d, "f"), "w").write("x")
            subprocess.run(["git", "add", "f"], cwd=d, env=env, check=True)
            subprocess.run(["git", "commit", "-q", "-m", "m"], cwd=d, env=env, check=True)
        print("ok")
    """)
    fx.run(scratchgit)
    _, _, note = fx.run(scratchgit)
    check("git confined to a directory the run created is the run's own: hit",
          kind(note), "HIT")

    # --- replay is byte-exact ------------------------------------------------------
    raw = fx.checker(r"""
        import sys
        sys.stdout.buffer.write(b"\xff\xfe not utf-8\r\n")
        sys.stdout.flush()
        sys.stderr.write("to stderr\n")
        sys.stderr.flush()
        print("last")
    """)
    _, first, _ = fx.run(raw)
    _, again, note = fx.run(raw)
    check("non-UTF-8 output with stderr interleaved is replayed byte for byte",
          (kind(note), again), ("HIT", first))

    # --- the controls -------------------------------------------------------------
    fx.run(ck)  # the input file was rewritten above; bring its entry up to date
    rc, _, note = fx.run(ck, verify="1")
    check("a verified lookup runs fresh and agrees", (rc, kind(note)), (0, "VERIFIED"))

    liar = fx.checker(f"""
        import sys
        sys.exit(0 if "hello" in open({data!r}).read() else 1)
    """)
    fx.run(liar)
    entries = [f for f in os.listdir(fx.store) if f.endswith(".json")]
    target = None
    for name in entries:
        with open(os.path.join(fx.store, name), encoding="utf-8") as fh:
            e = json.load(fh)
        if e["argv"][-1] == liar:
            target = os.path.join(fx.store, name)
    # Make the entry lie: it now claims the run passed with an exit status the
    # checker never gave. Its inputs still match, so only a fresh run can tell.
    with open(target, encoding="utf-8") as fh:
        e = json.load(fh)
    e["rc"] = 7
    with open(target, "w", encoding="utf-8", newline="\n") as fh:
        json.dump(e, fh)
    rc, _, note = fx.run(liar, verify="1")
    check("a verified lookup whose entry lies is caught; the fresh verdict stands",
          (rc, "VERIFY FAILED" in note), (0, True))
    check("...the lying entry is deleted", os.path.exists(target), False)
    check("...and the cache is disabled for the rest of the run",
          os.path.exists(os.path.join(fx.store, "DISABLED")), True)
    rc, _, note = fx.run(ck)
    check("with the cache disabled, gates run and say nothing", (rc, note), (0, ""))
    os.remove(os.path.join(fx.store, "DISABLED"))

    stale = [f for f in os.listdir(fx.store) if f.endswith(".json")][0]
    sp = os.path.join(fx.store, stale)
    with open(sp, encoding="utf-8") as fh:
        e = json.load(fh)
    e["utc_date"] = "2000-01-01"
    with open(sp, "w", encoding="utf-8", newline="\n") as fh:
        json.dump(e, fh)
    check("an entry from another UTC day is not trusted",
          "entries last one UTC day" in _lookup_reason(fx, e), True)

    # A trace lost on its way to merge says nothing about the checker, so it is
    # not stored and not remembered: the next run is an ordinary miss, stored,
    # not "found uncacheable earlier today".
    damager = fx.checker("""
        import os
        if os.environ.get("GATE_TEST_DAMAGE") == "1":
            with open(os.path.join(os.environ["GATE_CACHE_TRACE_DIR"], "git-junk.json"),
                      "w", encoding="utf-8") as fh:
                fh.write("{}{}")
        print("ran")
    """)
    rc, _, note = fx.run(damager, env=dict(os.environ, GATE_TEST_DAMAGE="1"))
    check("a damaged trace: the pass stands, not stored",
          (rc, "not stored" in note, "could not be read" in note), (0, True, True))
    rc, _, note = fx.run(damager, env=dict(os.environ, GATE_TEST_DAMAGE="0"))
    check("...and not remembered: the next run is traced and stored",
          (rc, kind(note), "stored" in note and "not stored" not in note), (0, "MISS", True))


def _lookup_reason(fx: Fixture, entry: dict) -> str:
    """Ask the driver's own validate() about an entry, as a lookup would."""
    code = ("import json,sys; sys.path.insert(0, %r); import importlib.util as u;"
            "s=u.spec_from_file_location('gc', %r); m=u.module_from_spec(s);"
            "s.loader.exec_module(m); print(m.validate(json.loads(sys.stdin.read())))"
            % (HERE, DRIVER))
    p = subprocess.run([sys.executable, "-c", code], input=json.dumps(entry),
                       capture_output=True, text=True)
    return p.stdout.strip() + p.stderr.strip()


if __name__ == "__main__":
    sys.exit(main())
