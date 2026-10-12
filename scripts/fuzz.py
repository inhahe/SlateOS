#!/usr/bin/env python3
"""Fuzz a crate with its own harness: libFuzzer through `cargo fuzz`, in WSL.

    python scripts/fuzz.py run <harness-dir> [SECONDS]   set up, then run every target
    python scripts/fuzz.py setup <harness-dir>           only set up
    python scripts/fuzz.py --self-test

Run from Windows or from WSL: on Windows it runs itself again in WSL (cargo-fuzz
needs Linux), with the paths translated. WSL needs a nightly toolchain and
`cargo install cargo-fuzz --locked`.

A HARNESS is a directory beside the crate it fuzzes, `<crate>/fuzz/`, holding:

- `harness.toml`: the cargo-fuzz manifest. Kept under that name so that
  nothing in the real workspace takes the directory for a package. Its `path`
  dependencies are written from the fuzz tree's root, which mirrors the
  repository: `netproto = { path = "../netproto" }`.
- `fuzz_targets/<target>.rs`: one per `[[bin]]` in `harness.toml`.
- `<target>.dict`, optionally: tokens the parser reads, for libFuzzer's
  mutations.
- Its seeds, given in `harness.toml`. A table maps each target to directories
  of the repository whose files seed it:

      [package.metadata.slateos-fuzz]
      seeds = { ipv4 = ["netproto/tests/data"] }
      sanitize = ["image"]     # targets that reach `unsafe` code: run under ASan

  `seeds.py`, optionally, for seeds that have to be made: its
  `seed(src, corpus)` is called with the checkout's root and a function that
  answers a target's corpus directory.

`setup` copies the crates the harness reaches by `path`, sources and manifests
only, into `~/fuzz/<package>/tree` under a workspace root holding the real one's
lints. It installs the harness as `tree/fuzz` and adds the seeds. The corpus the
fuzzer has grown, and its findings, stay there from run to run.

`run` builds every target with `-O` and no debug assertions, as a release
build ships, and runs them all in parallel at low priority for SECONDS each
(default 1200). Each run gets a 4 GB memory limit and 20 seconds per input, so
a hang or a runaway allocation is a finding, as a panic is. Targets listed in
`sanitize` run under the address sanitizer; the others run without it, at about
twice the speed. A finding lands in `tree/fuzz/artifacts/<target>/`. Reproduce
it with `cargo +nightly fuzz run -O <target> <artifact>` in `tree/`, fix the
crate, and copy the input into the crate's tests as a regression test.

Exit status: 0 nothing new found; 1 a finding this round, or a target that did not build; 2
a usage error. A round loads every core WSL has: do not start one while a QEMU
guest is up.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import selftestflag  # noqa: E402

PATH_DEP = re.compile(r'path\s*=\s*"([^"]+)"')
SKIP = ("target", "tests", "benches", "examples", "fuzz")
MAX_SEED = 256 * 1024
LIBFUZZER = ["-rss_limit_mb=4096", "-timeout=20", "-print_final_stats=1"]


def wsl_path(p: Path) -> str:
    """`E:\\x\\y` as WSL sees it: `/mnt/e/x/y`."""
    s = str(p.resolve()).replace(chr(92), "/")
    if len(s) > 1 and s[1] == ":":
        return f"/mnt/{s[0].lower()}{s[2:]}"
    return s


def qemu_running() -> bool:
    """Whether a QEMU guest is up on this Windows machine: a fuzz round
    beside one stretches its timings (CLAUDE.md)."""
    out = subprocess.run(["tasklist", "/NH"], capture_output=True, text=True,
                         encoding="utf-8", errors="replace", stdin=subprocess.DEVNULL).stdout
    return "qemu-system" in out.lower()


def closure(src: Path, roots: list[str]) -> list[str]:
    """`roots` and every crate they reach by a `path` dependency, as
    repository-relative directories."""
    seen: set[str] = set()
    todo = list(roots)
    while todo:
        crate = os.path.normpath(todo.pop()).replace(os.sep, "/")
        if crate in seen:
            continue
        seen.add(crate)
        manifest = (src / crate / "Cargo.toml").read_text(encoding="utf-8")
        for line in manifest.splitlines():
            m = PATH_DEP.search(line.split("#", 1)[0])
            if m:
                dep = os.path.normpath(os.path.join(crate, m.group(1))).replace(os.sep, "/")
                if (src / dep / "Cargo.toml").is_file():
                    todo.append(dep)
    return sorted(seen)


class Harness:
    def __init__(self, directory: Path):
        self.dir = directory
        self.manifest_text = (directory / "harness.toml").read_text(encoding="utf-8")
        m = tomllib.loads(self.manifest_text)
        self.name = m["package"]["name"]
        self.targets = [b["name"] for b in m.get("bin", [])]
        meta = m["package"].get("metadata", {}).get("slateos-fuzz", {})
        self.seeds: dict[str, list[str]] = meta.get("seeds", {})
        self.sanitize: list[str] = meta.get("sanitize", [])
        # Dependencies are written from the fuzz tree's root: `../<crate>`.
        self.roots = [d["path"].removeprefix("../") for d in m.get("dependencies", {}).values()
                      if isinstance(d, dict) and "path" in d]
        unknown = set(self.seeds) | set(self.sanitize)
        unknown -= set(self.targets)
        if unknown:
            raise SystemExit(f"fuzz: {directory}/harness.toml names no such target: "
                             f"{', '.join(sorted(unknown))}")


def setup(harness: Harness, src: Path, home: Path) -> Path:
    """Make `home/fuzz/<name>/tree` from `src`, and answer it."""
    tree = home / "fuzz" / harness.name / "tree"
    crates = closure(src, harness.roots)
    print("fuzz: crates:", " ".join(crates))
    for crate in crates:
        dst = tree / crate
        if dst.is_dir():
            shutil.rmtree(dst)
        dst.mkdir(parents=True)
        for item in (src / crate).iterdir():
            if item.name in ("target", "fuzz"):
                continue
            if item.name in SKIP:
                # Only the sources: a manifest's `[[test]]`, `[[bench]]` and
                # `[[example]]` must find their files, or cargo refuses the
                # whole workspace; their data stays behind.
                for f in item.rglob("*.rs"):
                    (dst / f.relative_to(src / crate)).parent.mkdir(parents=True, exist_ok=True)
                    shutil.copy2(f, dst / f.relative_to(src / crate))
            elif item.is_dir():
                shutil.copytree(item, dst / item.name)
            else:
                shutil.copy2(item, dst / item.name)
    root = (src / "Cargo.toml").read_text(encoding="utf-8")
    start = root.index("[workspace.lints.rust]")
    end = root.index("\n[profile", start)
    members = ",\n".join(f'    "{c}"' for c in crates)
    with open(tree / "Cargo.toml", "w", encoding="utf-8", newline="\n") as f:
        f.write(f'[workspace]\nresolver = "2"\nexclude = ["fuzz"]\nmembers = [\n{members},\n]\n\n'
                f"{root[start:end]}\n")
    fuzz = tree / "fuzz"
    fuzz.mkdir(parents=True, exist_ok=True)
    with open(fuzz / "Cargo.toml", "w", encoding="utf-8", newline="\n") as f:
        f.write(harness.manifest_text)
    if (fuzz / "fuzz_targets").is_dir():
        shutil.rmtree(fuzz / "fuzz_targets")
    shutil.copytree(harness.dir / "fuzz_targets", fuzz / "fuzz_targets")
    for d in harness.dir.glob("*.dict"):
        shutil.copy2(d, fuzz / d.name)

    def corpus(target: str) -> Path:
        d = fuzz / "corpus" / target
        d.mkdir(parents=True, exist_ok=True)
        return d

    def names(target: str) -> set[str]:
        return {p.name for p in corpus(target).iterdir()}

    had = {t: names(t) for t in harness.targets}
    for target, dirs in harness.seeds.items():
        for sub in dirs:
            base = src / sub
            if not base.is_dir():
                print(f"fuzz: {target}'s seed directory {sub} is not there")
            for p in sorted(base.rglob("*")) if base.is_dir() else []:
                if p.is_file() and p.stat().st_size <= MAX_SEED:
                    rel = str(p.relative_to(src)).replace(os.sep, "_").replace("/", "_")
                    shutil.copy2(p, corpus(target) / rel)
    hook = harness.dir / "seeds.py"
    if hook.is_file():
        namespace: dict = {"__file__": str(hook)}
        exec(compile(hook.read_text(encoding="utf-8"), str(hook), "exec"), namespace)
        namespace["seed"](str(src), lambda t: str(corpus(t)))
    # What this setup added, and what the corpus holds with what the fuzzer
    # has grown: a seed source gone missing shows as nothing added.
    print("fuzz: seeds added (corpus):", ", ".join(
        f"{t} {len(names(t) - had[t])} ({len(names(t))})" for t in harness.targets))
    return tree


def run(harness: Harness, tree: Path, secs: int) -> int:
    home = tree.parent
    logs = home / "logs"
    logs.mkdir(exist_ok=True)
    env = dict(os.environ, PATH=f"{Path.home()}/.cargo/bin:{os.environ.get('PATH', '')}")
    # `cargo fuzz build` takes one target at a time.
    builds = [["cargo", "+nightly", "fuzz", "build", "-O", t] if t in harness.sanitize
              else ["cargo", "+nightly", "fuzz", "build", "-O", "-s", "none",
                    "--target-dir", str(home / "target-none"), t]
              for t in harness.targets]
    for b in builds:
        with open(logs / "build.log", "a", encoding="utf-8", newline="\n") as log:
            if subprocess.run(b, cwd=tree, env=env, stdout=log, stderr=subprocess.STDOUT,
                              stdin=subprocess.DEVNULL).returncode != 0:
                print(f"fuzz: a target did not build; see {logs / 'build.log'}")
                return 1
    def artifacts(t: str) -> set[Path]:
        d = tree / "fuzz" / "artifacts" / t
        return set(d.glob("*")) if d.is_dir() else set()

    # Findings already there are earlier rounds', reported then.
    before = {t: artifacts(t) for t in harness.targets}
    procs = []
    for t in harness.targets:
        cmd = ["nice", "-n", "10", "cargo", "+nightly", "fuzz", "run", "-O"]
        if t not in harness.sanitize:
            cmd += ["-s", "none", "--target-dir", str(home / "target-none")]
        cmd += [t, "--", f"-max_total_time={secs}", *LIBFUZZER]
        if (tree / "fuzz" / f"{t}.dict").is_file():
            cmd.append(f"-dict={tree / 'fuzz' / f'{t}.dict'}")
        log = open(logs / f"{t}.log", "w", encoding="utf-8", newline="\n")
        procs.append((t, log, subprocess.Popen(cmd, cwd=tree, env=env, stdout=log,
                                               stderr=subprocess.STDOUT,
                                               stdin=subprocess.DEVNULL)))
    found = False
    for t, log, p in procs:
        code = p.wait()
        log.close()
        text = (logs / f"{t}.log").read_text(encoding="utf-8", errors="replace")
        stats = re.findall(r"^stat::(number_of_executed_units|new_units_added):\s*(\d+)", text, re.M)
        new = sorted(artifacts(t) - before[t])
        found = found or bool(new)
        print(f"=== {t}: " + ", ".join(f"{k} {v}" for k, v in stats))
        for line in re.findall(r"^(?:SUMMARY.*|.*panicked at.*|==\d+== ERROR.*)$", text, re.M)[:3]:
            print(f"    {line}")
        for a in new:
            print(f"    finding: {a}")
        if code != 0 and not new:
            # It stopped without a finding: a bad dictionary, a corpus it
            # could not read, the out-of-memory killer. Not a clean round.
            found = True
            print(f"    did not fuzz to the end (exit {code}); the log's end:")
            for line in text.splitlines()[-6:]:
                print(f"      {line}")
        if before[t]:
            print(f"    ({len(before[t])} finding(s) from earlier rounds in "
                  f"{tree / 'fuzz' / 'artifacts' / t})")
    return 1 if found else 0


def self_test() -> int:
    failures = []

    def expect(what, got, want):
        if got != want:
            failures.append(f"{what}: got {got!r}, want {want!r}")

    expect("wsl path", wsl_path(Path("E:/visual studio projects/os")) if os.name == "nt"
           else "/mnt/e/visual studio projects/os", "/mnt/e/visual studio projects/os")
    with tempfile.TemporaryDirectory() as tmp:
        src = Path(tmp) / "src"

        def crate(path: str, deps: str = "") -> None:
            (src / path / "src").mkdir(parents=True)
            (src / path / "src" / "lib.rs").write_text("", encoding="utf-8", newline="\n")
            (src / path / "tests").mkdir()
            (src / path / "tests" / "big.bin").write_bytes(b"x")
            # A `[[bench]]` the manifest declares: its source must come along.
            (src / path / "benches").mkdir()
            (src / path / "benches" / "rate.rs").write_text("fn main() {}", encoding="utf-8", newline="\n")
            (src / path / "Cargo.toml").write_text(
                f'[package]\nname = "{path.split("/")[-1]}"\n[dependencies]\n{deps}',
                encoding="utf-8", newline="\n")

        crate("a", 'b = { path = "../lib/b" }  # a comment with path = "nowhere"\n')
        crate("lib/b", 'c = { path = "../../c" }\nserde = "1"\n')
        crate("c")
        crate("unrelated")
        (src / "Cargo.toml").write_text(
            '[workspace]\n[workspace.lints.rust]\nunsafe_code = "warn"\n\n[profile.dev]\n',
            encoding="utf-8", newline="\n")
        (src / "a" / "tests" / "data").mkdir()
        (src / "a" / "tests" / "data" / "one.pkt").write_bytes(b"seed")
        h = src / "a" / "fuzz"
        (h / "fuzz_targets").mkdir(parents=True)
        (h / "fuzz_targets" / "parse.rs").write_text("", encoding="utf-8", newline="\n")
        (h / "parse.dict").write_text('"tok"\n', encoding="utf-8", newline="\n")
        (h / "harness.toml").write_text(
            '[package]\nname = "a-fuzz"\n[package.metadata.slateos-fuzz]\n'
            'seeds = { parse = ["a/tests/data"] }\n[dependencies]\n'
            'libfuzzer-sys = "0.4"\na = { path = "../a" }\n'
            '[[bin]]\nname = "parse"\npath = "fuzz_targets/parse.rs"\n', encoding="utf-8",
            newline="\n")
        (h / "seeds.py").write_text(
            "import os\ndef seed(src, corpus):\n"
            "    open(os.path.join(corpus('parse'), 'made'), 'wb').write(b'made')\n",
            encoding="utf-8", newline="\n")
        harness = Harness(h)
        expect("targets", harness.targets, ["parse"])
        expect("roots", harness.roots, ["a"])
        expect("closure", closure(src, ["a"]), ["a", "c", "lib/b"])
        home = Path(tmp) / "home"
        tree = setup(harness, src, home)
        expect("tree", tree, home / "fuzz" / "a-fuzz" / "tree")
        expect("crates copied without tests", sorted(
            str(p.relative_to(tree)).replace(os.sep, "/") for p in tree.rglob("lib.rs")),
            ["a/src/lib.rs", "c/src/lib.rs", "lib/b/src/lib.rs"])
        expect("no test data copied", list(tree.glob("*/tests/big.bin")), [])
        expect("declared bench sources copied", (tree / "a" / "benches" / "rate.rs").is_file(),
               True)
        root = (tree / "Cargo.toml").read_text(encoding="utf-8")
        expect("workspace lints kept", "unsafe_code" in root and '"lib/b"' in root, True)
        expect("harness installed", (tree / "fuzz" / "Cargo.toml").is_file()
               and (tree / "fuzz" / "parse.dict").is_file(), True)
        expect("seeds", sorted(p.name for p in (tree / "fuzz" / "corpus" / "parse").iterdir()),
               ["a_tests_data_one.pkt", "made"])
        (h / "harness.toml").write_text(
            (h / "harness.toml").read_text(encoding="utf-8").replace(
                'seeds = { parse', 'sanitize = ["nope"]\nseeds = { parse'), encoding="utf-8",
            newline="\n")
        try:
            Harness(h)
            failures.append("a sanitize entry naming no target was accepted")
        except SystemExit:
            pass
    for f in failures:
        print(f"FAIL {f}")
    print(f"fuzz self-test: {'FAILED' if failures else 'ok'}")
    return 1 if failures else 0


def main(argv: list[str]) -> int:
    if selftestflag.wants_selftest(argv):
        return self_test()
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("action", choices=("setup", "run"))
    ap.add_argument("harness", help="the harness directory, e.g. netproto/fuzz")
    ap.add_argument("seconds", nargs="?", type=int, default=1200, help="per target, for run")
    ap.add_argument("--src", help="the checkout to copy the crates from (default: the one "
                    "the harness is in)")
    a = ap.parse_args(argv)
    if os.name == "nt":
        if a.action == "run" and qemu_running():
            print("fuzz: a QEMU guest is up, and a round loads every core: run it when the "
                  "guest has finished")
            return 1
        # The crates are the harness's own checkout's, not this script's: a
        # lane trying a harness in its own worktree fuzzes its own code.
        # A harness outside any checkout of this repository (being tried
        # from a scratch directory, say) fuzzes the script's own checkout.
        top = subprocess.run(["git", "-C", str(Path(a.harness).resolve()), "rev-parse",
                              "--show-toplevel"], capture_output=True, text=True,
                             encoding="utf-8", errors="replace", stdin=subprocess.DEVNULL)
        src = Path(__file__).resolve().parent.parent
        if top.returncode == 0 and top.stdout.strip():
            candidate = Path(top.stdout.strip())
            roots = Harness(Path(a.harness).resolve()).roots
            if all((candidate / r / "Cargo.toml").is_file() for r in roots):
                src = candidate
        # `-u`: the log a background run writes to shows progress as it goes.
        r = subprocess.run(["wsl", "-d", "Ubuntu", "--exec", "python3", "-u",
                            wsl_path(Path(__file__)), a.action, wsl_path(Path(a.harness)),
                            str(a.seconds), "--src", wsl_path(src)],
                           stdin=subprocess.DEVNULL)
        return r.returncode
    harness = Harness(Path(a.harness).resolve())
    src = Path(a.src) if a.src else Path(__file__).resolve().parent.parent
    tree = setup(harness, src, Path.home())
    if a.action == "setup":
        return 0
    return run(harness, tree, a.seconds)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
