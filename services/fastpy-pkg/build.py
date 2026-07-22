#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-pkg` SlateOS utility.

This produces `fastpy-pkg.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): the **package manager
front-end** for SlateOS, built directly on top of the content-addressed store
primitive.  It is a real subcommand CLI over a persistent text registry
`/tmp/pkgdb.txt` whose records are `"<name> <digest> <deps>\n"`, where `<deps>`
is a comma-separated list of dependency package names, or `-` for none:

    pkg install <name> <payload-path> <deps>
        read the payload, compute its 32-bit FNV-1a digest, write it to the
        content-addressed blob /tmp/store-<digest>.blob, then upsert the
        "<name> <digest> <deps>" record into the registry (replacing any prior
        record for the same name).  `<deps>` is a comma list or `-`.  Prints
        "installed <name> <digest> <deps>", exit 0.

    pkg remove <name>
        drop the named record from the registry (read-modify-write).  Prints
        "removed <name>" + exit 0 if it was present, else "not found <name>" +
        exit 1.

    pkg query <name>
        resolve the named record and print its digest (exit 0), or
        "not found <name>" (exit 1).

    pkg deps <name>
        print the named record's dependency field (exit 0), or
        "not found <name>" (exit 1).

    pkg check <name>
        verify every dependency of <name> is itself installed in the registry.
        Prints "ok <name>" + exit 0 if all deps are present, else
        "missing <dep>" + exit 1 (or "not found <name>" + exit 1 if <name>
        itself is not installed).  This is the dependency-resolution primitive
        a real package manager needs before an install/upgrade is allowed.

    pkg list
        print every record in the registry, exit 0.

The registry file must already exist (the installer / a package-manager bootstrap
creates an empty one); each subcommand reads it, and install/remove rewrite it.

Where `fastpy-store` proved a single content round-trip and the earlier registry
proved a persistent name->content mapping, this adds **dependency records and
dependency verification**: `check` resolves each of a package's declared deps
against the registry, exactly the gate a package manager applies before allowing
an install/upgrade.

Pure-mode caveats honored (see fastpy `known-issues.md`):
  * Every helper that subscripts a string takes a `str`-annotated parameter
    (`fnv1a(s: str)`, `to_hex8(v: int)`, `field(line: str, idx: int)`,
    `find_line(db: str, name: str)`, `db_remove(db: str, name: str)`,
    `db_list(db: str)`) so `s[i]` lowers to the native `fastpy_str_index` rather
    than the CPython object-subscript bridge (BUG-SUBSCRIPT-UNTYPED-PARAM-BRIDGE).
  * The hash is 32-bit (`mask = 0xFFFFFFFF`, prime = 16777619) so every
    intermediate stays inside signed i64 — no bigint (BUG-BIGINT-MUL-INT-NULL).
  * Subcommand dispatch and name matching use `==` on strings, lowered to the
    native `fastpy_str_compare` (strcmp); character classification uses
    `ord(...)` integer compares (newline = 10, space = 32, comma = 44).  All
    pure-mode-safe.
  * File reads are inline (never returned through a user function), sidestepping
    BUG-FILEREAD-FN-RETTAG.

Reference digests (32-bit FNV-1a, verified against CPython):
    "libc demo\n"      -> 86732e22
    "coreutils demo\n" -> 1ee068f8
    "grep demo\n"      -> 0f4143a6

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and drives it through a full dependency lifecycle in
the ring-3 self-test (`self_test_fastpy_slateos_pkg`): install a chain
(libc <- coreutils <- grep), `check` that deps resolve, remove a base
dependency, and `check` again to confirm the now-missing dep is detected.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-pkg/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# Package manager front-end: a subcommand CLI (install/remove/query/deps/check/
# list) over a persistent text registry /tmp/pkgdb.txt of "<name> <digest>
# <deps>" records, built on the content-addressed store.  32-bit FNV (no
# bigint); every string-subscripting helper is type-annotated.
SRC = (
    "import sys\n"
    "def fnv1a(s: str) -> int:\n"
    "    h = 2166136261\n"
    "    mask = 4294967295\n"
    "    i = 0\n"
    "    n = len(s)\n"
    "    while i < n:\n"
    "        c = ord(s[i])\n"
    "        h = (h ^ c) & mask\n"
    "        h = (h * 16777619) & mask\n"
    "        i = i + 1\n"
    "    return h\n"
    "def to_hex8(v: int) -> str:\n"
    "    out = ''\n"
    "    i = 0\n"
    "    while i < 8:\n"
    "        shift = (7 - i) * 4\n"
    "        nib = (v >> shift) & 15\n"
    "        if nib < 10:\n"
    "            out = out + chr(48 + nib)\n"
    "        else:\n"
    "            out = out + chr(87 + nib)\n"
    "        i = i + 1\n"
    "    return out\n"
    # Return the idx-th space-delimited field of a record line ('' if absent).
    "def field(line: str, idx: int) -> str:\n"
    "    m = len(line)\n"
    "    j = 0\n"
    "    cur = 0\n"
    "    out = ''\n"
    "    while j < m:\n"
    "        ch = ord(line[j])\n"
    "        if ch == 32:\n"
    "            if cur == idx:\n"
    "                return out\n"
    "            cur = cur + 1\n"
    "            out = ''\n"
    "        else:\n"
    "            out = out + line[j]\n"
    "        j = j + 1\n"
    "    if cur == idx:\n"
    "        return out\n"
    "    return ''\n"
    # Return the whole record line whose name (field 0) == name ('' if absent).
    "def find_line(db: str, name: str) -> str:\n"
    "    n = len(db)\n"
    "    i = 0\n"
    "    line = ''\n"
    "    while i <= n:\n"
    "        if i == n or ord(db[i]) == 10:\n"
    "            if len(line) > 0:\n"
    "                if field(line, 0) == name:\n"
    "                    return line\n"
    "            line = ''\n"
    "        else:\n"
    "            line = line + db[i]\n"
    "        i = i + 1\n"
    "    return ''\n"
    # Rewrite db dropping the record whose name (field 0) == name.
    "def db_remove(db: str, name: str) -> str:\n"
    "    n = len(db)\n"
    "    i = 0\n"
    "    line = ''\n"
    "    out = ''\n"
    "    while i <= n:\n"
    "        if i == n or ord(db[i]) == 10:\n"
    "            if len(line) > 0:\n"
    "                if field(line, 0) != name:\n"
    "                    out = out + line + chr(10)\n"
    "            line = ''\n"
    "        else:\n"
    "            line = line + db[i]\n"
    "        i = i + 1\n"
    "    return out\n"
    "def db_list(db: str) -> int:\n"
    "    n = len(db)\n"
    "    i = 0\n"
    "    line = ''\n"
    "    count = 0\n"
    "    while i <= n:\n"
    "        if i == n or ord(db[i]) == 10:\n"
    "            if len(line) > 0:\n"
    "                print(line)\n"
    "                count = count + 1\n"
    "            line = ''\n"
    "        else:\n"
    "            line = line + db[i]\n"
    "        i = i + 1\n"
    "    return count\n"
    # Return the name of a missing dependency of `deps` ('' if all satisfied).
    "def missing_dep(db: str, deps: str) -> str:\n"
    "    if deps == '-':\n"
    "        return ''\n"
    "    m = len(deps)\n"
    "    j = 0\n"
    "    dep = ''\n"
    "    miss = ''\n"
    "    while j <= m:\n"
    "        if j == m or ord(deps[j]) == 44:\n"
    "            if len(dep) > 0:\n"
    "                if len(find_line(db, dep)) == 0:\n"
    "                    miss = dep\n"
    "            dep = ''\n"
    "        else:\n"
    "            dep = dep + deps[j]\n"
    "        j = j + 1\n"
    "    return miss\n"
    "cmd = sys.argv[1]\n"
    "db_path = '/tmp/pkgdb.txt'\n"
    "if cmd == 'install':\n"
    "    name = sys.argv[2]\n"
    "    src_path = sys.argv[3]\n"
    "    deps = sys.argv[4]\n"
    "    f = open(src_path, 'r')\n"
    "    payload = f.read()\n"
    "    f.close()\n"
    "    digest = to_hex8(fnv1a(payload))\n"
    "    store_path = '/tmp/store-' + digest + '.blob'\n"
    "    f = open(store_path, 'w')\n"
    "    f.write(payload)\n"
    "    f.close()\n"
    "    f = open(db_path, 'r')\n"
    "    db = f.read()\n"
    "    f.close()\n"
    "    db = db_remove(db, name)\n"
    "    db = db + name + ' ' + digest + ' ' + deps + chr(10)\n"
    "    f = open(db_path, 'w')\n"
    "    f.write(db)\n"
    "    f.close()\n"
    "    print('installed ' + name + ' ' + digest + ' ' + deps)\n"
    "    sys.exit(0)\n"
    "if cmd == 'remove':\n"
    "    name = sys.argv[2]\n"
    "    f = open(db_path, 'r')\n"
    "    db = f.read()\n"
    "    f.close()\n"
    "    line = find_line(db, name)\n"
    "    db = db_remove(db, name)\n"
    "    f = open(db_path, 'w')\n"
    "    f.write(db)\n"
    "    f.close()\n"
    "    if len(line) > 0:\n"
    "        print('removed ' + name)\n"
    "        sys.exit(0)\n"
    "    print('not found ' + name)\n"
    "    sys.exit(1)\n"
    "if cmd == 'query':\n"
    "    name = sys.argv[2]\n"
    "    f = open(db_path, 'r')\n"
    "    db = f.read()\n"
    "    f.close()\n"
    "    line = find_line(db, name)\n"
    "    if len(line) > 0:\n"
    "        print(field(line, 1))\n"
    "        sys.exit(0)\n"
    "    print('not found ' + name)\n"
    "    sys.exit(1)\n"
    "if cmd == 'deps':\n"
    "    name = sys.argv[2]\n"
    "    f = open(db_path, 'r')\n"
    "    db = f.read()\n"
    "    f.close()\n"
    "    line = find_line(db, name)\n"
    "    if len(line) > 0:\n"
    "        print(field(line, 2))\n"
    "        sys.exit(0)\n"
    "    print('not found ' + name)\n"
    "    sys.exit(1)\n"
    "if cmd == 'check':\n"
    "    name = sys.argv[2]\n"
    "    f = open(db_path, 'r')\n"
    "    db = f.read()\n"
    "    f.close()\n"
    "    line = find_line(db, name)\n"
    "    if len(line) == 0:\n"
    "        print('not found ' + name)\n"
    "        sys.exit(1)\n"
    "    miss = missing_dep(db, field(line, 2))\n"
    "    if len(miss) > 0:\n"
    "        print('missing ' + miss)\n"
    "        sys.exit(1)\n"
    "    print('ok ' + name)\n"
    "    sys.exit(0)\n"
    "if cmd == 'list':\n"
    "    f = open(db_path, 'r')\n"
    "    db = f.read()\n"
    "    f.close()\n"
    "    count = db_list(db)\n"
    "    print('total ' + str(count))\n"
    "    sys.exit(0)\n"
    "print('usage: pkg install|remove|query|deps|check|list')\n"
    "sys.exit(2)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-pkg.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
