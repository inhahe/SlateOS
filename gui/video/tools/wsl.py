"""Commands run in WSL in batches, for the oracle tools that ask a Linux
build of FFmpeg about many streams (`gui/video/aac/tools/pcm_expect.py`).

    import wsl
    results = wsl.run_all(commands, scratch, jobs, check=[ffprobe, "-version"])

Each command runs in WSL's Ubuntu; all of them through one `wsl.exe` --
with ten `wsl.exe` at once, some exit -1 partway through their output --
`jobs` chains at a time.
"""

import re
import shlex
import subprocess
from dataclasses import dataclass
from pathlib import Path


def wsl_path(p: Path) -> str:
    """`p` as WSL sees it: `E:/x/y` is `/mnt/e/x/y`."""
    s = p.resolve().as_posix()
    return f"/mnt/{s[0].lower()}{s[2:]}"


@dataclass
class Result:
    """What a command run in WSL wrote, as text, and how it exited."""
    out: str
    err: str
    status: int


ASSIGNMENT = re.compile(r"^([A-Z][A-Z0-9_]*)=(.*)$", re.S)


def run_all(commands: list[list[str]], scratch: Path, jobs: int,
            check: list[str] | None = None) -> list[Result]:
    """Each of `commands` run in WSL, in order of the list in the results.
    A command may begin with variable assignments (`FFDBG_PCM=/tmp/x`), the
    values quoted; every other word is quoted whole. Each one's output goes
    to files under `scratch`, read back and removed with it. `check`, if
    given, is run first and must succeed: a build that does not run would
    otherwise give nothing, which some streams also give."""
    if not commands:
        return []
    if check:
        subprocess.run(["wsl", "-d", "Ubuntu", "--exec", *check], stdin=subprocess.DEVNULL,
                       capture_output=True, check=True)
    scratch.mkdir(parents=True, exist_ok=True)
    chains: list[list[str]] = [[] for _ in range(jobs)]
    for k, argv in enumerate(commands):
        words, assigning = [], True
        for a in argv:
            m = ASSIGNMENT.match(a) if assigning else None
            if m:
                words.append(f"{m.group(1)}={shlex.quote(m.group(2))}")
            else:
                assigning = False
                words.append(shlex.quote(a))
        target = wsl_path(scratch / str(k))
        chains[k % jobs].append(f"{' '.join(words)} > {shlex.quote(target + '.out')} "
                                f"2> {shlex.quote(target + '.err')}; "
                                f"echo $? > {shlex.quote(target + '.rc')}")
    script = ["#!/bin/bash"]
    for chain in chains:
        if chain:
            script.append("(\n" + "\n".join(chain) + "\n) &")
    script.append("wait")
    script_file = scratch / "run.sh"
    script_file.write_bytes(("\n".join(script) + "\n").encode())
    subprocess.run(["wsl", "-d", "Ubuntu", "--exec", "bash", wsl_path(script_file)],
                   stdin=subprocess.DEVNULL, check=True)
    results = []
    for k in range(len(commands)):
        read = lambda ext: (scratch / f"{k}.{ext}").read_bytes().decode("utf-8", "replace")
        results.append(Result(read("out"), read("err"), int(read("rc").strip() or "-1")))
    for f in scratch.iterdir():
        f.unlink()
    scratch.rmdir()
    return results
