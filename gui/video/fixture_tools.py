"""Where the video crates' fixture generators find the programs they run.

A tool comes from, in order: the generator's own option, where it has one;
the tool's environment variable (`FFMPEG`, `FFPROBE`, `MKVMERGE`); `PATH`; and
last the folders the operator keeps tools in (`C:\\utils`, or `D:\\utils` on a
machine that has that drive, with MKVToolNix in its `mkvtoolnix-99.0`
subfolder). A tool found nowhere stops the generator with a message naming
those ways to supply it, rather than a missing-file error naming one guessed
path on a drive the machine may not have.

The generators sit in `gui/video/<crate>/tests/data/` and import this with
`sys.path.insert(0, str(Path(__file__).resolve().parents[3]))`.
"""

from __future__ import annotations

import os
import shutil
from pathlib import Path

#: The folders tools are kept in, the one-drive machine's first.
UTILS = (Path("C:/utils"), Path("D:/utils"))

#: Each tool's environment variable, and the subfolders of a utils folder it
#: may be unpacked in.
ENV = {"ffmpeg": "FFMPEG", "ffprobe": "FFPROBE", "mkvmerge": "MKVMERGE"}
SUBDIRS = {"mkvmerge": ("mkvtoolnix-99.0",)}


def tool(name: str, given: str | None = None) -> str:
    """The program *name* (`ffmpeg`, `ffprobe`, `mkvmerge`) to run: *given*
    (the generator's own option) if set, else the first found as the module
    says. Stops with a message when it is nowhere."""
    if given:
        return given
    env = ENV.get(name)
    if env and os.environ.get(env):
        return os.environ[env]
    found = shutil.which(name)
    if found:
        return found
    exe = name + (".exe" if os.name == "nt" else "")
    for utils in UTILS:
        for sub in ("",) + SUBDIRS.get(name, ()):
            p = utils / sub / exe
            if p.is_file():
                return str(p)
    raise SystemExit(f"{name} not found: set {env}, put it on PATH, or keep it in "
                     f"{' or '.join(str(u) for u in UTILS)}")


def tool_dir(name: str) -> str:
    """The folder *name* is in, for a generator that takes a folder of tools
    (ffmpeg's, which holds ffprobe too)."""
    return str(Path(tool(name)).parent)


if __name__ == "__main__":
    for n in ENV:
        try:
            print(f"{n:9} {tool(n)}")
        except SystemExit as e:
            print(e)
