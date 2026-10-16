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

    # --- imports: what each import looked for, not the directory's listing ---
    fx.write("helper_mod.py", "VALUE = 'helped'\n")
    importer = fx.checker("""
        import json
        import helper_mod
        print(helper_mod.VALUE, json.dumps(1))
    """)
    fx.run(importer)
    _, _, note = fx.run(importer)
    check("a gate that imports from its own directory hits when nothing changed",
          kind(note), "HIT")
    fx.write("check-something-else.py", "print('another gate')\n")
    _, _, note = fx.run(importer)
    check("a script it does not import, added beside the ones it does: still a hit",
          kind(note), "HIT")
    fx.write("helper_mod.py", "VALUE = 'changed'\n")
    _, _, note = fx.run(importer)
    check("a module it imported changed: miss",
          (kind(note), "helper_mod.py changed" in note), ("MISS", True))
    fx.run(importer)
    fx.write("json.py", "raise SystemExit('the standard json, shadowed')\n")
    rc, _, note = fx.run(importer)
    # The traced run would import the json the tracer loaded first; the
    # untraced gate imports this file and fails, and that verdict stands.
    check("a module that would shadow one it imported appeared: miss, untraced verdict",
          (kind(note), "an import of json finds" in note, "exit 1 stands" in note, rc),
          ("MISS", True, True, 1))
    os.remove(fx.path("json.py"))
    # A second importer: the first is uncacheable for the day now.
    importer2 = fx.checker("""
        import helper_mod
        print(helper_mod.VALUE)
    """)
    fx.run(importer2)
    os.makedirs(fx.path("helper_mod"))
    _, _, note = fx.run(importer2)
    check("a package of the same name appeared beside a module it imported: miss",
          (kind(note), "an import of helper_mod finds" in note), ("MISS", True))
    os.rmdir(fx.path("helper_mod"))

    # An import answered by a valid .pyc opens only the bytecode, never the
    # source: the source must be an input all the same.
    fx.write("pyc_mod.py", "VALUE = 'one'\n")
    pyc_importer = fx.checker("""
        import pyc_mod
        print(pyc_mod.VALUE)
    """)
    warm = {k: v for k, v in os.environ.items() if k != "PYTHONDONTWRITEBYTECODE"}
    subprocess.run([sys.executable, "-c", "import pyc_mod"], cwd=fx.root, env=warm,
                   check=True)
    check("the fixture has a .pyc for the module",
          any(n.startswith("pyc_mod.") for n in os.listdir(fx.path("__pycache__"))), True)
    fx.run(pyc_importer)
    fx.write("pyc_mod.py", "VALUE = 'two, longer'\n")
    _, out, note = fx.run(pyc_importer)
    check("a module imported through its .pyc changed: miss",
          (kind(note), "pyc_mod.py changed" in note, b"two" in out), ("MISS", True, True))

    # A module the tracer itself loads before the gate runs (json), shadowed
    # by a file beside the gate: the traced run imports the real one, the
    # untraced gate the file. The traced pass is not stored, and the
    # untraced run's verdict is the one returned.
    shadowed = fx.checker("""
        import json
        print(json.dumps(2))
    """)
    fx.write("json.py", "raise SystemExit('the standard json, shadowed')\n")
    rc, _, note = fx.run(shadowed)
    os.remove(fx.path("json.py"))
    check("a file shadowing a module the tracer loaded first: not stored, untraced verdict",
          (rc, "would shadow the json module" in note, "exit 1 stands" in note),
          (1, True, True))
    # Named like it, but no import would load it: notes, not a module.
    beside_notes = fx.checker("""
        import json
        print(json.dumps(3))
    """)
    fx.write("json.md", "notes on json\n")
    fx.run(beside_notes)
    rc, _, note = fx.run(beside_notes)
    os.remove(fx.path("json.md"))
    check("a file named like such a module that is not one (json.md): cached, a hit",
          (rc, kind(note), "would shadow" in note), (0, "HIT", False))

    # The installation's own modules are pinned by the key's interpreter
    # version; a module of the gate's own is an input.
    fx.write("own_mod.py", "VALUE = 'own'\n")
    stdlib_user = fx.checker("""
        import colorsys
        import own_mod
        print(own_mod.VALUE, colorsys.rgb_to_hsv(0, 0, 0))
    """)
    fx.run(stdlib_user)
    reads = [os.path.basename(p).lower() for p in entry_for(fx, stdlib_user)["inputs"]["reads"]]
    check("a gate's own module is an input, an installed one (colorsys) is not",
          ("own_mod.py" in reads, "colorsys.py" in reads), (True, False))

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
    logged_input = fx.write("data/logged.txt", "one\n")
    logged = fx.checker(f"""
        print(open({logged_input!r}, encoding="utf-8").read().strip())
    """)
    fx.run(logged, env=env)
    fx.write("data/logged.txt", "two\n")
    fx.run(logged, env=env)
    with open(logf, encoding="utf-8") as fh:
        rows = [ln.rstrip("\n").split("\t") for ln in fh]
    check("each invocation logs its label and outcome",
          [r[:2] for r in rows], [["t", "HIT"], ["t", "SKIP"], ["t", "MISS"], ["t", "MISS"]])
    check("...and how long it took", all(float(r[2]) >= 0 for r in rows), True)
    check("...and why a gate ran, as a kind to count and the reason itself",
          [(r[3], r[4].endswith("logged.txt changed")) for r in rows],
          [("", False), ("writes outside its own directories", False),
           ("no entry", False), ("a file it read changed", True)])
    reason_kinds()

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
    target = entry_for(fx, liar)["_path"]
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


def entry_for(fx: Fixture, script: str) -> dict:
    """The stored cache entry of checker `script`, with its file as `_path`."""
    for name in os.listdir(fx.store):
        if not name.endswith(".json"):
            continue
        path = os.path.join(fx.store, name)
        with open(path, encoding="utf-8") as fh:
            e = json.load(fh)
        if e["argv"][-1] == script:
            e["_path"] = path
            return e
    raise AssertionError(f"no cache entry for {script}")


def reason_kinds() -> None:
    """Every reason the driver and the tracer give, as each words it, is
    counted under a kind without a path, a date or a value in it -- so the
    same cause adds up across gates and boots -- and one nobody wrote a
    pattern for is "other", not lost."""
    import srcload
    gc = srcload.load(DRIVER, "gate_cache_kinds")
    path = os.path.join(tempfile.gettempdir(), "some dir", "x.py")
    samples = {
        "no entry": "no entry",
        "recorded on 2026-10-15, and entries last one UTC day": "recorded on another UTC day",
        "environment variable LANE changed": "environment variable LANE",
        f"the listing of {path} changed": "a directory listing changed",
        f"what an import of json finds in {path} changed": "what an import finds changed",
        f"isfile of {path} changed": "isfile of a path changed",
        f"{path} changed": "a file it read changed",
        f"{path} changed while the gate ran": "an input changed while the gate ran",
        "git ls-files -z answers differently": "git ls-files -z answers differently",
        "runs 'cargo.exe', whose reads cannot be seen": "runs cargo",
        "a shell command line: ['ls']": "a shell command line",
        f"writes {path}, outside the directories the run created": "writes outside its own directories",
        f"open on {path}, outside the directories the run created": "open outside its own directories",
        "socket.connect: an input the tracer cannot see": "socket.connect: an input the tracer cannot see",
        "git status against a repository the run did not create":
            "git status on a repository the run did not create",
        f"shadowed: {path}* would shadow the json module the tracer loaded first":
            "a file would shadow a module the tracer loaded",
        "traced exit 1, untraced 0": "traced and untraced runs disagreed",
        "something nobody has written yet": "other",
    }
    got = {r: gc.reason_kind(r) for r in samples}
    check("each reason is counted under its kind", got, samples)


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
