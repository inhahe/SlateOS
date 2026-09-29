#!/usr/bin/env python3
"""Refuse a C prototype that disagrees with its Rust definition in `libc.a`.

That is, whose calling convention -- which registers its arguments and result
travel in, and how wide they are -- is not the definition's.

Why
---
C here is compiled against musl's headers (`zig cc --target=x86_64-linux-musl`)
and linked against this tree's `libc.a`, whose functions are Rust. The linker
matches them by name alone. If the header says one thing and the definition
another -- a `timer_t` eight bytes wide in the header and four in the
definition, an `int` return declared where the definition returns nothing --
the program links, runs, and reads a register nobody set. On 2026-09-29 the
first run of this comparison found eleven: `__fpurge` returned nothing where
musl's `<stdio_ext.h>` says `int`; `readahead` returned 32 bits of an
`ssize_t`; `wctype`, `wctrans` and their `_l` forms returned 32 bits of a
64-bit handle, and `iswctype`, `towctrans` and theirs took them back as 32;
`timer_create` stored four bytes into the caller's eight-byte `timer_t`, and
the four other `timer_*` functions read four. `check-libc-declared.py` asks
whether each declared function exists; nothing asked whether it takes and
returns what its declaration says.

How
---
Each side is reduced to what the SysV x86-64 calling convention does with it
-- the arguments that go in integer registers, in order and with their widths;
those in SSE registers, in order; those on the stack (`long double`, a
`long double complex`, a large structure) in order; the return's class; and
whether the function is variadic -- and the two must be the same.

  C       every header under zig's `generic-musl` include directory, and the
          overlay `posix/include`, parsed by clang on its own with
          `_GNU_SOURCE`, `_BSD_SOURCE` and `_LARGEFILE64_SOURCE` (clang's
          JSON syntax tree: `zig cc -Xclang -ast-dump=json`). A `static
          inline` function is the header's own and is skipped.
  Rust    `posix/src`'s `extern "C" fn` definitions, `type` aliases resolved,
          with the export macros expanded first (`binary_exports!`'s
          positional fragments and bracketed repetition, the `export_*!`
          entry lists); the `long double` functions' assembly thunks, whose
          shape
          (`ld_c!(l_li "jnl" => ...)` and the `export_*!` blocks) is what the C
          caller sees; the variadic functions' `va_trampoline!`s, whose
          register-save offset and `va_list` register must match the number
          of fixed arguments the header declares; and the functions written
          wholly in assembly (`setjmp` and kin, the `ucontext` four), which
          have their own ring-3 tests and are listed, not compared.

A variadic function may be defined in Rust with its variable arguments as
further integer parameters (`open`'s `mode`): the caller passes them in the
same registers, so the fixed part must agree and the rest be integers.
`EXCEPTIONS` names each remaining difference and why it is harmless; the
gate fails on any other, on an exception that no longer differs (stale), and
on a declared function it could not find a definition of to compare, beyond
`UNCHECKED`, a ratchet.

Exit codes: 0 clean, 1 violation, 2 could not check (a parse found too little
to judge), 3 could not run (no zig), which `scripts/run-checker.sh` files as
skipped.

Usage
-----
    python scripts/check-libc-prototypes.py
    python scripts/check-libc-prototypes.py --self-test
"""
from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "posix" / "src"
OVERLAY = ROOT / "posix" / "include"
# The parsed declarations, keyed by everything they depend on: build output,
# regenerable, where `cargo clean` removes it.
CACHE = ROOT / "target" / "check-libc-prototypes.json"

# ---------------------------------------------------------------------------
# What is allowed to differ, and why
# ---------------------------------------------------------------------------

EXCEPTIONS: dict[str, str] = {
    "getdents": "returns 64 bits where musl declares `int`: the caller reads the low half, "
                "which is the whole value -- no read of a directory returns 2 GiB",
    "ioctl": "takes the request as 64 bits (glibc's `unsigned long`) where musl declares "
             "`int`, and reads only its low 32 (`ioctl.rs`): the rest of the register is "
             "whatever the caller left",
    "membarrier": "takes Linux's third argument, `cpu_id`, which musl does not declare: it is "
                  "read only with MEMBARRIER_CMD_FLAG_CPU, which only the RSEQ command accepts, "
                  "and that fails EINVAL here before the argument is used",
    "strfmon": "variadic in C; defined taking one `double` (`monetary.rs`): a variadic call "
               "passes it in %xmm0 as a fixed argument is passed, so one value formats",
    "strfmon_l": "as `strfmon`",
}

# Declared functions whose definition this reading cannot find, as of when
# the gate was written: only ever removed from.
UNCHECKED: frozenset[str] = frozenset()

# Declarations that are not the library's to define: `sys/soundcard.h`'s
# helper its macros expect the program to write, and MIPS cache control,
# which x86-64 has none of (as `check-libc-declared.py`'s NOT_FUNCTIONS).
NOT_LIBRARY = frozenset({"seqbuf_dump", "cachectl", "cacheflush", "_flush_cache"})

# Functions written wholly in assembly: the definition is not Rust, and their
# conventions are exercised by their own ring-3 tests.
ASSEMBLY = frozenset({
    "setjmp", "longjmp", "sigsetjmp", "__sigsetjmp", "siglongjmp", "_setjmp", "_longjmp",
    "getcontext", "setcontext", "swapcontext", "makecontext",
})

# By-value records, as the eightbytes the convention passes them in: a
# 16-byte pair of integers is two integer registers, a larger record goes on
# the stack (or, returned, through a hidden pointer), a complex double is two
# SSE registers.
RECORDS_C = {
    "div_t": ("i64",), "ldiv_t": ("i64", "i64"), "lldiv_t": ("i64", "i64"),
    "imaxdiv_t": ("i64", "i64"), "struct entry": ("i64", "i64"), "ENTRY": ("i64", "i64"),
    "union sigval": ("i64",), "struct in_addr": ("i32",),
    "struct mallinfo": ("mem:mallinfo",), "struct mallinfo2": ("mem:mallinfo2",),
    "cookie_io_functions_t": ("mem:cookie",), "struct _IO_cookie_io_functions_t": ("mem:cookie",),
}
RECORDS_RUST = {
    "DivT": ("i64",), "LdivT": ("i64", "i64"), "LldivT": ("i64", "i64"), "ImaxdivT": ("i64", "i64"),
    "Entry": ("i64", "i64"), "Sigval": ("i64",), "InAddr": ("i32",), "Semun": ("i64",),
    "Mallinfo": ("mem:mallinfo",), "Mallinfo2": ("mem:mallinfo2",), "CookieIoFunctions": ("mem:cookie",),
    "Complex64": ("f64", "f64"), "Complex32": ("cf32",), "i128": ("i64", "i64"), "u128": ("i64", "i64"),
}

# ---------------------------------------------------------------------------
# Classes
# ---------------------------------------------------------------------------

C_SCALARS = {
    "char": "i8", "_Bool": "i8", "bool": "i8", "short": "i16", "short int": "i16",
    "int": "i32", "long": "i64", "long int": "i64", "long long": "i64", "long long int": "i64",
    "float": "f32", "double": "f64", "long double": "x87",
    "_Complex float": "cf32", "float _Complex": "cf32",
    "_Complex double": ("f64", "f64"), "double _Complex": ("f64", "f64"),
    "_Complex long double": "cx87", "long double _Complex": "cx87",
    "__int128": ("i64", "i64"), "__uint128_t": ("i64", "i64"), "__int128_t": ("i64", "i64"),
}


def c_class(t: str, typedefs: dict[str, str], depth: int = 0):
    """A C type spelling -> a class token, or a tuple of them (a record)."""
    t = re.sub(r"__attribute__\(\(.*?\)\)", "", t)
    t = re.sub(r"\b(const|volatile|restrict|__restrict|_Nonnull|_Nullable|_Atomic)\b", "", t)
    t = re.sub(r"\s+", " ", t).strip()
    if t == "void":
        return "void"
    if t in RECORDS_C:
        return RECORDS_C[t]
    if "*" in t or "[" in t or "(" in t:
        return "i64"
    if t.startswith("enum "):
        return "i32"
    if depth < 12 and t in typedefs and typedefs[t] != t:
        return c_class(typedefs[t], typedefs, depth + 1)
    base = t.replace("unsigned ", "").replace("signed ", "").strip()
    if t in ("unsigned", "signed"):
        base = "int"
    if base in C_SCALARS:
        return C_SCALARS[base]
    return "?" + t


RUST_SCALARS = {
    "i8": "i8", "u8": "i8", "c_char": "i8", "c_schar": "i8", "c_uchar": "i8", "bool": "i8",
    "i16": "i16", "u16": "i16", "c_short": "i16", "c_ushort": "i16",
    "i32": "i32", "u32": "i32", "c_int": "i32", "c_uint": "i32",
    "i64": "i64", "u64": "i64", "isize": "i64", "usize": "i64", "c_long": "i64", "c_ulong": "i64",
    "c_longlong": "i64", "c_ulonglong": "i64",
    "f32": "f32", "f64": "f64", "!": "void", "()": "void", "": "void",
}


def rust_class(t: str, aliases: dict[str, str], depth: int = 0):
    t = re.sub(r"\s+", " ", t.strip())
    if t.startswith(("*", "&")) or "fn(" in t or t.startswith("Option<"):
        return "i64"
    base = t.split("::")[-1].strip()
    if base in RECORDS_RUST:
        return RECORDS_RUST[base]
    if base in RUST_SCALARS:
        return RUST_SCALARS[base]
    if depth < 12 and base in aliases:
        return rust_class(aliases[base], aliases, depth + 1)
    return "?" + t


def assignment(args, ret, variadic):
    """The convention's view of a signature: (integer registers, SSE
    registers, stack, return, variadic)."""
    ints, sses, stack = [], [], []
    for a in args:
        for part in (a if isinstance(a, tuple) else (a,)):
            if part in ("i8", "i16", "i32", "i64"):
                ints.append(part)
            elif part in ("f32", "f64", "cf32"):
                sses.append(part)
            else:
                stack.append(part)
    r = ret if isinstance(ret, tuple) else (ret,)
    return (tuple(ints), tuple(sses), tuple(stack), r, variadic)


def compatible(c, r) -> bool:
    """Whether the C and Rust assignments agree -- exactly, or as a variadic
    function defined with its variable arguments as further integers."""
    if c == r:
        return True
    ci, cs, cst, cr, cv = c
    ri, rs, rst, rr, rv = r
    return (cv and not rv and len(ri) >= len(ci) and ri[:len(ci)] == ci
            and cs == rs and cst == rst and cr == rr)


# ---------------------------------------------------------------------------
# The C side
# ---------------------------------------------------------------------------


def find_zig() -> str | None:
    z = os.environ.get("FASTPY_ZIG")
    if z and Path(z).exists():
        return z
    return shutil.which("zig")


def musl_include(zig: str) -> Path | None:
    inc = Path(zig).resolve().parent / "lib" / "libc" / "include" / "generic-musl"
    return inc if inc.is_dir() else None


def return_type(ftype: str) -> str:
    """The return type in a function type's spelling -- which for `void
    (*(int, void (*)(int)))(int)`, a function returning a pointer to a
    function, is found by the parameter list that closes the spelling."""
    ftype = re.sub(r"__attribute__\(\(.*?\)\)\s*$", "", ftype.strip()).strip()
    depth = 0
    for i in range(len(ftype) - 1, -1, -1):
        ch = ftype[i]
        if ch == ")":
            depth += 1
        elif ch == "(":
            depth -= 1
            if depth == 0:
                head = re.sub(r"__attribute__\(\(.*?\)\)", "", ftype[:i]).strip()
                return head or "int"
    return ftype


def declarations_in(ast: dict) -> dict[str, tuple]:
    """name -> (arg classes, return class, variadic) for the non-inline
    function declarations in one translation unit's syntax tree."""
    typedefs: dict[str, str] = {}
    for node in ast.get("inner", []):
        if node.get("kind") == "TypedefDecl":
            q = node.get("type", {})
            d = q.get("desugaredQualType")
            # `typedef enum ACTION {...} ACTION` desugars to itself; its
            # spelling is what says it is an enum.
            typedefs[node["name"]] = q.get("qualType", "") if not d or d == node["name"] else d
    out = {}
    for node in ast.get("inner", []):
        if node.get("kind") != "FunctionDecl" or node.get("isImplicit"):
            continue
        if node.get("inline") or node.get("storageClass") == "static":
            continue
        ftype = node["type"]["qualType"]
        args = []
        for p in node.get("inner", []):
            if p.get("kind") == "ParmVarDecl":
                q = p["type"]
                args.append(c_class(q.get("desugaredQualType") or q["qualType"], typedefs))
        ret = c_class(return_type(ftype), typedefs)
        params = ftype[len(return_type(ftype)):]
        variadic = "..." in params
        out.setdefault(node["name"], (tuple(args), ret, variadic))
    return out


def c_declarations(zig: str, inc: Path, overlay: Path | None) -> dict[str, tuple]:
    """name -> (args, ret, variadic, header) over every header: each parsed
    on its own (some pairs conflict), in parallel, and the result cached by a
    hash of every header's text, zig's version and the flags -- the headers
    change only with zig or the overlay, and parsing them all takes a minute."""
    headers: list[str] = []
    for base in [b for b in (overlay, inc) if b and b.is_dir()]:
        headers += [p.relative_to(base).as_posix() for p in sorted(base.rglob("*.h"))
                    if not p.relative_to(base).as_posix().startswith("bits/")]
    flags = ["-D_GNU_SOURCE", "-D_BSD_SOURCE", "-D_LARGEFILE64_SOURCE"]
    if overlay and overlay.is_dir():
        # -I, not -isystem: zig's driver searches its own libc headers before
        # any -isystem directory, which would hide the overlay's headers
        # behind the musl ones they extend.
        flags += ["-I", str(overlay)]
    key = hashlib.sha256()
    key.update(subprocess.run([zig, "version"], capture_output=True, text=True).stdout.encode())
    key.update(" ".join(flags).encode())
    for base in [b for b in (overlay, inc) if b and b.is_dir()]:
        for p in sorted(base.rglob("*.h")):
            key.update(p.relative_to(base).as_posix().encode())
            key.update(p.read_bytes())
    digest = key.hexdigest()
    try:
        cached = json.loads(CACHE.read_text(encoding="utf-8"))
        if cached.get("key") == digest:
            return {n: (tuple(tuple(a) if isinstance(a, list) else a for a in v[0]),
                        tuple(v[1]) if isinstance(v[1], list) else v[1], v[2], v[3])
                    for n, v in cached["decls"].items()}
    except (OSError, ValueError, KeyError):
        pass

    def parse(h: str) -> tuple[str, dict]:
        with tempfile.TemporaryDirectory() as t:
            src = Path(t) / "h.c"
            src.write_text(f"#include <{h}>\n", encoding="utf-8", newline="")
            r = subprocess.run([zig, "cc", "--target=x86_64-linux-musl", "-fsyntax-only",
                                "-Xclang", "-ast-dump=json", *flags, str(src)],
                               capture_output=True, text=True, encoding="utf-8", errors="replace",
                               timeout=300)
        try:
            return h, declarations_in(json.loads(r.stdout)) if r.stdout.strip() else {}
        except json.JSONDecodeError:
            return h, {}

    out: dict[str, tuple] = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=min(16, os.cpu_count() or 4)) as pool:
        results = dict(pool.map(parse, headers))
    for h in headers:  # in order, so the first header to declare a name keeps it
        for name, sig in results.get(h, {}).items():
            out.setdefault(name, sig + (h,))
    try:
        CACHE.parent.mkdir(parents=True, exist_ok=True)
        CACHE.write_text(json.dumps({"key": digest, "decls": out}), encoding="utf-8", newline="")
    except OSError:
        pass  # a cache that cannot be written is a slower next run, nothing more
    return out


# ---------------------------------------------------------------------------
# The Rust side
# ---------------------------------------------------------------------------

FN = re.compile(r'(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?extern\s+"C"\s+fn\s+([A-Za-z_]\w*)\s*\(')
ALIAS = re.compile(r"(?:pub(?:\([^)]*\))?\s+)?type\s+([A-Za-z_]\w*)\s*=\s*([^;]+);")
LDC = re.compile(r'ld_c!\(\s*([a-z_]+)\s+"([^"]+)"\s*=>\s*\w+\s*\)')
EXPORT = re.compile(r"export_([a-z_]+)!\s*\{([^}]*)\}", re.S)
TRAMPOLINE = re.compile(r'va_trampoline!\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,\s*"(\d+)"\s*,\s*"(\w+)"\s*\)')
ARG_REGS = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"]

# `ld_abi.rs`'s thunk shapes: what the C caller passes and gets back.
SHAPES = {
    "l_l": (["x87"], "x87"), "l_ll": (["x87", "x87"], "x87"), "l_lll": (["x87", "x87", "x87"], "x87"),
    "l_li": (["x87", "i32"], "x87"), "l_ln": (["x87", "i64"], "x87"), "l_lp": (["x87", "i64"], "x87"),
    "l_llp": (["x87", "x87", "i64"], "x87"), "l_p": (["i64"], "x87"), "l_pp": (["i64", "i64"], "x87"),
    "l_ppp": (["i64", "i64", "i64"], "x87"), "v_lpp": (["x87", "i64", "i64"], "void"),
    "d_dl": (["f64", "x87"], "f64"), "f_fl": (["f32", "x87"], "f32"),
    "i_pl": (["i64", "x87"], "i32"),
    "cl_cl": (["cx87"], "cx87"), "cl_clcl": (["cx87", "cx87"], "cx87"),
    # `export_l_cl!`: a long double complex argument, a long double back
    # (its thunk is `l_l`, the argument starting where a long double's would).
    "l_cl": (["cx87"], "x87"),
    "cl_llll": (["x87", "x87", "x87", "x87"], "cx87"),
    # Shapes whose result comes back in a register the shim's own return type
    # names -- `float` or `double` from long doubles (`narrow.rs`), an integer
    # from one (`i_l`: `ilogbl`, `lrintl`): `None` means "the shim's".
    "x_ll": (["x87", "x87"], None), "x_lll": (["x87", "x87", "x87"], None),
    "i_l": (["x87"], None), "n_lii": (["x87", "i32", "i32"], None),
}


def strip_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    return re.sub(r"//[^\n]*", "", text)


def split_params(s: str) -> list[str]:
    out, depth, cur, prev = [], 0, "", ""
    for ch in s:
        if ch in "(<[":
            depth += 1
        elif ch in ")]" or (ch == ">" and prev != "-"):
            depth -= 1
        prev = ch
        if ch == "," and depth == 0:
            out.append(cur)
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur)
    return [p.strip() for p in out if p.strip()]


def rust_functions(text: str) -> dict[str, tuple[list[str], str, bool]]:
    """name -> (parameter types, return type, variadic) as spelled."""
    text = strip_comments(text)
    out = {}
    for m in FN.finditer(text):
        i = j = m.end()
        depth = 1
        while depth and j < len(text):
            depth += {"(": 1, ")": -1}.get(text[j], 0)
            j += 1
        rm = re.match(r"\s*->\s*([^{;]+?)\s*(?:\{|;|where\b)", text[j:j + 300])
        types, variadic = [], False
        for p in split_params(text[i:j - 1]):
            if p.lstrip("_ ").startswith("...") or p.endswith("..."):
                variadic = True
            elif ":" in p:
                types.append(p.split(":", 1)[1].strip())
        out.setdefault(m.group(1), (types, rm.group(1) if rm else "()", variadic))
    return out


MACRO = re.compile(r"macro_rules!\s*(\w+)\s*\{")


def balanced(text: str, i: int) -> int:
    """The index just past the bracket group opening at `text[i]`."""
    pairs = {"(": ")", "[": "]", "{": "}"}
    stack = [pairs[text[i]]]
    j = i + 1
    while stack and j < len(text):
        ch = text[j]
        if ch in pairs:
            stack.append(pairs[ch])
        elif ch == stack[-1]:
            stack.pop()
        j += 1
    return j


def macro_arms(text: str) -> dict[str, tuple[str, str]]:
    """name -> (pattern, body) for every single-arm `macro_rules!`."""
    out = {}
    for m in MACRO.finditer(text):
        start = m.end() - 1
        end = balanced(text, start)
        inner = text[start + 1:end - 1].strip()
        if not inner or inner[0] not in "([{":
            continue
        pat_end = balanced(inner, 0)
        pattern = inner[1:pat_end - 1]
        rest = inner[pat_end:].lstrip()
        if not rest.startswith("=>"):
            continue
        rest = rest[2:].lstrip()
        if not rest or rest[0] not in "([{":
            continue
        body_end = balanced(rest, 0)
        if rest[body_end:].strip(" ;\n\t"):
            continue  # a second arm: not ours to expand
        out[m.group(1)] = (pattern, rest[1:body_end - 1])
    return out


def split_top(s: str, sep: str) -> list[str]:
    out, depth, cur = [], 0, ""
    for ch in s:
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
        if ch == sep and depth == 0:
            out.append(cur)
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur)
    return out


FRAG = re.compile(r"\$(\w+):\w+")


def substitute(body: str, values: dict[str, str]) -> str:
    return re.sub(r"\$(\w+)", lambda m: values.get(m.group(1), m.group(0)), body)


def expand_repetitions(body: str, entries: list[dict[str, str]]) -> str:
    """`$( ... )*` in `body`, once per entry."""
    out, i = "", 0
    while True:
        k = body.find("$(", i)
        if k < 0:
            return out + body[i:]
        end = balanced(body, k + 1)
        inner = body[k + 2:end - 1]
        # the repetition operator after it, perhaps with a separator
        m = re.match(r"\s*[,;]?\s*[*+?]", body[end:])
        stop = end + (m.end() if m else 0)
        out += body[i:k] + "".join(substitute(inner, e) for e in entries)
        i = stop


def expansions(text: str) -> list[str]:
    """The text each invocation of a file's single-arm macros expands to --
    enough of Rust's macro_rules for the export macros here: a pattern that
    is a list of fragment groups repeated (`$($c:literal $shim:ident ...;)*`,
    every `export_*!`), or comma-separated fragments with at most one
    bracketed repetition at the end (`binary_exports!`)."""
    out = []
    for name, (pattern, body) in macro_arms(text).items():
        for inv in re.finditer(r"\b" + re.escape(name) + r"!\s*([({\[])", text):
            if text[max(0, inv.start() - 13):inv.start()].rstrip().endswith("macro_rules!"):
                continue
            start = inv.end() - 1
            args = text[start + 1:balanced(text, start) - 1]
            pat = pattern.strip()
            if pat.startswith("$(") and pat.rstrip().endswith(("*", "+")):
                # a list of entries: $( frags ; )*
                inner = pat[2:pat.rindex(")")]
                names = FRAG.findall(inner)
                sep = ";" if ";" in inner else ","
                entries = []
                for e in split_top(args, sep):
                    toks = re.findall(r'"[^"]*"|[\w:<>]+(?:\s*<[^>]*>)?', e)
                    if len(toks) >= len(names):
                        entries.append(dict(zip(names, toks)))
                out.append(expand_repetitions(body, entries) if "$(" in body
                           else "".join(substitute(body, en) for en in entries))
                continue
            # positional fragments, the last perhaps a bracketed repetition
            pparts = [x.strip() for x in split_top(pat, ",") if x.strip() and x.strip() != "$(,)?"]
            aparts = [x.strip() for x in split_top(args, ",") if x.strip()]
            values: dict[str, str] = {}
            entries: list[dict[str, str]] = []
            for pp, ap in zip(pparts, aparts):
                if pp.startswith("["):
                    inner = pp[1:-1].strip()
                    inner = inner[2:inner.index(")")] if inner.startswith("$(") else inner
                    names = FRAG.findall(inner)
                    for e in split_top(ap.strip()[1:-1], ","):
                        toks = e.split()
                        if len(toks) == len(names):
                            entries.append(dict(zip(names, toks)))
                else:
                    fm = FRAG.fullmatch(pp)
                    if fm:
                        values[fm.group(1)] = ap
            out.append(substitute(expand_repetitions(body, entries), values))
    return out


def rust_definitions(src: Path) -> tuple[dict[str, tuple], dict[str, tuple]]:
    """(name -> (args, ret, variadic, file), trampolines: name -> (target,
    fixed-argument bytes, va_list register))."""
    texts = {f.name: f.read_text(encoding="utf-8", errors="replace") for f in sorted(src.glob("*.rs"))}
    aliases: dict[str, str] = {}
    for text in texts.values():
        for m in ALIAS.finditer(strip_comments(text)):
            aliases.setdefault(m.group(1), m.group(2).strip())
    defs: dict[str, tuple] = {}
    tramps: dict[str, tuple] = {}
    for name, text in texts.items():
        # The file as written, and what its macros expand to.
        for part in [text] + expansions(strip_comments(text)):
            for fn, (types, ret, variadic) in rust_functions(part).items():
                args = tuple(rust_class(t, aliases) for t in types)
                defs.setdefault(fn, (args, rust_class(ret, aliases), variadic, name))
            for m in LDC.finditer(part):
                if m.group(1) in SHAPES:
                    a, r = SHAPES[m.group(1)]
                    shim = re.search(r"=>\s*(\w+)", m.group(0)).group(1)
                    if r is None:
                        # the shim's own return register is the caller's
                        sdef = rust_functions(part).get(shim)
                        r = rust_class(sdef[1], aliases) if sdef else "?" + shim
                    defs[m.group(2)] = (tuple(a), r, False, name)
        for m in EXPORT.finditer(text):
            kind = m.group(1)
            for entry in m.group(2).split(";"):
                toks = re.findall(r'"([^"]+)"|([\w:]+)', entry)
                toks = [a or b for a, b in toks]
                if not toks or '"' not in entry:
                    continue
                if kind == "i_l":
                    defs[toks[0]] = (("x87",), "i64" if toks[-1] in ("i64", "c_long") else "i32",
                                     False, name)
                elif kind in SHAPES:
                    a, r = SHAPES[kind]
                    defs[toks[0]] = (tuple(a), r, False, name)
        for m in TRAMPOLINE.finditer(text):
            tramps[m.group(1)] = (m.group(2), int(m.group(3)), m.group(4))
    return defs, tramps


# ---------------------------------------------------------------------------
# The verdict
# ---------------------------------------------------------------------------


def verdict(decls: dict[str, tuple], defs: dict[str, tuple], tramps: dict[str, tuple],
            exceptions: dict[str, str], unchecked: frozenset[str]) -> tuple[list[str], dict]:
    problems: list[str] = []
    stats = {"compared": 0, "trampolines": 0, "assembly": 0, "excepted": 0, "unchecked": []}
    for name, (args, ret, variadic, header) in sorted(decls.items()):
        if name in NOT_LIBRARY:
            continue
        if name in ASSEMBLY:
            stats["assembly"] += 1
            continue
        c = assignment(args, ret, variadic)
        if name in tramps and variadic:
            # The trampoline builds a va_list whose first element is the
            # integer argument at byte `off` of the register save area, and
            # calls `target` with the `off / 8` arguments before it and the
            # va_list after them, in `reg`. So the target must take exactly
            # those arguments, as the declaration has them, then the list --
            # which may start at a fixed argument the target reads as the
            # list's first element (`execl`'s `arg`: `vexecl(path, ap)`).
            target, off, reg = tramps[name]
            ints, sses, stack = c[0], c[1], c[2]
            before = off // 8
            tdef = defs.get(target)
            why = None
            if sses or stack:
                why = "the declaration's fixed arguments are not all integers"
            elif off % 8 or before > len(ints) or ARG_REGS[before:before + 1] != [reg]:
                why = f"{len(ints)} fixed integer argument(s) cannot leave the list at byte {off} in %{reg}"
            elif tdef is None:
                why = f"its target {target} has no definition to compare"
            else:
                ta = assignment(tdef[0], tdef[1], tdef[2])
                if ta[0] != ints[:before] + ("i64",) or ta[1] or ta[2] or ta[3] != c[3]:
                    why = (f"its target {target} is {describe(ta)}, not the {before} leading "
                           f"argument(s) and a va_list returning {','.join(c[3])}")
            if why:
                problems.append(f"{name} ({header}): va_trampoline saves from byte {off} and passes "
                                f"the va_list in %{reg}: {why}")
            stats["trampolines"] += 1
            continue
        if name not in defs:
            stats["unchecked"].append(name)
            continue
        rargs, rret, rvar, rfile = defs[name]
        r = assignment(rargs, rret, rvar)
        unknown = [x for x in list(args) + [ret] + list(rargs) + [rret]
                   if isinstance(x, str) and x.startswith("?")]
        if unknown:
            stats["unchecked"].append(name)
            continue
        stats["compared"] += 1
        same = compatible(c, r)
        if name in exceptions:
            stats["excepted"] += 1
            if same:
                problems.append(f"{name}: in EXCEPTIONS but agrees now -- delete the entry")
            continue
        if not same:
            problems.append(f"{name}: declared in {header} as {describe(c)}, defined in {rfile} "
                            f"as {describe(r)}")
    new = sorted(set(stats["unchecked"]) - unchecked)
    if new:
        problems.append("declared, and no definition this reading can compare: " + " ".join(new))
    stale = sorted(n for n in unchecked if n not in stats["unchecked"])
    if stale:
        problems.append("in UNCHECKED but compared now -- delete: " + " ".join(stale))
    for name in exceptions:
        if name not in decls:
            problems.append(f"{name}: in EXCEPTIONS but declared by no header")
    return problems, stats


def describe(a) -> str:
    ints, sses, stack, ret, variadic = a
    parts = []
    if ints:
        parts.append("integer " + ",".join(ints))
    if sses:
        parts.append("sse " + ",".join(sses))
    if stack:
        parts.append("stack " + ",".join(stack))
    return (f"({'; '.join(parts) or 'no arguments'}{', ...' if variadic else ''}) -> "
            f"{','.join(ret)}")


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------


def self_test() -> int:
    failures = 0

    def check(what, cond):
        nonlocal failures
        print(("ok    " if cond else "FAIL  ") + what)
        failures += 0 if cond else 1

    al = {"TimerT": "i32", "Fd": "i32", "SizeT": "usize"}
    check("an alias resolves through posix's `type`", rust_class("TimerT", al) == "i32")
    check("a pointer is an integer register", rust_class("*mut u8", al) == "i64")
    check("`!` returns nothing", rust_class("!", al) == "void")
    check("unsigned long is 64 bits", c_class("unsigned long", {}) == "i64")
    check("a typedef resolves", c_class("size_t", {"size_t": "unsigned long"}) == "i64")
    check("an enum is an int", c_class("enum __ns_sect", {}) == "i32")
    check("a function returning a function pointer returns a pointer",
          c_class(return_type("void (*(int, void (*)(int)))(int)"), {}) == "i64")
    check("an attribute after the parameters", return_type("void (int) __attribute__((noreturn))") == "void")
    fns = rust_functions('pub extern "C" fn f(a: i32, // x\n cb: Option<extern "C" fn(i32, i32) -> i32>, '
                         'b: usize) -> i64 { 0 }')
    check("a callback's `->` and a comment do not split parameters",
          fns.get("f") == (["i32", 'Option<extern "C" fn(i32, i32) -> i32>', "usize"], "i64", False))
    decls = {"timer_delete": (("i64",), "i32", False, "time.h"),
             "open": (("i64", "i32"), "i32", True, "fcntl.h"),
             "printf": (("i64",), "i32", True, "stdio.h"),
             "snprintf": (("i64", "i64", "i64"), "i32", True, "stdio.h"),
             "setjmp": (("i64",), "i32", False, "setjmp.h"),
             "lost": (("i32",), "i32", False, "x.h")}
    defs = {"timer_delete": (("i32",), "i32", False, "time.rs"),
            "open": (("i64", "i32", "i32"), "i32", False, "file.rs")}
    decls["execl"] = (("i64", "i64"), "i32", True, "unistd.h")
    defs_extra = {"vprintf": (("i64", "i64"), "i32", False, "printf.rs"),
                  "vsnprintf": (("i64", "i64", "i64", "i64"), "i32", False, "printf.rs"),
                  "vexecl": (("i64", "i64"), "i32", False, "spawn.rs")}
    tramps = {"printf": ("vprintf", 8, "rsi"), "snprintf": ("vsnprintf", 16, "rdx"),
              "execl": ("vexecl", 8, "rsi")}
    problems, stats = verdict(decls, {**defs, **defs_extra}, tramps, {}, frozenset())
    text = "\n".join(problems)
    check("a 32-bit timer_t against a 64-bit one is refused", "timer_delete" in text)
    check("a variadic open defined with its mode as a further integer passes", "open:" not in text)
    check("printf's trampoline agrees with one fixed argument",
          not any(p.startswith("printf ") for p in problems))
    check("snprintf's trampoline saving from byte 16 with three fixed arguments is refused",
          "snprintf" in text)
    check("an assembly function is listed, not compared", stats["assembly"] == 1)
    check("execl's list starting at its second fixed argument passes",
          not any(p.startswith("execl ") for p in problems))
    check("a declared function with nothing to compare is refused", "lost" in text)
    # The export macros, expanded: a positional one with a bracketed
    # repetition (c23math.rs's `binary_exports!`), and an entry list whose
    # thunk's result is its shim's (narrow.rs's `export_ll!`).
    macros = (
        'macro_rules! pair { ($t:ty, $up:ident, [$($mm:ident $op:ident),* $(,)?]) => {\n'
        '  pub extern "C" fn $up(x: $t) -> $t { x }\n'
        '  $( pub extern "C" fn $mm(x: $t, y: $t) -> $t { x } )*\n} }\n'
        'pair!(f64, nextup, [fmax1 A, fmin1 B]);\n'
        'macro_rules! export_ll { ($($c:literal $shim:ident $f:ident $t:ty;)*) => { $(\n'
        '  unsafe extern "C" fn $shim(x: *const L, y: *const L) -> $t { 0 }\n'
        '  crate::ld_c!(x_ll $c => $shim);\n)* }; }\n'
        'export_ll! { "faddl" __s_faddl faddl f32; "daddl" __s_daddl daddl f64; }\n')
    with tempfile.TemporaryDirectory() as t:
        (Path(t) / "m.rs").write_text(macros, encoding="utf-8", newline="")
        mdefs, _ = rust_definitions(Path(t))
    check("a positional macro's function is found", mdefs.get("nextup", ())[:2] == (("f64",), "f64"))
    check("its repetition's functions are found",
          mdefs.get("fmin1", ())[:2] == (("f64", "f64"), "f64"))
    check("an entry-list thunk returns what its shim does",
          mdefs.get("faddl", ())[:2] == (("x87", "x87"), "f32")
          and mdefs.get("daddl", ())[:2] == (("x87", "x87"), "f64"))
    problems, _ = verdict({"g": (("i32",), "i32", False, "g.h")}, {"g": (("i32",), "i32", False, "g.rs")},
                          {}, {"g": "why"}, frozenset())
    check("an exception that agrees now is stale", any("delete the entry" in p for p in problems))
    print(f"check-libc-prototypes self-test: {failures} failure(s)")
    return 1 if failures else 0


# ---------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--self-test", "--selftest", action="store_true")
    parser.add_argument("--src", default=str(SRC), help="posix/src to read (default: this tree's)")
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    zig = find_zig()
    inc = musl_include(zig) if zig else None
    if inc is None:
        print("SKIPPED: no zig (FASTPY_ZIG or PATH), or no musl headers in it. (Exit 3: could not "
              "run.)", file=sys.stderr)
        return 3
    decls = c_declarations(zig, inc, OVERLAY)
    defs, tramps = rust_definitions(Path(args.src))
    if len(decls) < 1000 or len(defs) < 1000:
        print(f"ERROR: {len(decls)} declarations and {len(defs)} definitions -- too few to judge. "
              "(Exit 2.)", file=sys.stderr)
        return 2
    problems, stats = verdict(decls, defs, tramps, EXCEPTIONS, UNCHECKED)
    for p in problems:
        print(f"check-libc-prototypes: {p}", file=sys.stderr)
    if problems:
        return 1
    print(f"check-libc-prototypes: {stats['compared']} prototypes agree with their definitions "
          f"({stats['excepted']} by a documented exception), {stats['trampolines']} variadic "
          f"trampolines with theirs; {stats['assembly']} written in assembly, "
          f"{len(stats['unchecked'])} not comparable (UNCHECKED)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
