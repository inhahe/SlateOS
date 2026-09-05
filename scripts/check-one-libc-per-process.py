#!/usr/bin/env python3
"""Refuse a Rust dependency on any part of `posix` that keeps state.

Why this exists
===============

`posix` is compiled two different ways, and only one of them works.

Built as a **staticlib** (`target_os = "none"`, which is what
`toolchain/build-sysroot.ps1` produces as `libc.a`) it contains real syscalls,
a real fd table, a real per-thread block. Every SlateOS program links against
that copy, and it is correct.

Listed as an ordinary **Rust dependency**, it compiles a *second* time. A
SlateOS program target says `target_os = "linux"`
(`toolchain/x86_64-slateos.json`), so all ~1,845 `#[cfg(target_os = "none")]`
arms take the other branch: `syscallN` returns `HOST_ENOSYS` (`-38`),
`perthread::current()` becomes a `thread_local!` in a different TLS slot, and
`errno` is a different cell from the one `__errno_location()` hands out.

Nothing warns. The program now carries two libcs that disagree, and the broken
one is the one its Rust code reaches.

That is not a hypothetical. On 2026-09-04 it meant `ssh` and `sshd` could not
run at all: both drew their key material through `posix::random::fill`, which
read `-ENOSYS` as "no kernel here", fell through to an RDRAND fallback the QEMU
guest CPU does not have, and returned `EIO` -- with an error message blaming
"the kernel CSPRNG", which had never been asked. Full chain in `known-issues.md`
-> `TD-B-THE-POSIX-RLIB-IS-A-SECOND-LIBC-WITH-EVERY-SYSCALL-STUBBED-OUT`;
the decision this enforces is `design-decisions.md` section 768.

The rule
========

**One libc per process.** Anything in `posix` that touches the kernel, global
state, or per-thread state is reachable from a program only through the C ABI
-- the symbols in `libc.a`. Pure computation over caller-owned buffers is
exempt, because two copies of a function that reads its input and writes its
output through the caller's pointers have nothing to disagree about.

What it checks
==============

**Half 1 -- who may name `posix::`.** Any crate outside `posix/` that writes
`posix::<module>` must name a module on `PURE_MODULES`. Naming anything else
fails.

Additionally, even within an allowed module, naming one of its `extern "C"`
functions as a *Rust path* fails. `posix::crypt::crypt_r(...)` called as a Rust
item runs the rlib copy and sets an `errno` the program cannot read; the same
function reached as a C symbol through `libc.a` is correct. The name is the
same, so only this check distinguishes them.

**Half 2 -- whether the allowlist is still honest.** For each allowed module,
every mention of `syscall`, `perthread` or `errno` must sit inside either

  (a) a `#[cfg(test)]` block -- host tests exercise the host arm on purpose, or
  (b) an `extern "C"` function -- that *is* a libc entry point. A program
      reaching it goes through `libc.a`'s symbol, so the state it touches is
      the linked libc's state, which is the correct one.

A mention anywhere else is a Rust-callable path into per-process state, which
is precisely the hazard. This half is the point of the gate: without it the
allowlist is a claim made once and never rechecked, which is the shape of every
rule in this tree that has quietly stopped being true. `crypt` earns its place
today only because all four of its `errno` writes are inside `extern "C"`
functions; if a fifth appeared in the Rust-native API, this is what would say so.

Usage
=====

    python scripts/check-one-libc-per-process.py [--self-test] [ROOT]

Exit codes: 0 clean, 1 a violation, 2 the tree could not be read.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# Modules of `posix` a foreign crate may name, and why each is safe.
PURE_MODULES: dict[str, str] = {
    "ed25519": (
        "pure arithmetic over caller-owned buffers; reaches only crate::sha2"
    ),
    "crypt": (
        "Rust-native API (buf/setting_into/hash_into) is pure; its errno writes "
        "are all inside extern \"C\" libc entry points"
    ),
}

# Directories that hold crates linked into SlateOS programs. `posix/` itself is
# excluded by construction -- it is allowed to name its own modules.
DEFAULT_SCAN = ("apps", "userspace", "services", "init", "gui", "pkg", "net")

# Names that indicate per-process or per-thread state in `posix`.
STATEFUL = ("syscall", "perthread", "errno")

# The ABI string matches either `"C"` or `" "`: these regexes run over
# `strip_noise` output, which blanks the *contents* of string literals, and
# `extern "C"` contains one. Matching only `"C"` made every `extern "C"` body
# invisible to `exempt_spans`, so the gate reported all 23 of `crypt`'s errno
# writes as violations when every one of them is inside a C entry point. The
# self-test now exercises the stripped form, which is what missed it.
_EXTERN_FN = re.compile(r'\bpub\s+extern\s+"\s*C?\s*"\s+fn\s+(\w+)')
_POSIX_PATH = re.compile(r"\bposix\s*::\s*(\w+)\s*(?:::\s*(\w+))?")
_USE_BRACED = re.compile(r"\buse\s+posix\s*::\s*\{([^}]*)\}")


def strip_noise(text: str) -> str:
    """Blank out comments and string literals, preserving line numbering.

    Without this the gate fires on its own documentation: several `Cargo.toml`
    comments and doc comments in `ssh`/`ssh-keygen` name `posix::random`
    precisely to explain why it must not be called.
    """
    out: list[str] = []
    i, n = 0, len(text)
    in_line_comment = in_string = False
    in_char = False
    block_depth = 0
    while i < n:
        ch = text[i]
        nxt = text[i + 1] if i + 1 < n else ""
        if in_line_comment:
            if ch == "\n":
                in_line_comment = False
                out.append(ch)
            else:
                out.append(" ")
            i += 1
            continue
        if block_depth:
            if ch == "*" and nxt == "/":
                block_depth -= 1
                out.append("  ")
                i += 2
                continue
            if ch == "/" and nxt == "*":
                block_depth += 1
                out.append("  ")
                i += 2
                continue
            out.append("\n" if ch == "\n" else " ")
            i += 1
            continue
        if in_string or in_char:
            closer = '"' if in_string else "'"
            if ch == "\\":
                out.append("  ")
                i += 2
                continue
            if ch == closer:
                in_string = in_char = False
                out.append(ch)
                i += 1
                continue
            out.append("\n" if ch == "\n" else " ")
            i += 1
            continue
        if ch == "/" and nxt == "/":
            in_line_comment = True
            out.append("  ")
            i += 2
            continue
        if ch == "/" and nxt == "*":
            block_depth = 1
            out.append("  ")
            i += 2
            continue
        if ch == '"':
            in_string = True
        elif ch == "'":
            # A lifetime (`'a`) is not a character literal. A char literal is
            # `'x'` or `'\n'`, so look for the closing quote within four bytes.
            tail = text[i : i + 6]
            if re.match(r"'(\\.|[^\\'])'", tail):
                in_char = True
        out.append(ch)
        i += 1
    return "".join(out)


def _spans_of(text: str, starts: list[int]) -> list[tuple[int, int]]:
    """For each offset, the [start, end) of the brace-delimited body after it."""
    spans: list[tuple[int, int]] = []
    for start in starts:
        open_at = text.find("{", start)
        if open_at < 0:
            continue
        depth, j = 0, open_at
        while j < len(text):
            if text[j] == "{":
                depth += 1
            elif text[j] == "}":
                depth -= 1
                if depth == 0:
                    break
            j += 1
        spans.append((start, j + 1))
    return spans


def exempt_spans(text: str) -> list[tuple[int, int]]:
    """Regions where a mention of stateful machinery is legitimate.

    Namely `#[cfg(test)]` items and `extern "C"` functions -- see the module
    docstring for why each is exempt.
    """
    starts = [m.start() for m in re.finditer(r"#\[cfg\(test\)\]", text)]
    starts += [m.start() for m in _EXTERN_FN.finditer(text)]
    return _spans_of(text, starts)


def extern_fn_names(text: str) -> set[str]:
    """The `extern "C"` function names a module exports."""
    return {m.group(1) for m in _EXTERN_FN.finditer(text)}


def line_of(text: str, offset: int) -> int:
    """1-based line number of a byte offset."""
    return text.count("\n", 0, offset) + 1


def rust_sources(base: Path):
    """Every `.rs` file under `base`, without descending into build output.

    `rglob` would walk into `target/` before the filter sees it, and a warm
    `target/` holds hundreds of thousands of files -- enough to turn a
    one-second gate into a minute-long one, which is how a gate stops being
    run. Pruning at the directory level is the difference.
    """
    skip = {"target", ".git", "node_modules"}
    stack = [base]
    while stack:
        directory = stack.pop()
        try:
            entries = list(directory.iterdir())
        except OSError:
            continue
        for entry in entries:
            if entry.is_dir():
                if entry.name not in skip and not entry.name.startswith("target-"):
                    stack.append(entry)
            elif entry.suffix == ".rs":
                yield entry


_DEP_POSIX = re.compile(r'^\s*posix\s*=|^\s*posix\s*\.', re.MULTILINE)


def posix_dependents(root: Path, scan: tuple[str, ...]) -> list[Path]:
    """Crate directories whose manifest names `posix` as a dependency.

    Scanning every `.rs` file in the tree took 2m55s -- almost all of it
    reading files that cannot possibly contain a `posix::` path, since a crate
    that does not depend on `posix` would not compile if it named one. Filtering
    by manifest first cuts it to the 11 crates that actually can, and makes the
    gate fast enough that nobody has a reason to skip it.
    """
    found: list[Path] = []
    for top in scan:
        base = root / top
        if not base.is_dir():
            continue
        stack = [base]
        while stack:
            directory = stack.pop()
            try:
                entries = list(directory.iterdir())
            except OSError:
                continue
            for entry in entries:
                if entry.is_dir():
                    if entry.name not in {"target", ".git", "src", "tests"}:
                        stack.append(entry)
                elif entry.name == "Cargo.toml":
                    try:
                        text = entry.read_text(encoding="utf-8", errors="replace")
                    except OSError:
                        continue
                    # Only the dependency sections, each ending at the next
                    # section header: a `[package]` description or a comment
                    # may mention posix in prose, and several in this tree do
                    # -- including the comments written to explain this very
                    # rule.
                    deps = ""
                    for m in re.finditer(r"^\s*\[[^\]]*dependencies\]", text, re.M):
                        rest = text[m.end() :]
                        nxt = re.search(r"^\s*\[", rest, re.M)
                        deps += rest[: nxt.start()] if nxt else rest
                    if _DEP_POSIX.search(deps):
                        found.append(entry.parent)
    return found


def check_callers(root: Path, scan: tuple[str, ...]) -> list[str]:
    """Half 1: every `posix::` reference from outside `posix/`."""
    problems: list[str] = []
    externs: dict[str, set[str]] = {}
    for module in PURE_MODULES:
        path = root / "posix" / "src" / f"{module}.rs"
        externs[module] = (
            extern_fn_names(strip_noise(path.read_text(encoding="utf-8", errors="replace")))
            if path.is_file()
            else set()
        )

    for base in sorted(posix_dependents(root, scan)):
        for source in sorted(rust_sources(base)):
            try:
                raw = source.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            if "posix" not in raw:
                continue
            text = strip_noise(raw)
            rel = source.relative_to(root).as_posix()

            # `use posix::{a, b};` names several modules in one path.
            for m in _USE_BRACED.finditer(text):
                for part in m.group(1).split(","):
                    name = part.strip().split("::")[0].strip()
                    if name and name not in PURE_MODULES:
                        problems.append(
                            f"{rel}:{line_of(text, m.start())}: `posix::{name}` is "
                            f"not a pure module of posix"
                        )

            for m in _POSIX_PATH.finditer(text):
                module, item = m.group(1), m.group(2)
                lineno = line_of(text, m.start())
                if module not in PURE_MODULES:
                    problems.append(
                        f"{rel}:{lineno}: `posix::{module}` is not a pure module "
                        f"of posix"
                    )
                elif item and item in externs.get(module, set()):
                    problems.append(
                        f"{rel}:{lineno}: `posix::{module}::{item}` is an "
                        f'extern "C" libc entry point; as a Rust path it runs '
                        f"the rlib copy, whose errno the program cannot read"
                    )
    return problems


def _is_import_line(text: str, offset: int) -> bool:
    """Whether an offset sits on a `use` declaration.

    A bare `use crate::errno;` reaches nothing -- it only brings a name into
    scope. Exempting it costs the check nothing, because wherever that name is
    then *called* from Rust-native code, the call site is what fires. Without
    this the gate reports `crypt`'s single import as a violation while every
    actual use of it is correctly inside a C entry point, which is a finding
    that teaches the reader to distrust the gate.
    """
    start = text.rfind("\n", 0, offset) + 1
    return text[start:offset].lstrip().startswith(("use ", "pub use "))


def check_allowlist(root: Path) -> list[str]:
    """Half 2: whether each allowlisted module still deserves its place."""
    problems: list[str] = []
    for module in sorted(PURE_MODULES):
        path = root / "posix" / "src" / f"{module}.rs"
        if not path.is_file():
            problems.append(
                f"posix/src/{module}.rs: on the allowlist but does not exist"
            )
            continue
        text = strip_noise(path.read_text(encoding="utf-8", errors="replace"))
        spans = exempt_spans(text)
        # One source line can name the same thing twice (`errno::set_errno(
        # errno::EFAULT)`), and reporting it twice makes a two-fault file look
        # like a twenty-fault one.
        seen: set[tuple[int, str]] = set()
        for name in STATEFUL:
            for m in re.finditer(rf"\b{name}\b", text):
                if any(a <= m.start() < b for a, b in spans):
                    continue
                if _is_import_line(text, m.start()):
                    continue
                if (line_of(text, m.start()), name) in seen:
                    continue
                seen.add((line_of(text, m.start()), name))
                problems.append(
                    f"posix/src/{module}.rs:{line_of(text, m.start())}: `{name}` "
                    f'outside #[cfg(test)] and outside any extern "C" function. '
                    f"{module} is allowlisted as pure; this is a Rust-callable "
                    f"path into per-process state, so either move it behind a C "
                    f"entry point or drop {module} from PURE_MODULES."
                )
    return problems


def _self_test() -> int:
    """Check the analysis against sources written for the purpose."""
    failures = 0

    def expect(label: str, got: object, want: object) -> None:
        nonlocal failures
        if got != want:
            print(f"  FAIL {label}: got {got!r}, want {want!r}")
            failures += 1
        else:
            print(f"  ok   {label}")

    # The stripper must not see code in comments or strings -- the gate's own
    # documentation names `posix::random` on purpose.
    stripped = strip_noise('// posix::random\nlet s = "posix::random";\nposix::crypt::buf();\n')
    expect("comment is blanked", "posix::random" in stripped.split("\n")[0], False)
    expect("string is blanked", "posix::random" in stripped.split("\n")[1], False)
    expect("code survives", "posix::crypt::buf" in stripped, True)
    expect("line count preserved", stripped.count("\n"), 3)

    # A URL contains `//` but is inside a string, so it is blanked as a string,
    # not mistaken for a comment that swallows the rest of the file.
    after_url = strip_noise('let u = "https://x/";\nposix::ed25519::sign();\n')
    expect("url does not swallow the file", "posix::ed25519::sign" in after_url, True)

    # A lifetime is not a character literal.
    lifetimes = strip_noise("fn f<'a>(x: &'a u8) {}\nposix::crypt::buf();\n")
    expect("lifetime is not a char literal", "posix::crypt::buf" in lifetimes, True)

    # Brace spans cover the whole body, including nested braces.
    body = 'pub extern "C" fn f() { if x { errno::set(1); } }\nerrno::set(2);\n'
    spans = exempt_spans(body)
    inside = body.index("errno::set(1)")
    outside = body.index("errno::set(2)")
    expect("nested brace stays inside", any(a <= inside < b for a, b in spans), True)
    expect("after the fn is outside", any(a <= outside < b for a, b in spans), False)

    expect("extern names are found", extern_fn_names(body), {"f"})

    # The regression that got past the first version of this file: the gate
    # runs over `strip_noise` output, and `extern "C"` *contains a string
    # literal*, so stripping turns it into `extern " "`. A pattern anchored on
    # `"C"` therefore matched nothing, `exempt_spans` returned no spans, and
    # every errno write inside a C entry point was reported as a violation --
    # 23 of them in `crypt`, all of them correct code. The original self-test
    # passed because it fed the detector raw source, which is not what the gate
    # ever sees. Every case below goes through the stripper first.
    stripped_body = strip_noise(body)
    expect("stripper blanks the ABI string", '"C"' in stripped_body, False)
    expect("extern survives stripping", extern_fn_names(stripped_body), {"f"})
    inside_s = stripped_body.index("errno::set(1)")
    spans_s = exempt_spans(stripped_body)
    expect(
        "C entry point still exempt after stripping",
        any(a <= inside_s < b for a, b in spans_s),
        True,
    )

    # And the converse: a Rust-native fn touching errno is *not* exempt, which
    # is the whole point of the check.
    native = strip_noise("pub fn helper() { errno::set(1); }\n")
    native_spans = exempt_spans(native)
    expect(
        "a plain Rust fn is not exempt",
        any(a <= native.index("errno::set") < b for a, b in native_spans),
        False,
    )

    imports = "use crate::errno;\nerrno::set(1);\n"
    expect("an import is exempt", _is_import_line(imports, imports.index("errno")), True)
    expect(
        "a call on the next line is not",
        _is_import_line(imports, imports.rindex("errno")),
        False,
    )

    expect("line_of counts from one", line_of("a\nb\nc", 2), 2)

    # A gate that passes but cannot fail is not a gate. Everything above tests
    # a helper; this builds a whole synthetic tree and runs the two halves over
    # it, so the refusal itself is what is proved -- including that the refusal
    # is *aimed*, since the same tree contains a legitimate use that must not
    # fire. Compare `scripts/check-gates-can-refuse.py`, which exists because
    # gates in this tree have shipped unable to say no.
    import shutil
    import tempfile

    tmp = Path(tempfile.mkdtemp(prefix="one-libc-selftest-"))
    try:
        # `newline=""` on every write below. These fixtures live in a temp dir
        # and cannot corrupt a tracked file, so `scripts/check-text-mode-writes.py`
        # is what forces the keyword here -- but it is right on the merits too:
        # the fixtures are Rust source that `check_callers` then matches line by
        # line, and a fixture whose bytes differ between Windows and Linux makes
        # this self-test grade something slightly different on each.
        (tmp / "posix" / "src").mkdir(parents=True)
        (tmp / "posix" / "src" / "ed25519.rs").write_text(
            "pub fn sign(m: &[u8]) -> [u8; 64] { [0; 64] }\n",
            encoding="utf-8", newline="",
        )
        (tmp / "posix" / "src" / "crypt.rs").write_text(
            'use crate::errno;\n'
            'pub fn buf() -> [u8; 8] { [0; 8] }\n'
            '#[cfg_attr(target_os = "none", unsafe(no_mangle))]\n'
            'pub extern "C" fn crypt_r(k: *const u8) -> *mut u8 {\n'
            "    errno::set_errno(errno::EINVAL);\n"
            "    core::ptr::null_mut()\n"
            "}\n",
            encoding="utf-8", newline="",
        )

        def crate(name: str, body: str, dep: str = 'posix = { path = "../../posix" }') -> None:
            d = tmp / "userspace" / name / "src"
            d.mkdir(parents=True)
            (tmp / "userspace" / name / "Cargo.toml").write_text(
                f'[package]\nname = "{name}"\n\n[dependencies]\n{dep}\n',
                encoding="utf-8", newline="",
            )
            (d / "main.rs").write_text(body, encoding="utf-8", newline="")

        crate("good", "fn f() { let _ = posix::ed25519::sign(b\"x\"); }\n")
        crate("stateful", "fn f() { posix::random::fill(&mut []); }\n")
        crate("cabi", "fn f() { posix::crypt::crypt_r(core::ptr::null()); }\n")
        crate("prose", "// posix::random::fill is what not to do\nfn f() {}\n")
        crate(
            "nodep",
            "fn f() { posix::random::fill(&mut []); }\n",
            dep='sha2 = { path = "../../sha2" }',
        )

        found = check_callers(tmp, ("userspace",))
        joined = " | ".join(found)
        expect("refuses a stateful module", "posix::random" in joined, True)
        expect("refuses a C entry point named as Rust", "crypt_r" in joined, True)
        expect("allows a pure module", "ed25519" in joined, False)
        expect("ignores prose in a comment", "prose" in joined, False)
        expect("ignores a crate that does not depend on posix", "nodep" in joined, False)
        expect("exactly two findings", len(found), 2)

        expect("an honest allowlist passes", check_allowlist(tmp), [])

        # Now make the allowlist dishonest the way it would rot in practice:
        # a Rust-native helper that touches per-thread state.
        (tmp / "posix" / "src" / "crypt.rs").write_text(
            "use crate::errno;\n"
            "pub fn buf() -> [u8; 8] { errno::set_errno(0); [0; 8] }\n",
            encoding="utf-8", newline="",
        )
        rotted = check_allowlist(tmp)
        expect("catches an allowlist that has rotted", len(rotted), 1)
        expect(
            "and says which line",
            "crypt.rs:2" in (rotted[0] if rotted else ""),
            True,
        )

        # A module on the allowlist that no longer exists is also a rot.
        (tmp / "posix" / "src" / "ed25519.rs").unlink()
        expect("catches a vanished module", len(check_allowlist(tmp)), 2)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    return 1 if failures else 0


def main(argv: list[str]) -> int:
    args = [a for a in argv[1:] if not a.startswith("--")]
    if "--self-test" in argv:
        print("check-one-libc-per-process --self-test")
        code = _self_test()
        print("PASSED" if code == 0 else "FAILED")
        return code

    root = Path(args[0]) if args else Path(__file__).resolve().parent.parent
    if not (root / "posix" / "src").is_dir():
        print(f"check-one-libc-per-process: no posix/src under {root}", file=sys.stderr)
        return 2

    problems = check_callers(root, DEFAULT_SCAN) + check_allowlist(root)
    for problem in problems:
        print(f"ERROR {problem}")
    if problems:
        print()
        print(f"check-one-libc-per-process: FAILED ({len(problems)} violation(s))")
        print(
            "A SlateOS program links the real libc already. Reach stateful posix "
            "through its C symbols, not as a Rust dependency."
        )
        print(
            "See design-decisions.md section 768 and known-issues.md "
            "TD-B-THE-POSIX-RLIB-IS-A-SECOND-LIBC-WITH-EVERY-SYSCALL-STUBBED-OUT."
        )
        return 1
    allowed = ", ".join(sorted(PURE_MODULES))
    print(f"check-one-libc-per-process: OK (pure modules: {allowed})")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
