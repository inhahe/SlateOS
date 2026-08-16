#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-minishell` SlateOS utility.

This produces `fastpy-minishell.elf`, a native SlateOS (`x86_64-slateos`)
binary compiled by fastpy (AOT Python -> LLVM IR -> native).  It is the
*composition* step of the fastpy shell I/O-plumbing work: where the
`fastpy-run`/`-forkexec`/`-capture`/`-pipeline`/`-redirect`/`-inredirect`
utilities each proved a single primitive by calling it directly, this one
**parses a command line** and dispatches it — a real (if minimal) shell.

It accepts one argument, a whole command line as a single string, and
supports pipelines of simple commands:

    cmd [args...] [< infile] [> outfile | >> outfile] | cmd2 [args...] ... | ...

i.e. one or more stages separated by `|`, where each stage is a command
with arguments and optional per-stage input redirection (`< file`,
pointing the stage's stdin at a file) and output redirection: `> file`
(O_TRUNC, truncate) or `>> file` (O_APPEND, append), pointing the stage's
stdout at a file.  A single stage with no `|` is the simple-command case.
Adjacent stages are connected by a pipe (`os.pipe()`): stage i's stdout
feeds stage i+1's stdin, unless overridden by an explicit `>`/`>>`/`<` on
that stage (an explicit redirection wins over the pipe wiring, exactly as
in a POSIX shell).

The shell:
  1. Tokenises the line on whitespace (`str.split()`).
  2. Splits the tokens into pipeline *stages* on `|`.
  3. For each stage: separates the command's own argv from the `<` / `>`
     redirection operands, and resolves argv[0] — an absolute path (one
     starting with `/`) is used directly; otherwise it is searched over a
     fixed PATH (`['/nonexistent/bin', '/mnt/bin']`).
  4. Creates the inter-stage pipe (for all but the last stage), then
     `os.fork()`s the stage.  In the child it wires stdin/stdout with
     `os.dup2()`: stdin <- explicit infile, else the previous stage's pipe;
     stdout -> explicit outfile, else this stage's pipe to the next.  These
     redirections survive the following `os.execv()` (exercising the exec
     fd-table preservation fix for both FILE and PIPE fds on both standard
     streams), then it `os.execv()`s the resolved command.
  5. After forking all stages, the parent `os.waitpid()`s each and exits
     with the LAST stage's exit status (`os.WEXITSTATUS`), exactly as a
     shell reports a pipeline's status as its last command's `$?`.

So `fastpy-minishell "cat /in > /out"` makes `cat` copy `/in` to `/out`;
`fastpy-minishell "/prog < /in"` runs `/prog` with stdin from `/in`; and
`fastpy-minishell "cat /in | wc"` pipes `cat`'s output into `wc`.

Argv (supplied by the caller):
    argv[1] = the command line to parse and run (a single string)

Diagnostic exit codes:
   90 = empty command in some stage (no command word after parsing)
  100 = a stage's argv[0] resolved to nothing (absolute path missing, or
        PATH search matched nothing)
  101 = os.execv returned in a child (exec failed)
  110 = os.fork() returned -1
  111 = os.waitpid() returned pid -1
  112 = a child did not exit normally (os.WIFEXITED false)
  120 = a child's os.open of an output-redirect file
        (O_WRONLY|O_CREAT|O_TRUNC) failed
  122 = a child's os.open of an input-redirect file (O_RDONLY) failed
  130 = os.pipe() failed to create an inter-stage pipe
  (any other value = the last stage command's own exit status)

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-minishell/build.py

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
    # Append a sentinel '|' so the final stage is flushed by the SAME code path
    # as every internal pipe boundary. We copy into a flat append-built list and
    # never build a list-of-lists: iterating an element of a nested (append-built)
    # list yields a flexible value whose iteration needs the CPython bridge, which
    # is absent in pure-native mode and spins forever. A single flat forward pass
    # sidesteps that entirely — every construct here (flat append list, `cmd`
    # from the concrete loop variable, PATH iteration, `argv` handed to execv) is
    # exactly what the proven simple-command version used.
    "toks2 = []\n"
    "for t in toks:\n"
    "    toks2.append(t)\n"
    "toks2.append('|')\n"
    # nstages = count of '|' in toks2 (original pipe count + the one sentinel).
    "nstages = 0\n"
    "for t in toks2:\n"
    "    if t == '|':\n"
    "        nstages = nstages + 1\n"
    "pids = []\n"
    # Read end of the pipe feeding the current stage's stdin (-1 = none/inherit).
    "prev_read = -1\n"
    "si = 0\n"
    # Per-stage accumulators, reset after each flush.
    "cmd = ''\n"
    "argv = []\n"
    "infile = ''\n"
    "outfile = ''\n"
    # outmode: 0 = truncate ('>'), 1 = append ('>>'). Chosen by the redirect
    # operator, so it is fixed when the operator token is seen (not the operand).
    "outmode = 0\n"
    "mode = 0\n"
    # Single forward pass: accumulate a stage's argv + redirections; at each '|'
    # boundary (including the trailing sentinel) resolve, pipe, and fork it, then
    # reset for the next stage.
    "for t in toks2:\n"
    "    if t == '<':\n"
    "        mode = 1\n"
    "    elif t == '>':\n"
    "        mode = 2\n"
    "        outmode = 0\n"
    "    elif t == '>>':\n"
    "        mode = 2\n"
    "        outmode = 1\n"
    "    elif t == '|':\n"
    # --- Flush the accumulated stage ---
    "        if len(argv) == 0:\n"
    "            sys.exit(90)\n"
    # Resolve cmd: absolute (leading '/') used directly, else PATH search. `cmd`
    # was captured from the concrete-`str` loop variable, so `.find` is a plain
    # str method (no CPython bridge needed).
    "        found = ''\n"
    "        if cmd.find('/') == 0:\n"
    "            if os.path.exists(cmd):\n"
    "                found = cmd\n"
    "        else:\n"
    "            PATH = ['/nonexistent/bin', '/mnt/bin']\n"
    "            for d in PATH:\n"
    "                pth = d + '/' + cmd\n"
    "                if os.path.exists(pth):\n"
    "                    found = pth\n"
    "        if found == '':\n"
    "            sys.exit(100)\n"
    # Create this stage's pipe to the NEXT stage (all but the last).
    "        pr = -1\n"
    "        pw = -1\n"
    "        if si < nstages - 1:\n"
    "            pr, pw = os.pipe()\n"
    "            if pr < 0:\n"
    "                sys.exit(130)\n"
    "        pid = os.fork()\n"
    "        if pid < 0:\n"
    "            sys.exit(110)\n"
    "        if pid == 0:\n"
    # Child. stdin: explicit infile wins, else the previous stage's pipe.
    "            if infile != '':\n"
    "                ifd = os.open(infile, 0, 0)\n"
    "                if ifd < 0:\n"
    "                    sys.exit(122)\n"
    "                os.dup2(ifd, 0)\n"
    "                os.close(ifd)\n"
    "            elif prev_read >= 0:\n"
    "                os.dup2(prev_read, 0)\n"
    # stdout: explicit outfile wins, else this stage's pipe to the next.
    "            if outfile != '':\n"
    # O_WRONLY|O_CREAT|O_TRUNC = 577 (truncate) vs O_WRONLY|O_CREAT|O_APPEND =
    # 1089 (append). if/else, not a ternary (pure-mode codegen keeps it simple).
    "                oflags = 577\n"
    "                if outmode == 1:\n"
    "                    oflags = 1089\n"
    "                ofd = os.open(outfile, oflags, 420)\n"
    "                if ofd < 0:\n"
    "                    sys.exit(120)\n"
    "                os.dup2(ofd, 1)\n"
    "                os.close(ofd)\n"
    "            elif pw >= 0:\n"
    "                os.dup2(pw, 1)\n"
    # Drop the raw pipe fds in the child (their duplicates on 0/1 remain).
    "            if prev_read >= 0:\n"
    "                os.close(prev_read)\n"
    "            if pr >= 0:\n"
    "                os.close(pr)\n"
    "            if pw >= 0:\n"
    "                os.close(pw)\n"
    "            os.execv(found, argv)\n"
    "            sys.exit(101)\n"
    # Parent: record the pid, close fds it no longer needs, advance the pipe.
    "        pids.append(pid)\n"
    "        if prev_read >= 0:\n"
    "            os.close(prev_read)\n"
    "        if pw >= 0:\n"
    "            os.close(pw)\n"
    "        prev_read = pr\n"
    "        si = si + 1\n"
    # Reset accumulators for the next stage. (argv is safely rebound to a fresh
    # list here; the child already execv'd its own copy above.)
    "        cmd = ''\n"
    "        argv = []\n"
    "        infile = ''\n"
    "        outfile = ''\n"
    "        outmode = 0\n"
    "        mode = 0\n"
    "    else:\n"
    # An ordinary word: a redirection operand (per pending mode) or an argv word.
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
    # Reap every stage; a pipeline's status is its LAST command's exit status.
    # Iterate `pids` by the concrete loop variable `p` (not `pids[pi]`): the pids
    # accumulate in stage order, so the value left in `last_status` after the
    # loop is the last stage's exit status.
    "last_status = 0\n"
    "for p in pids:\n"
    "    rpid, status = os.waitpid(p, 0)\n"
    "    if rpid < 0:\n"
    "        sys.exit(111)\n"
    "    if not os.WIFEXITED(status):\n"
    "        sys.exit(112)\n"
    "    last_status = os.WEXITSTATUS(status)\n"
    "sys.exit(last_status)\n"
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
