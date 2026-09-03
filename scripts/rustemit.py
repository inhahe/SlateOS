#!/usr/bin/env python3
"""Rebuild, from Rust source, a byte buffer that Rust source builds by hand.

`kernel/src/proc/elf.rs::build_linux_evdev_test_elf` assembles an x86-64
program as byte literals pushed into a `Vec<u8>`.  `check-evdev-elf-asm.py`
mirrors that sequence in Python so the encodings can be disassembled before
the kernel ever runs them -- and for as long as it existed, **nothing tied the
mirror to the original**.  A byte changed in the Rust and not in the mirror
left the checker disassembling a program that no longer ships, and reporting
it clean.  `check-shellquote-vs-bash.py` names this failure exactly: a drifted
port "reports 0 disagreements about a scanner that no longer exists -- worse
than no checker, because it looks like evidence."

That file *guards* its port on the one table easiest to get silently wrong,
because a shell scanner is control flow and cannot be recovered from source
text.  This emission is not: it is a straight-line sequence of pushes whose
every operand is a literal or a named constant.  So the whole buffer can be
rebuilt from the Rust and compared byte for byte.  A guard is second best;
this is the check the guard stands in for.

Why the hand mirror is kept rather than deleted
-----------------------------------------------
Once this module can rebuild the buffer, the Python transcription is redundant
as a *source* of bytes -- but redundancy is the point.  The reader here can be
wrong, and a lone reader has nothing to be wrong against.  Two independent
constructions that agree is a check; either one alone is an assertion.  So the
mirror stays, and the two are compared.

That is also why the mirror keeps its own hardcoded `EV_VERSION`, `KEY_BYTES`
and `EVIOC_NR_*` rather than importing the ones read from `evdev.rs`.  Sharing
them would make the two constructions agree *by construction* about the values
most likely to drift.  Hardcoded, a changed constant now fails the comparison
loudly, where before it changed nothing at all.

Nothing about the emission is transcribed here
-----------------------------------------------
Not the constants (read from `evdev.rs`), not the `_IOC` bit layout (read from
`evdev::ioc`), and not the encodings emitted by the `sentinel` / `jcc` /
`ioctl_call` helpers -- those are nested functions in the Rust, and this module
parses their bodies and replays them with the arguments bound.  Had they been
transcribed, a changed helper would have had to be changed here too, and a
reader that must be updated in lockstep with its subject is the very thing
being removed.

Anything not recognised is an error, never a skip
--------------------------------------------------
A statement this module cannot parse means it can no longer claim to model the
function.  A model that quietly drops a line it did not understand is how a
checker comes to be about a different program -- so an unmodelled statement
raises, and the caller gets no verdict rather than a false one.
"""

from __future__ import annotations

import re

__all__ = ["RustEmitError", "load_consts", "load_value_fns", "Emitter", "compare"]


class RustEmitError(RuntimeError):
    """The Rust could not be modelled, so no verdict about it is available.

    Distinct from a mismatch, which is a fact about the code.  This means the
    reader is broken or its subject outgrew it -- not data, and never to be
    scored as a pass.
    """


_CONST_RE = re.compile(
    r"^\s*pub const (?P<name>[A-Z][A-Z0-9_]*)\s*:\s*\w+\s*=\s*(?P<val>[^;]+);",
    re.MULTILINE,
)

#: A `const NAME: TY = VALUE;` declared inside a function body.
_LOCAL_CONST_RE = re.compile(
    r"^\s*const (?P<name>[A-Z][A-Z0-9_]*)\s*:\s*(?P<ty>[^=]+?)\s*=\s*(?P<val>[^;]+);",
    re.MULTILINE,
)

#: Characters a const expression may contain once names are substituted.
_EXPR_OK = re.compile(r"^[0-9+\-*/()<>&|^ ]+$")


def strip_comments(text: str) -> str:
    """Drop `//` comments, including the `///` doc form.

    Safe for the regions this module reads: no emitting statement contains a
    `//`, and the one string literal in range (`b"/dev/input/event0\\0"`) is in
    the data-layout section, past where any caller stops.
    """
    return "\n".join(line.split("//")[0] for line in text.splitlines())


def _int_literal(raw: str) -> int:
    """`0x2ff`, `1`, `0u8`, `b'A'` -> int. Raises ValueError otherwise."""
    raw = raw.strip()
    m = re.fullmatch(r"b'(\\?.)'", raw)
    if m:
        lit = m.group(1)
        if lit.startswith("\\"):
            return ord({"n": "\n", "t": "\t", "r": "\r", "0": "\0", "\\": "\\"}[lit[1]])
        return ord(lit)
    raw = re.sub(r"(u|i)(8|16|32|64|size)$", "", raw).replace("_", "")
    if raw.lower().startswith("0x"):
        return int(raw, 16)
    return int(raw, 10)


def _arith(expr: str, ns: dict[str, int]) -> int:
    """Evaluate a Rust integer expression over `ns`.

    Only the arithmetic these files use is supported, and `/` is integer
    division because that is what it means for a Rust unsigned constant --
    `KEY_BYTES` is `(KEY_MAX as usize + 8) / 8`, and a reader that got that
    wrong would rebuild a program with the wrong bitmap length.
    """
    e = re.sub(r"\s+as\s+\w+", "", expr)
    e = re.sub(r"\b[A-Za-z_][A-Za-z0-9_:]*\b",
               lambda m: str(ns[_bare(m.group(0))]) if _bare(m.group(0)) in ns
               else m.group(0), e)
    e = re.sub(r"\b(0[xX][0-9a-fA-F_]+|\d[\d_]*)(u|i)(8|16|32|64|size)?\b",
               lambda m: str(_int_literal(m.group(1))), e)
    e = re.sub(r"\b0[xX][0-9a-fA-F_]+\b", lambda m: str(_int_literal(m.group(0))), e)
    if not _EXPR_OK.fullmatch(e):
        raise ValueError(f"unresolved names or unsupported syntax in {expr!r} -> {e!r}")
    e = re.sub(r"(?<![/])/(?![/])", "//", e)
    return int(eval(e, {"__builtins__": {}}, {}))  # noqa: S307 - charset-guarded


def _bare(name: str) -> str:
    """`crate::evdev::IOC_READ` -> `IOC_READ`."""
    return name.rsplit("::", 1)[-1]


def _split_statements(text: str) -> list[str]:
    """Split on `;`, but not on one inside brackets.

    `&[0u8; 8]` is a Rust array-repeat expression, and splitting it in half
    turns one statement that emits eight bytes into two fragments that emit
    none. That is not a parse failure that announces itself -- it is the exact
    silent-drop this module refuses to do.

    A block (`for site in ... { ... }`) carries no trailing `;`, so a closing
    brace that returns to depth zero also ends a statement.  Without that the
    block swallows whatever follows it up to the next semicolon, and since the
    block itself is on the ignore list, the swallowed statement is discarded
    with it -- which is how three bytes of `mov rdi, r8` went missing the first
    time this ran.
    """
    out, depth, cur = [], 0, ""
    for ch in text:
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
        if ch == ";" and depth == 0:
            out.append(cur)
            cur = ""
            continue
        cur += ch
        if ch == "}" and depth == 0:
            out.append(cur)
            cur = ""
    out.append(cur)
    return out


def split_args(inner: str) -> list[str]:
    """Split a call's argument list on top-level commas."""
    out, depth, cur = [], 0, ""
    for ch in inner:
        if ch in "([":
            depth += 1
        elif ch in ")]":
            depth -= 1
        if ch == "," and depth == 0:
            out.append(cur)
            cur = ""
        else:
            cur += ch
    out.append(cur)
    return [a.strip() for a in out if a.strip()]


def load_consts(source: str, names: tuple[str, ...]) -> dict[str, int]:
    """Read `pub const` integer values out of Rust source.

    Every requested name must be present and must evaluate.  A missing one
    raises rather than defaulting: a renamed constant is precisely the drift
    this module exists to notice, and silently keeping a stale value would
    rebuild the program the source no longer describes.

    Constants defined over other constants resolve by repeated passes, so
    declaration order in the Rust does not matter.
    """
    raws = {m.group("name"): m.group("val").strip() for m in _CONST_RE.finditer(source)}
    missing = [n for n in names if n not in raws]
    if missing:
        raise RustEmitError(
            "constant(s) not found in the Rust source: " + ", ".join(missing) + "\n"
            "  Either they were renamed, or the file read is not the one that\n"
            "  defines them. Until they are found, nothing built from them\n"
            "  describes the code that ships."
        )
    resolved: dict[str, int] = {}
    for _ in range(len(raws) + 1):
        progress = False
        for name, raw in raws.items():
            if name in resolved:
                continue
            try:
                resolved[name] = _arith(raw, resolved)
                progress = True
            except (ValueError, KeyError, SyntaxError, ZeroDivisionError):
                continue
        if not progress:
            break
    unresolved = [n for n in names if n not in resolved]
    if unresolved:
        raise RustEmitError(
            "constant(s) could not be evaluated: "
            + ", ".join(f"{n} = {raws[n]}" for n in unresolved)
            + "\n  This module resolves constants by reading them, so one it "
            "cannot read must\n  be taught to the evaluator -- a hardcoded copy "
            "here is the drift the\n  module exists to remove."
        )
    return {n: resolved[n] for n in names}


#: `fn name(params) -> ret {` ... a nested or free function.
_FN_RE = re.compile(
    r"^(?P<indent>[ \t]*)(?:pub )?(?:const )?fn (?P<name>\w+)"
    r"\((?P<params>[^)]*)\)(?:\s*->\s*(?P<ret>[^{]+?))?\s*\{",
    re.MULTILINE,
)


def _fn_bodies(source: str) -> dict[str, tuple[list[tuple[str, str]], str]]:
    """Every function in `source`, as name -> (params, body).

    Bodies are matched by brace counting from the opening `{`, which is exact
    for these files because comments are stripped first and no string literal
    in range contains a brace.
    """
    out: dict[str, tuple[list[tuple[str, str]], str]] = {}
    for m in _FN_RE.finditer(source):
        start = m.end() - 1
        depth = 0
        end = None
        for i in range(start, len(source)):
            if source[i] == "{":
                depth += 1
            elif source[i] == "}":
                depth -= 1
                if depth == 0:
                    end = i
                    break
        if end is None:
            continue
        params = []
        for p in split_args(m.group("params")):
            if ":" not in p:
                continue
            pname, pty = p.split(":", 1)
            params.append((pname.strip(), pty.strip()))
        out[m.group("name")] = (params, source[start + 1:end])
    return out


def load_value_fns(source: str, names: tuple[str, ...]) -> dict[str, tuple]:
    """Pick out pure value-returning functions, e.g. `evdev::ioc`.

    Their bodies are single expressions, so they can be evaluated with the
    arguments bound rather than having their formula copied.  `ioc` encodes the
    `_IOC` bit layout; a transcription of it here would be one more thing to
    keep in step with `evdev.rs`, which is the failure being removed.
    """
    fns = _fn_bodies(strip_comments(source))
    missing = [n for n in names if n not in fns]
    if missing:
        raise RustEmitError(
            "value function(s) not found: " + ", ".join(missing) + "\n"
            "  A renamed or restructured encoder cannot be replayed, and the\n"
            "  request numbers built from it would be this module's invention."
        )
    return {n: (fns[n][0], " ".join(fns[n][1].split())) for n in names}


class Emitter:
    """Replays the emission sequence of one Rust function, from its source.

    Positions whose bytes the Rust patches *after* emitting them -- jump
    displacements, and the 64-bit address of the path string -- are recorded in
    `wildcards`.  They depend on where their targets land and cannot be
    recovered from source text, so they are excluded from comparison rather
    than guessed.  The instructions carrying them are still compared in full;
    only the displacement's own bytes are exempt.
    """

    def __init__(self, consts: dict[str, int], value_fns: dict[str, tuple] | None = None):
        self.code = bytearray()
        self.wildcards: set[int] = set()
        self.ns: dict[str, int] = {_bare(k): v for k, v in consts.items()}
        self.slices: dict[str, bytes] = {}
        self.positions: dict[str, int] = {}
        self.value_fns: dict[str, tuple] = dict(value_fns or {})
        self.helpers: dict[str, tuple[list[tuple[str, str]], str]] = {}
        self.unhandled: list[str] = []
        self._depth = 0

    # -- expressions -------------------------------------------------------

    def value(self, expr: str, ns: dict[str, int] | None = None) -> int:
        ns = self.ns if ns is None else ns
        expr = expr.strip()
        m = re.fullmatch(r"(.+?)\s+as\s+u(8|16|32|64|size)", expr)
        if m:
            width = 64 if m.group(2) == "size" else int(m.group(2))
            return self.value(m.group(1), ns) & ((1 << width) - 1)
        m = re.fullmatch(r"([\w:]+)\((.*)\)", expr, re.DOTALL)
        if m and _bare(m.group(1)) in self.value_fns:
            params, body = self.value_fns[_bare(m.group(1))]
            args = split_args(m.group(2))
            if len(args) != len(params):
                raise RustEmitError(
                    f"{expr!r}: {len(args)} argument(s) for "
                    f"{len(params)} parameter(s) of {_bare(m.group(1))}"
                )
            inner = dict(ns)
            inner.update({p[0]: self.value(a, ns) for p, a in zip(params, args)})
            return self.value(body, inner)
        if _bare(expr) in ns:
            return ns[_bare(expr)]
        try:
            return _int_literal(expr)
        except (ValueError, KeyError):
            pass
        try:
            return _arith(expr, ns)
        except (ValueError, KeyError, SyntaxError) as exc:
            raise RustEmitError(
                f"cannot evaluate {expr!r} from the emission region.\n"
                "  It is not a literal, a known constant, or arithmetic over "
                "them.\n  Guessing here would rebuild a different program, so "
                "it stops instead."
            ) from exc

    def byte_array(self, inner: str, ns: dict[str, int]) -> bytes:
        """`0x48, evdev::BUS_I8042 as u8` or `0u8; 8` -> bytes."""
        m = re.fullmatch(r"(.+?);\s*(\d+)", inner.strip())
        if m:
            return bytes([self.value(m.group(1), ns) & 0xFF]) * int(m.group(2))
        return bytes(self.value(a, ns) & 0xFF for a in split_args(inner))

    # -- driving -----------------------------------------------------------

    def learn_helpers(self, fn_source: str, names: tuple[str, ...]) -> None:
        """Record the emitting helper functions so their bodies can be replayed."""
        fns = _fn_bodies(strip_comments(fn_source))
        missing = [n for n in names if n not in fns]
        if missing:
            raise RustEmitError(
                "emitting helper(s) not found: " + ", ".join(missing) + "\n"
                "  Their encodings would have to be transcribed instead, which "
                "is the\n  drift this module removes. Point it at the right "
                "function, or teach\n  it the new names."
            )
        self.helpers = {n: fns[n] for n in names}

    def learn_local_consts(self, header: str) -> None:
        """Bind the `const` declarations of the function being replayed.

        Read from the source for the same reason as everything else: `JNZ`,
        the stack displacements and `KEY_BITMAP_LEN` are exactly the kind of
        value that gets changed in one place and not the other.
        """
        for m in _LOCAL_CONST_RE.finditer(strip_comments(header)):
            name, ty, val = m.group("name"), m.group("ty").strip(), m.group("val")
            if "[u8]" in ty:
                inner = re.fullmatch(r"&\s*\[(.+)\]", val.strip(), re.DOTALL)
                if inner:
                    self.slices[name] = self.byte_array(inner.group(1), self.ns)
                    continue
            self.ns[name] = self.value(val)

    def run(self, region: str, *, unknown_ok: bool = False) -> None:
        """Replay every emitting statement in `region`, in order.

        `unknown_ok` must be False.  It is a named parameter so that anyone
        tempted to loosen this has to write the word down, and so that this
        docstring is what they read when they do.
        """
        if unknown_ok:
            raise RustEmitError(
                "unknown_ok=True would let an unparsed statement pass as if it "
                "emitted\n  nothing, which is exactly how a rebuilt buffer comes "
                "to describe a\n  different program."
            )
        self._replay(region, self.ns, "code")
        if self.unhandled:
            raise RustEmitError(
                f"{len(self.unhandled)} statement(s) in the emission region are "
                "not modelled:\n  "
                + "\n  ".join(self.unhandled)
                + "\n\nThe function has outgrown this reader. Until it is taught "
                "them, the buffer\nit rebuilds is missing whatever they emit, and "
                "every comparison against it\nwould be about a different program."
            )

    #: Statements that emit nothing and are safely ignored.  Listed explicitly
    #: so that anything NOT listed is a hard error rather than a silent skip.
    _IGNORE = (
        r"^let\s+mut\s+\w+\s*:\s*Vec<\w+>\s*=\s*Vec::new\(\)$",
        r"^for\s+\w+\s+in\s+",
        r"^let\s+disp\s*=",
        r"^code\s*\[\s*\*\w+",           # the displacement patch loop
        r"^\}?\s*$",
        r"^\{\s*$",
    )

    def _replay(self, region: str, ns: dict[str, int], buf: str) -> None:
        self._depth += 1
        if self._depth > 8:
            raise RustEmitError("helper recursion too deep -- refusing to guess")
        try:
            for raw in _split_statements(strip_comments(region)):
                stmt = " ".join(raw.split())
                if stmt:
                    self._statement(stmt, ns, buf)
        finally:
            self._depth -= 1

    def _statement(self, stmt: str, ns: dict[str, int], buf: str) -> None:
        for pat in self._IGNORE:
            if re.search(pat, stmt):
                return

        # `let path_imm = code.len();` -- a position remembered for later patching.
        m = re.fullmatch(rf"let (?:mut )?(\w+)(?: : \w+)? = {re.escape(buf)} \. len \( \)"
                         .replace(" ", r"\s*"), stmt)
        if m:
            self.positions[m.group(1)] = len(self.code)
            return
        # `sites.push(code.len() - 4);` -- records a displacement to patch. The
        # recorded offset is the start of a 4-byte `i32`, which is what the
        # patch loop writes there, so those four bytes become wildcards.
        m = re.fullmatch(rf"(\w+) \. push \( {re.escape(buf)} \. len \( \) - (\d+) \)"
                         .replace(" ", r"\s*"), stmt)
        if m and m.group(1) != buf:
            at = len(self.code) - int(m.group(2))
            self.wildcards.update(range(at, at + 4))
            return

        m = re.fullmatch(r"const (\w+)\s*:\s*([^=]+?)\s*=\s*(.+)", stmt)
        if m:
            if "[u8]" in m.group(2):
                inner = re.fullmatch(r"&\s*\[(.+)\]", m.group(3).strip())
                if inner:
                    self.slices[m.group(1)] = self.byte_array(inner.group(1), ns)
                    return
            ns[m.group(1)] = self.value(m.group(3), ns)
            return
        m = re.fullmatch(r"let (\w+) = (.+)", stmt)
        if m and buf not in m.group(2):
            ns[m.group(1)] = self.value(m.group(2), ns)
            return

        m = re.fullmatch(rf"{re.escape(buf)} \. extend_from_slice \( & \[(.+)\] \)"
                         .replace(" ", r"\s*"), stmt)
        if m:
            self.code.extend(self.byte_array(m.group(1), ns))
            return
        m = re.fullmatch(rf"{re.escape(buf)} \. extend_from_slice \( & (.+) \. to_le_bytes \( \) \)"
                         .replace(" ", r"\s*"), stmt)
        if m:
            self.code.extend(self.value(m.group(1), ns).to_bytes(4, "little"))
            return
        # `code.extend_from_slice(set_rdx);` -- a bound `&[u8]` argument.
        m = re.fullmatch(rf"{re.escape(buf)} \. extend_from_slice \( (\w+) \)"
                         .replace(" ", r"\s*"), stmt)
        if m and m.group(1) in self.slices:
            self.code.extend(self.slices[m.group(1)])
            return
        m = re.fullmatch(rf"{re.escape(buf)} \. push \( (.+) \)".replace(" ", r"\s*"), stmt)
        if m:
            self.code.append(self.value(m.group(1), ns) & 0xFF)
            return

        m = re.fullmatch(r"(\w+)\s*\((.+)\)", stmt, re.DOTALL)
        if m and m.group(1) in self.helpers:
            self._call_helper(m.group(1), split_args(m.group(2)), ns)
            return

        self.unhandled.append(stmt)

    def _call_helper(self, name: str, args: list[str], ns: dict[str, int]) -> None:
        params, body = self.helpers[name]
        if len(args) != len(params):
            self.unhandled.append(f"{name}(...): {len(args)} args, {len(params)} params")
            return
        inner = dict(ns)
        buf = "code"
        for (pname, pty), arg in zip(params, args):
            if "Vec<u8>" in pty:
                buf = pname
            elif "[u8]" in pty:
                key = arg.strip()
                if key in self.slices:
                    self.slices[pname] = self.slices[key]
                else:
                    lit = re.fullmatch(r"&\s*\[(.+)\]", key, re.DOTALL)
                    if not lit:
                        self.unhandled.append(f"{name}(...): cannot resolve {key!r}")
                        return
                    self.slices[pname] = self.byte_array(lit.group(1), ns)
            elif "Vec<usize>" in pty:
                continue  # the patch-site list; its effect is the wildcards
            else:
                inner[pname] = self.value(arg, ns)
        self._replay(body, inner, buf)

    def mark_patched(self, fn_source: str) -> None:
        """Wildcard the byte ranges the Rust overwrites by name after emitting.

        `code[path_imm..path_imm + 8].copy_from_slice(&path_vaddr.to_le_bytes())`
        fills in the program's own load address, which is not knowable from the
        source text -- so those eight bytes are excluded rather than invented.
        """
        for m in re.finditer(
            r"code\s*\[\s*(\w+)\s*\.\.\s*\1\s*\+\s*(\d+)\s*\]\s*\.copy_from_slice",
            strip_comments(fn_source),
        ):
            name, width = m.group(1), int(m.group(2))
            if name not in self.positions:
                raise RustEmitError(
                    f"`code[{name}..{name} + {width}]` is patched, but no "
                    f"`let {name} = code.len()` was seen.\n"
                    "  Those bytes would be compared against a value this module "
                    "cannot know."
                )
            self.wildcards.update(range(self.positions[name], self.positions[name] + width))


def compare(built: bytes, mirror: bytes, wildcards: set[int]) -> list[str]:
    """Byte-for-byte, skipping only positions the Rust patches after the fact.

    Returns human-readable differences; empty means the two constructions agree.
    """
    diffs: list[str] = []
    if len(built) != len(mirror):
        diffs.append(
            f"length: rebuilt from Rust = {len(built)} bytes, "
            f"hand mirror = {len(mirror)} bytes"
        )
    for i in range(min(len(built), len(mirror))):
        if i in wildcards or built[i] == mirror[i]:
            continue
        lo = max(0, i - 6)
        diffs.append(
            f"offset 0x{i:x}: rust=0x{built[i]:02x} mirror=0x{mirror[i]:02x}\n"
            f"    rust  : {built[lo:i + 6].hex(' ')}\n"
            f"    mirror: {mirror[lo:i + 6].hex(' ')}"
        )
        if len(diffs) >= 8:
            diffs.append("... further differences suppressed")
            break
    return diffs
