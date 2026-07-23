#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-minishell` SlateOS utility.

This produces `fastpy-minishell.elf`, a native SlateOS (`x86_64-slateos`)
binary compiled by fastpy (AOT Python -> LLVM IR -> native).  It is the
first *composition* step of the fastpy shell I/O-plumbing work: where the
`fastpy-run`/`-forkexec`/`-capture`/`-pipeline`/`-redirect`/`-inredirect`
utilities each proved a single primitive by calling it directly, this one
**parses a command line** and dispatches it — a real (if minimal) shell.

It accepts one argument, a whole command line as a single string, and
supports the *simple-command* grammar:

    cmd [args...] [< infile] [> outfile]

i.e. a command with arguments, optional input redirection (`< file`,
pointing the command's stdin at a file), and optional output redirection
(`> file`, pointing the command's stdout at a file, O_TRUNC).  Pipelines
(`|`) are intentionally out of scope for this first increment; they are a
natural follow-up (the two-stage plumbing is already proven by
`fastpy-pipeline`).

The shell:
  1. Tokenises the line on whitespace (`str.split()`).
  2. Walks the tokens, separating the command's own argv from the `<` / `>`
     redirection operands.
  3. Resolves argv[0]: an absolute path (one starting with `/`) is used
     directly; otherwise it is searched over a fixed PATH
     (`['/nonexistent/bin', '/mnt/bin']`).
  4. `os.fork()`s; in the child, applies the redirections with
     `os.open()` + `os.dup2()` (stdin<-infile, stdout->outfile) so they
     survive the following `os.execv()` (exercising the exec fd-table
     preservation fix for both a readable and a writable FILE fd), then
     `os.execv()`s the resolved command.
  5. In the parent, `os.waitpid()`s and exits with the child's exit status
     (`os.WEXITSTATUS`), exactly as a shell propagates `$?`.

So `fastpy-minishell "cat /in > /out"` makes `cat` copy `/in` to `/out`
and the shell exits with cat's byte count; `fastpy-minishell "/prog < /in"`
runs `/prog` with its stdin redirected from `/in`.

Argv (supplied by the caller):
    argv[1] = the command line to parse and run (a single string)

Diagnostic exit codes:
   90 = empty command (no command word after parsing)
  100 = argv[0] resolved to nothing (absolute path missing, or PATH search
        matched nothing)
  101 = os.execv returned in the child (exec failed)
  110 = os.fork() returned -1
  111 = os.waitpid() returned pid -1
  112 = child did not exit normally (os.WIFEXITED false)
  120 = child's os.open of the output-redirect file (O_WRONLY|O_CREAT|O_TRUNC)
        failed
  122 = child's os.open of the input-redirect file (O_RDONLY) failed
  (any other value = the executed command's own exit status)

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-minishell/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# O_WRONLY(1) | O_CREAT(0o100=64) | O_TRUNC(0o1000=512) = 577; O_RDONLY = 0.
# mode 0o644 = 420. Parser modes: 0=argv word, 1=next token is infile, 2=next
# token is outfile.
SRC = (
    "import sys\n"
    "import os\n"
    "line = sys.argv[1]\n"
    "toks = line.split()\n"
    "argv = []\n"
    "infile = ''\n"
    "outfile = ''\n"
    "mode = 0\n"
    # `cmd` mirrors argv[0] but is captured directly from the (concrete-`str`)
    # loop variable: indexing an append-built list yields a flexible value whose
    # method calls (`.find`) would need the CPython bridge, unavailable in
    # pure-native mode. Capturing the command word from `t` keeps it a plain str.
    "cmd = ''\n"
    "for t in toks:\n"
    "    if t == '<':\n"
    "        mode = 1\n"
    "    elif t == '>':\n"
    "        mode = 2\n"
    "    else:\n"
    "        if mode == 1:\n"
    "            infile = t\n"
    "            mode = 0\n"
    "        elif mode == 2:\n"
    "            outfile = t\n"
    "            mode = 0\n"
    "        else:\n"
    "            if cmd == '':\n"
    "                cmd = t\n"
    "            argv.append(t)\n"
    "if len(argv) == 0:\n"
    "    sys.exit(90)\n"
    # Resolve argv[0]: absolute (leading '/') used directly, else PATH search.
    "found = ''\n"
    "if cmd.find('/') == 0:\n"
    "    if os.path.exists(cmd):\n"
    "        found = cmd\n"
    "else:\n"
    "    PATH = ['/nonexistent/bin', '/mnt/bin']\n"
    "    for d in PATH:\n"
    "        p = d + '/' + cmd\n"
    "        if os.path.exists(p):\n"
    "            found = p\n"
    "if found == '':\n"
    "    sys.exit(100)\n"
    "pid = os.fork()\n"
    "if pid < 0:\n"
    "    sys.exit(110)\n"
    "if pid == 0:\n"
    # Child: apply redirections (they survive the exec), then become the command.
    "    if infile != '':\n"
    "        ifd = os.open(infile, 0, 0)\n"
    "        if ifd < 0:\n"
    "            sys.exit(122)\n"
    "        os.dup2(ifd, 0)\n"
    "        os.close(ifd)\n"
    "    if outfile != '':\n"
    "        ofd = os.open(outfile, 577, 420)\n"
    "        if ofd < 0:\n"
    "            sys.exit(120)\n"
    "        os.dup2(ofd, 1)\n"
    "        os.close(ofd)\n"
    "    os.execv(found, argv)\n"
    "    sys.exit(101)\n"
    # Parent: reap the child and propagate its exit status, as a shell sets $?.
    "rpid, status = os.waitpid(pid, 0)\n"
    "if rpid < 0:\n"
    "    sys.exit(111)\n"
    "if not os.WIFEXITED(status):\n"
    "    sys.exit(112)\n"
    "sys.exit(os.WEXITSTATUS(status))\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-minishell.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
