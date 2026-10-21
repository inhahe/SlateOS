#!/usr/bin/env python3
"""Build the tree that puts the fastpy compiler on a SlateOS image.

fastpy compiles Python into native programs: Python source -> LLVM IR (built
by `llvmlite.ir`, which is pure Python) -> an object (LLVM's `opt` and `llc`)
-> a program (`ld.lld`, against fastpy's C runtime and the C library). On
SlateOS every step of that runs on the image itself: the image's CPython runs
the compiler, lane D's static LLVM tools turn the IR into an object, and the
link uses two archives in the system library directory. This script
assembles lane B's share as a directory tree laid out like the image, for the
rootfs recipe to copy in:

    <out>/usr/lib/x86_64-slateos/libfastpy_rt.a   fastpy's C runtime, pure mode
    <out>/usr/lib/fastpy/compiler/                 the compiler package
    <out>/usr/lib/fastpy/llvmlite/                 llvmlite's IR builder, only
    <out>/usr/lib/fastpy/BUNDLE                    what this tree was built from
    <out>/bin/fastpy                               the command

What the tree leaves out is as deliberate as what it holds. `opt`, `llc`,
`ld.lld` and `libc.a` are lane D's to build and stage
(`requests/b-d-fastpy-on-slateos-needs-llvm-tools.md`). And `llvmlite.binding`
-- the half of llvmlite that loads LLVM as a shared library through `ctypes` --
can never load on a system with no dynamic linker, so it is not shipped at all
rather than shipped broken: fastpy reaches LLVM through the tools there, and
chooses them by itself when the binding is absent (`FASTPY_LLVM_BACKEND`).

Byte-compiled `.pyc` files are written when an interpreter of the image's
CPython version is at hand -- the running one, or `python3.12` on `PATH` -- as
checked-hash pycs, which stay valid through a copy that does not keep
modification times. Without one, the sources go alone and the image compiles
them on first import: correct, but slow for a 2.8 MB `codegen.py` under
emulation, so the script says which it did.

The runtime archive comes from fastpy itself (`toolchain.
build_slateos_runtime_archive`), which cross-compiles the C runtime with
`zig cc` and packs it with `zig ar`. So this needs what a fastpy fixture build
needs -- fastpy, which is this repository's submodule `toolchain/fastpy`, and
zig -- and the llvmlite installed for the Python that runs it, whose `ir`
package is copied rather than imported.

usage:
    python scripts/fastpy-slateos-bundle.py [--fastpy DIR] [--out DIR]
                                            [--llvmlite DIR] [--python-version X.Y]
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import os
import py_compile
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path
from types import ModuleType

import gitenv

REPO = Path(__file__).resolve().parent.parent

#: Where the tree is assembled unless `--out` says otherwise: the gitignored
#: shelf the rootfs recipe already stages spike artifacts from (`build/spike/`).
DEFAULT_OUT = REPO / "build" / "fastpy-slateos"

#: The image's CPython. `scripts/cpython-spike/run.sh` builds 3.12 (a cross
#: build needs a build interpreter of the same version, and WSL's is 3.12),
#: and the rootfs recipe stages its standard library as `python312.zip`.
DEFAULT_PYTHON_VERSION = "3.12"

#: fastpy, a submodule pinned to the commit the OS is tested with: what the
#: tree is built from unless `--fastpy` or `$FASTPY_DIR` names another
#: checkout. `ctest-fixtures.py::_fastpy_dir` searches the same way, so the
#: fixtures and the image's compiler come from one checkout.
SUBMODULE = REPO / "toolchain" / "fastpy"

#: The image paths, relative to the tree's root.
LIB_DIR = Path("usr/lib/x86_64-slateos")
FASTPY_HOME = Path("usr/lib/fastpy")
COMMAND = Path("bin/fastpy")
MARKER = FASTPY_HOME / "BUNDLE"

#: The archive name fastpy looks for on SlateOS (`SLATEOS_RUNTIME_ARCHIVE` in
#: its `compiler/toolchain.py`).
RUNTIME_ARCHIVE = "libfastpy_rt.a"

#: llvmlite's pure-Python half: the package's own modules, then `ir/`.
#: `utils.py` is not imported by `ir`, but it is all of the package outside
#: `binding/` and `tests/`, and a few lines -- shipped so the package is whole.
LLVMLITE_FILES = ("__init__.py", "_version.py", "utils.py")

LAUNCHER = """\
#!/bin/python3
# fastpy -- compile a Python program into a native SlateOS program.
#
# Installed by the OS repository's scripts/fastpy-slateos-bundle.py; the
# compiler lives in /usr/lib/fastpy. `fastpy --help` lists its options.
import sys

sys.path.insert(0, "/usr/lib/fastpy")

from compiler.__main__ import main

sys.exit(main())
"""


class BundleError(Exception):
    """A reason the tree cannot be built, worded for the person running it."""


def find_fastpy(explicit: str | None) -> Path:
    """The fastpy checkout to bundle: the one `explicit` (`--fastpy`) or
    `$FASTPY_DIR` names, else the submodule. Nothing else is searched: a
    fastpy found beside the repository or on `$PYTHONPATH` is not the commit
    this tree pins, and would be bundled without anyone being told."""

    def usable(p: Path) -> bool:
        return (p / "compiler" / "__init__.py").is_file()

    named = explicit or os.environ.get("FASTPY_DIR")
    if named:
        # A named checkout that is wrong is reported, not fallen through: the
        # tree must come from the checkout the caller chose.
        if usable(Path(named)):
            return Path(named)
        raise BundleError(f"{named} has no compiler/__init__.py")
    if usable(SUBMODULE):
        return SUBMODULE
    raise BundleError(
        f"{SUBMODULE} is not checked out: run `git submodule update --init "
        "toolchain/fastpy` (scripts/bootstrap-worktree.sh does), or name "
        "another checkout with --fastpy or FASTPY_DIR"
    )


def pinned_commit() -> str | None:
    """The commit this tree pins `toolchain/fastpy` at (the index's, so a
    staged move of the pin counts), or None if git cannot say.

    Under WSL, where the rootfs recipe runs this, git cannot open a lane
    worktree by itself: its `.git` file names the git directory by a Windows
    path (`gitdir: C:/...`). That path is then tried again as WSL names it."""
    attempts: list[list[str]] = [[]]
    wsl_git_dir = _wsl_git_dir()
    if wsl_git_dir is not None:
        attempts.append(["--git-dir", wsl_git_dir, "--work-tree", str(REPO)])
    for extra in attempts:
        try:
            result = subprocess.run(
                ["git", "-C", str(REPO), *extra, "rev-parse", ":toolchain/fastpy"],
                capture_output=True, text=True, encoding="utf-8",
                env=gitenv.clean_env(),
            )
        except OSError:
            return None
        if result.returncode == 0 and result.stdout.strip():
            return result.stdout.strip()
    return None


def _wsl_git_dir() -> str | None:
    """REPO's git directory as WSL names it, when REPO's `.git` is a file
    naming it by a Windows path and `wslpath` is here to translate it; else
    None."""
    try:
        text = (REPO / ".git").read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        # A directory (a main checkout), or no `.git` at all.
        return None
    named = re.fullmatch(r"gitdir: ([A-Za-z]:[/\\].*)", text.strip())
    if named is None or shutil.which("wslpath") is None:
        return None
    try:
        result = subprocess.run(["wslpath", "-u", named.group(1)],
                                capture_output=True, text=True, encoding="utf-8")
    except OSError:
        return None
    path = result.stdout.strip()
    return path if result.returncode == 0 and path else None


def check_pin(checked_out: str, pinned: str | None) -> str | None:
    """Refuse a submodule that is not the compiler this tree pins.

    `checked_out` is `git_identity` of the submodule (`<commit>`, with
    `-dirty` for edits in `compiler/` or `runtime/`); `pinned` is
    `pinned_commit()`. A merge that moves the pin leaves the submodule where
    it was, and bundling it then would put an older compiler on the image
    than the one the fixtures are built with. Returns a line to print when
    the pin cannot be read, else None; raises BundleError when they differ.
    """
    if pinned is None:
        return (f"cannot read the commit this tree pins toolchain/fastpy at; "
                f"bundling it as checked out, at {checked_out}")
    if checked_out == pinned:
        return None
    if checked_out == f"{pinned}-dirty":
        raise BundleError(
            f"toolchain/fastpy has uncommitted edits in compiler/ or runtime/, "
            f"so it is not {pinned}, the commit this tree pins: undo them, or "
            "bundle the checkout they belong in with --fastpy or FASTPY_DIR"
        )
    raise BundleError(
        f"toolchain/fastpy is at {checked_out}, not {pinned}, the commit this "
        "tree pins: run `git submodule update toolchain/fastpy`"
    )


def find_llvmlite(explicit: str | None) -> Path:
    """llvmlite's package directory: `--llvmlite`, else the running Python's.
    Only its files are read; nothing of it is imported."""
    if explicit:
        path = Path(explicit)
    else:
        spec = importlib.util.find_spec("llvmlite")
        if spec is None or spec.origin is None:
            raise BundleError(
                "llvmlite is not importable by this Python; "
                "pass --llvmlite <its package directory>"
            )
        path = Path(spec.origin).parent
    if not (path / "ir" / "__init__.py").is_file():
        raise BundleError(f"{path} is not llvmlite's package (no ir/__init__.py)")
    return path


def load_toolchain(fastpy: Path) -> ModuleType:
    """fastpy's `compiler/toolchain.py`, loaded from `fastpy` under a private
    name. Importing `compiler` would bind whichever checkout came first on
    `sys.path`; this binds the one chosen, and the module imports nothing of
    its package, so it loads alone."""
    path = fastpy / "compiler" / "toolchain.py"
    spec = importlib.util.spec_from_file_location("fastpy_bundle_toolchain", path)
    if spec is None or spec.loader is None:
        raise BundleError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    # Registered before it runs: `dataclasses` looks a class's module up in
    # sys.modules by name while it builds the class, and toolchain.py has
    # dataclasses. The private name cannot collide with `compiler`.
    sys.modules[spec.name] = module
    try:
        spec.loader.exec_module(module)
    except BaseException:
        del sys.modules[spec.name]
        raise
    if not hasattr(module, "build_slateos_runtime_archive"):
        raise BundleError(
            f"{path} has no build_slateos_runtime_archive: this fastpy predates "
            "running on SlateOS (0.3.0)"
        )
    return module


def llvmlite_version(package: Path) -> str:
    """The version `llvmlite/_version.py` records, read without importing it:
    a release's copy, generated by versioneer, assigns
    `version_version = '0.47.0'`."""
    text = (package / "_version.py").read_text(encoding="utf-8")
    for line in text.splitlines():
        name, sep, value = line.partition("=")
        if sep and name.strip() == "version_version":
            return value.strip().strip("'\"")
    return "unknown"


def fastpy_version(fastpy: Path) -> str:
    """fastpy's `version`, from its `pyproject.toml`."""
    try:
        with open(fastpy / "pyproject.toml", "rb") as f:
            project = tomllib.load(f).get("project", {})
    except (OSError, tomllib.TOMLDecodeError):
        return "unknown"
    return str(project.get("version", "unknown"))


def git_identity(path: Path) -> str:
    """`<commit>`, `<commit>-dirty` when `compiler/` or `runtime/` has edits
    the commit lacks, or `unknown` outside a checkout. Recorded in `BUNDLE`,
    and held to the pin by `check_pin` when the checkout is the submodule."""
    # A checkout has `.git` -- a directory, or a file in a worktree. Without
    # one there is nothing to ask, and no reason to start git to be told so.
    if not (path / ".git").exists():
        return "unknown"
    try:
        head = subprocess.run(
            ["git", "-C", str(path), "rev-parse", "HEAD"],
            capture_output=True, text=True, encoding="utf-8", check=True,
            env=gitenv.clean_env(),
        ).stdout.strip()
        dirty = subprocess.run(
            ["git", "-C", str(path), "status", "--porcelain",
             "--untracked-files=no", "--", "compiler", "runtime"],
            capture_output=True, text=True, encoding="utf-8", check=True,
            env=gitenv.clean_env(),
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return "unknown"
    return f"{head}-dirty" if dirty else head


def clear_output(out: Path) -> None:
    """Empty `out` for a fresh tree, which stale files from an earlier one
    would otherwise join on the image. Only a directory this script made
    (it holds `BUNDLE`) or an empty one is cleared: `--out` pointed at the
    wrong place must cost an error, not a directory."""
    if not out.exists():
        return
    if not out.is_dir():
        raise BundleError(f"{out} exists and is not a directory")
    if (out / MARKER).is_file() or not any(out.iterdir()):
        shutil.rmtree(out)
        return
    raise BundleError(
        f"{out} is not empty and holds no {MARKER.as_posix()}: it is not a tree "
        "this script made, so it is left alone; name an empty or new directory"
    )


def copy_files(source: Path, dest: Path, names: list[str]) -> None:
    """Copy each of `names` from `source` into `dest`."""
    dest.mkdir(parents=True, exist_ok=True)
    for name in names:
        shutil.copy2(source / name, dest / name)


def byte_compile(home: Path, version: str) -> str:
    """Write checked-hash pycs for CPython `version` under `home`; return one
    line saying what was done."""
    running = f"{sys.version_info[0]}.{sys.version_info[1]}"
    if running == version:
        failed = []
        for source in sorted(home.rglob("*.py")):
            try:
                py_compile.compile(
                    str(source), doraise=True,
                    invalidation_mode=py_compile.PycInvalidationMode.CHECKED_HASH,
                )
            except py_compile.PyCompileError as err:
                failed.append(f"{source}: {err.msg}")
        if failed:
            raise BundleError("byte-compiling failed:\n" + "\n".join(failed))
        return f"byte-compiled for Python {version} by this interpreter"
    other = shutil.which(f"python{version}")
    if other:
        result = subprocess.run(
            [other, "-m", "compileall", "-q", "--invalidation-mode",
             "checked-hash", str(home)],
            capture_output=True, text=True, encoding="utf-8", errors="replace",
        )
        if result.returncode != 0:
            raise BundleError(
                f"{other} -m compileall failed:\n{result.stdout}{result.stderr}"
            )
        return f"byte-compiled for Python {version} by {other}"
    return (
        f"NOT byte-compiled: no Python {version} here (this is {running}); "
        "the image compiles the sources on first import"
    )


def build(fastpy: Path, llvmlite: Path, out: Path, version: str,
          toolchain: ModuleType) -> str:
    """Assemble the tree in `out`; return the byte-compile line."""
    clear_output(out)

    archive = Path(toolchain.build_slateos_runtime_archive(
        out / LIB_DIR / RUNTIME_ARCHIVE))

    home = out / FASTPY_HOME
    compiler = fastpy / "compiler"
    copy_files(compiler, home / "compiler",
               sorted(p.name for p in compiler.glob("*.py")))
    copy_files(llvmlite, home / "llvmlite", list(LLVMLITE_FILES))
    copy_files(llvmlite / "ir", home / "llvmlite" / "ir",
               sorted(p.name for p in (llvmlite / "ir").glob("*.py")))

    compiled = byte_compile(home, version)

    command = out / COMMAND
    command.parent.mkdir(parents=True, exist_ok=True)
    command.write_bytes(LAUNCHER.encode("ascii"))
    # The mode a POSIX host keeps; NTFS has no execute bit, so the rootfs
    # recipe sets it again when it stages the file.
    command.chmod(0o755)

    # Written last: its presence is what marks the tree complete, and what
    # lets the next run clear it.
    fields = (
        ("fastpy", git_identity(fastpy)),
        ("fastpy-version", fastpy_version(fastpy)),
        ("llvmlite", llvmlite_version(llvmlite)),
        ("python", version),
        ("pyc", compiled),
        (RUNTIME_ARCHIVE, hashlib.sha256(archive.read_bytes()).hexdigest()),
    )
    (out / MARKER).write_bytes(
        "".join(f"{key}: {value}\n" for key, value in fields).encode("utf-8")
    )
    return compiled


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="fastpy-slateos-bundle",
        description="Build the tree that puts the fastpy compiler on a SlateOS image.",
    )
    parser.add_argument("--fastpy", help="the fastpy checkout (default: $FASTPY_DIR, else toolchain/fastpy)")
    parser.add_argument("--llvmlite", help="llvmlite's package directory (default: this Python's)")
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT,
                        help="where to assemble the tree (default: build/fastpy-slateos)")
    parser.add_argument("--python-version", default=DEFAULT_PYTHON_VERSION,
                        help="the image's CPython, for the pycs (default: %(default)s)")
    args = parser.parse_args(argv)
    try:
        fastpy = find_fastpy(args.fastpy)
        if fastpy == SUBMODULE:
            note = check_pin(git_identity(fastpy), pinned_commit())
            if note is not None:
                print(f"fastpy-bundle: {note}")
        else:
            print(f"fastpy-bundle: using {fastpy}, not the pinned toolchain/fastpy")
        llvmlite = find_llvmlite(args.llvmlite)
        compiled = build(fastpy, llvmlite, args.out, args.python_version,
                         load_toolchain(fastpy))
    except (BundleError, RuntimeError, OSError) as err:
        # RuntimeError is fastpy's own report of a failed runtime build (no
        # zig, a compile error); OSError a copy that could not be made.
        print(f"fastpy-bundle: {err}", file=sys.stderr)
        return 1
    print(f"fastpy-bundle: {args.out}")
    print(f"fastpy-bundle: {compiled}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
