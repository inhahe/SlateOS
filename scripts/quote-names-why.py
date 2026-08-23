#!/usr/bin/env python3
"""Derive the `--why` for `quote-names-wire.py` from the crate's own source.

`quote-names-wire.py` requires a rationale and gives it no default, because a
default is a blank that every crate silently shares. That is the right rule,
and it is why the first 213 crates of this burn-down were written by hand.

The 1-site tail is 444 crates, and hand-writing 444 more would not honour that
rule so much as parody it: they are the same shape, so what I would actually be
doing is reading a diff, recognising `unknown command '{}'`, and typing a
sentence I had already typed forty times. A sentence produced that way is not
evidence about the crate -- it is my recollection of a pattern, and the one
crate in forty that does not fit gets the same sentence as the rest.

So this derives the rationale instead, and derives it from the thing that
actually settles the question: **where the interpolated value was bound**. It
finds the identifier the new `quote*` call wraps and follows it backwards
through the binding forms these crates use -- `let`, `let`-else, `if let`,
`for` loops, plain assignment, `Vec::push`, match arms and function parameters
-- emitting a rationale only once it has reached `args`/`env::args()`. When it
cannot get there it prints nothing for that crate and reports it, so the crates
that need a human are exactly the ones a human is given.

The emitted text quotes *this* crate's message and names *this* crate's
identifier, so it is per-crate content in the sense the rule cares about -- and
unlike a hand-written one, the claim it makes ("this came from argv") has been
checked against the source rather than remembered.

    python scripts/quote-names-why.py userspace/apk-cli
    python scripts/quote-names-why.py --batch <file-of-crate-names>
    python scripts/quote-names-why.py --selftest

`--batch` prints `<crate>\t<why>` for each resolved crate and a final list of
unresolved ones on stderr.

**Scope, and what the claim is worth.** The search is scoped per *function*:
a local named `governor` in one function is not evidence about a different
function's `governor`, and the two exist in `cpupower`. Crossing a function
boundary is done only through the one edge that really carries a value -- a
parameter, resolved *by position* against each call site, so `run(&rest, prog)`
cannot resolve `args` by way of `prog`.

It is still a dataflow search and not a proof: an expression it cannot
decompose is accepted if the *text* of that expression reaches argv, so a
hypothetical `let n = args.len()` would be called argv-derived even though it
is a count rather than a word. That shape does not occur inside a diagnostic in
this tree -- a `quote*` call exists to wrap a name -- and the emitted sentence
names the identifier and quotes the message, so the one-line check is available
to whoever reads the manifest. What the tool buys is not certainty; it is that
the 444 sentences were each checked against the source instead of recalled from
the previous forty.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# A call this sweep adds. The wrapped expression may be a field path
# (`entry.path`) or an index (`args[i]`), both of which occur in this tail.
_QUOTE_CALL = re.compile(
    r"\bquote(?:a?f)(?:_os)?\(\s*&?\s*"
    r"(?P<expr>[A-Za-z_][A-Za-z0-9_.]*(?:\[[^\]\n]*\])?)\s*\)"
)

# The macro whose format string is being reported on, and a *single-line*
# string literal.
#
# Both anchors matter. An earlier version looked backwards for "the nearest
# preceding string literal holding a placeholder", with a character class that
# happened to admit newlines -- so a literal could start at one `"` and end at
# another forty lines below, and the "message" it reported was a slab of
# intervening code. The format string of a call is not "some string before the
# call": it is the first literal *inside the macro invocation that contains the
# call*, which is what these two find between them.
_MACRO = re.compile(r"\b(?:e?print(?:ln)?|format|write(?:ln)?)!\s*\(")
_STR = re.compile(r'"(?:[^"\\\n]|\\.)*"')

# A bound on the identifiers one search will visit. Cycles are already
# impossible -- an identifier is chased once per function -- so this only
# bounds the fan-out of the parameter case, where every call site contributes.
MAX_IDENTS = 40

# Reaching any of these means the value came in from the command line. `argv`
# is not a Rust identifier here but appears in the wrappers' own helper names.
_ARGV = re.compile(r"\b(?:args|argv|env::args|arguments)\b")


# --------------------------------------------------------------------------
# Lexical groundwork
#
# Every structural question below -- which brace encloses this arm, where this
# statement ends, which function contains this call -- is asked of a *blanked*
# copy of the source, in which comments and literals have been replaced by
# spaces of the same length. Offsets therefore still index the original, so
# text is always taken from the real source while structure is read off a copy
# that cannot be fooled by a brace inside a string or a `;` inside a comment.
# --------------------------------------------------------------------------

_CHAR_LIT = re.compile(r"'(?:\\.|[^'\\])'")


def blank(src: str) -> str:
    """`src` with comment and literal *contents* replaced by spaces."""
    out = list(src)
    i, n = 0, len(src)

    def erase(a: int, b: int) -> None:
        for k in range(a, min(b, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            j = src.find("\n", i)
            j = n if j == -1 else j
            erase(i, j)
            i = j
        elif c == "/" and i + 1 < n and src[i + 1] == "*":
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth += 1
                    j += 2
                elif src.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            erase(i, j)
            i = j
        elif c == "r" and i + 1 < n and src[i + 1] in '#"':
            k = i + 1
            while k < n and src[k] == "#":
                k += 1
            if k < n and src[k] == '"':
                hashes = "#" * (k - i - 1)
                end = src.find('"' + hashes, k + 1)
                end = n if end == -1 else end + 1 + len(hashes)
                erase(i, end)
                i = end
            else:
                i += 1
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            erase(i, j + 1)
            i = j + 1
        elif c == "'":
            # A char literal -- but `'a` in `&'a str` is a lifetime, and
            # blanking from it would swallow the rest of the file.
            m = _CHAR_LIT.match(src, i)
            if m:
                erase(i, m.end())
                i = m.end()
            else:
                i += 1
        else:
            i += 1
    return "".join(out)


def _match_brackets(bl: str, start: int, open_c: str, close_c: str) -> int:
    """Index just past the bracket opened at `start`, or `len(bl)`."""
    depth, i, n = 0, start, len(bl)
    while i < n:
        if bl[i] == open_c:
            depth += 1
        elif bl[i] == close_c:
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return n


def _split_top(bl: str, text: str) -> list[str]:
    """Split `text` at commas outside brackets; `bl` is its blanked twin."""
    parts, depth, start = [], 0, 0
    for i, c in enumerate(bl):
        if c in "([{<":
            depth += 1
        elif c in ")]}>":
            depth -= 1
        elif c == "," and depth == 0:
            parts.append(text[start:i])
            start = i + 1
    parts.append(text[start:])
    return [p.strip() for p in parts if p.strip()]


def _enclosing_brace(bl: str, at: int) -> int | None:
    """Index of the innermost `{` still open at offset `at`."""
    stack: list[int] = []
    for i in range(at):
        c = bl[i]
        if c == "{":
            stack.append(i)
        elif c == "}" and stack:
            stack.pop()
    return stack[-1] if stack else None


class Fn:
    """One `fn` item: its name, parameter list and body range."""

    __slots__ = ("name", "params", "params_bl", "start", "end")

    def __init__(self, name: str, params: str, params_bl: str, start: int, end: int):
        self.name, self.params, self.params_bl = name, params, params_bl
        self.start, self.end = start, end

    def __repr__(self) -> str:  # pragma: no cover - debugging aid
        return f"<fn {self.name} {self.start}..{self.end}>"


_FN = re.compile(r"\bfn\s+(?P<name>\w+)\s*(?:<[^>{(]*>)?\s*\(")


def find_functions(src: str, bl: str) -> list[Fn]:
    fns = []
    for m in _FN.finditer(bl):
        pclose = _match_brackets(bl, m.end() - 1, "(", ")")
        body = bl.find("{", pclose)
        if body == -1:
            continue
        # A `where` clause or a return type of `impl Fn() -> T` would put a
        # `;` before the body; such an item has no body to search.
        if ";" in bl[pclose:body]:
            continue
        end = _match_brackets(bl, body, "{", "}")
        fns.append(
            Fn(
                m.group("name"),
                src[m.end() : pclose - 1],
                bl[m.end() : pclose - 1],
                body,
                end,
            )
        )
    return fns


def enclosing_fn(fns: list[Fn], at: int) -> Fn | None:
    best = None
    for f in fns:
        if f.start <= at < f.end and (best is None or f.start > best.start):
            best = f
    return best


# --------------------------------------------------------------------------
# Where a value came from
# --------------------------------------------------------------------------

# An expression that is one identifier plus a chain of field accesses, indexes
# and no-argument method calls: `sub`, `&sub`, `sub.as_str()`, `args.first()`,
# `args[i]`, `entry.path.clone()`.
_SIMPLE = re.compile(
    r"\s*[&*]*\s*(?P<root>[A-Za-z_][A-Za-z0-9_]*)"
    r"(?:\s*\.\s*[A-Za-z_][A-Za-z0-9_]*(?:\s*\(\s*\))?|\s*\[[^\]]*\])*\s*\??\s*"
)

# The receiver of a method chain whose calls *do* take arguments, which is the
# same value flowing on: `query.join("/")`, `args.get(1).unwrap_or("")`.
_RECEIVER = re.compile(r"\s*[&*]*\s*(?P<root>[A-Za-z_][A-Za-z0-9_]*)\s*(?=[.\[])")

# Words that read as an identifier at the head of an expression but name no
# value that can be chased.
_NOT_A_VALUE = {"if", "match", "while", "for", "loop", "return", "Some", "Ok"}


def _root_of(expr: str) -> str | None:
    """The identifier an expression is *about*, or `None` if it is not one."""
    m = _SIMPLE.fullmatch(expr)
    if m is None:
        m = _RECEIVER.match(expr)
    if m is None:
        return None
    root = m.group("root")
    return None if root in _NOT_A_VALUE else root


# `let`, `if let`, `while let` -- the pattern is whatever precedes the `=`, so
# `let Some(sub) = args.first() else` binds `sub` just as `let sub = ...` does.
# The `else` arm of a `let`-else is the *failure* path, so the value is still
# the right-hand side, which is where the scan stops.
_LET = re.compile(r"\b(?:(?:if|while)\s+let\s+|let\s+)(?P<pat>[^=;{}]*?)=\s*")
_FOR = re.compile(r"\bfor\s+(?P<pat>[^\s{][^{;]*?)\s+in\s+")
_PUSH = re.compile(r"\.\s*(?:push|insert|extend|push_str)\s*\(")


# An expression whose value is produced by a *block*, so its `{` opens the
# value rather than ending the statement:
#
#     let subargs: Vec<String> = if cmd == "nextest" { args[1..].to_vec() }
#                                else { args };
#
# `cargo-nextest` strips its own name this way, and stopping at the first `{`
# would report the right-hand side as `if cmd == "nextest"` -- the condition,
# which is the one part of that statement the value does not come from.
_BLOCK_EXPR = re.compile(r"\s*(?:(?:if|match|loop|unsafe|while)\b|\{)")


def _stmt_end(bl: str, at: int) -> int:
    """Where the expression beginning at `at` ends."""
    block = _BLOCK_EXPR.match(bl, at) is not None
    depth = 0
    for i in range(at, len(bl)):
        c = bl[i]
        if c in "([":
            depth += 1
        elif c in ")]":
            depth -= 1
        elif block and c == "{":
            depth += 1
        elif block and c == "}":
            depth -= 1
        elif depth == 0:
            if c == ";" or (c == "{" and not block):
                return i
            if not block and bl.startswith("else", i) and not bl[i - 1].isalnum():
                return i
    return len(bl)


def _param_index(params_bl: str, params: str, ident: str) -> int | None:
    """Which argument of a call fills the parameter named `ident`."""
    idx = 0
    for p in _split_top(params_bl, params):
        if re.fullmatch(r"&?\s*(?:mut\s+)?self", p.strip()):
            continue  # a receiver is not an argument at the call site
        if re.match(rf"\s*(?:mut\s+)?{re.escape(ident)}\s*:", p):
            return idx
        idx += 1
    return None


class Tracer:
    """Answers "did this value come from the command line?" for one file."""

    def __init__(self, src: str):
        self.src = src
        self.bl = blank(src)
        self.fns = find_functions(src, self.bl)

    # -- the binding forms, in the order they occur in this tail ------------

    def bindings(self, ident: str, fn: Fn) -> list[tuple[str, Fn]]:
        """Every expression that could give `ident` its value inside `fn`."""
        out: list[tuple[str, Fn]] = []
        body_bl = self.bl[fn.start : fn.end]
        base = fn.start
        word = rf"\b{re.escape(ident)}\b"

        # `let x = e;`, `let Some(x) = e else { … }`, `if let Some(x) = e {`
        for m in _LET.finditer(body_bl):
            if re.search(word, m.group("pat")):
                a = base + m.end()
                out.append((self.src[a : base + _stmt_end(body_bl, m.end())], fn))

        # `for x in e {` -- how a per-argument loop hands each name to a body.
        for m in _FOR.finditer(body_bl):
            if re.search(word, m.group("pat")):
                a = base + m.end()
                out.append((self.src[a : base + _stmt_end(body_bl, m.end())], fn))

        # `x = e;` -- an accumulator declared `let mut x = String::new()` and
        # filled in from the argument loop, which is how `coredumpctl` takes
        # `--output` and `cpupower` takes its governor.
        for m in re.finditer(rf"(?<![=!<>]){word}\s*=(?!=)\s*", body_bl):
            if re.search(r"\blet\s+(?:mut\s+)?$", body_bl[: m.start()]):
                continue
            a = base + m.end()
            out.append((self.src[a : base + _stmt_end(body_bl, m.end())], fn))

        # `x.push(e)` -- a `Vec` accumulator is bound by what is pushed into
        # it, not by the `Vec::new()` it started as. `cgroup` collects its
        # group names this way.
        for m in _PUSH.finditer(body_bl):
            head = body_bl[: m.start()]
            if not re.search(word + r"\s*$", head):
                continue
            close = _match_brackets(body_bl, m.end() - 1, "(", ")")
            a, b = base + m.end(), base + close - 1
            out.append((self.src[a:b], fn))

        # A match arm binder: `other => …`, or with a guard, which is how a
        # dispatcher separates a positional argument from an option:
        # `s if !s.starts_with('-') => groups.push(s.to_string())` is
        # `cgroup`'s. The value is the scrutinee, found through the brace that
        # opens the arm list rather than by taking the nearest `match` above --
        # an arm containing its own `match` is common, and "nearest" picks that
        # inner one and loses the trail.
        arm = rf"(?m)^\s*{re.escape(ident)}\s*(?:if\b[^\n]*?)?=>"
        for m in re.finditer(arm, body_bl):
            brace = _enclosing_brace(body_bl, m.start())
            if brace is None:
                continue
            mm = re.search(r"\bmatch\s+(?P<v>[^\n]*?)\s*$", body_bl[:brace])
            if mm:
                out.append((self.src[base + mm.start("v") : base + mm.end("v")], fn))

        # A parameter: the matching argument of each call site, matched by
        # position and resolved in the *caller's* scope.
        i = _param_index(fn.params_bl, fn.params, ident)
        if i is not None:
            for call in re.finditer(rf"\b{re.escape(fn.name)}\s*\(", self.bl):
                if fn.start <= call.start() < fn.end:
                    continue  # a recursive call adds nothing
                close = _match_brackets(self.bl, call.end() - 1, "(", ")")
                a, b = call.end(), close - 1
                argv = _split_top(self.bl[a:b], self.src[a:b])
                if i < len(argv):
                    caller = enclosing_fn(self.fns, call.start())
                    if caller is not None:
                        out.append((argv[i], caller))
        return out

    def traces_to_argv(self, expr: str, at: int) -> bool:
        fn = enclosing_fn(self.fns, at)
        if fn is None:
            return False
        stack = [(expr, fn)]
        seen: set[tuple[int, str]] = set()
        while stack:
            e, f = stack.pop()
            root = _root_of(e)
            if root is None:
                # Not decomposable. Accept only if the expression itself
                # reaches argv; see the module docstring on what that is worth.
                if _ARGV.search(e):
                    return True
                continue
            if _ARGV.search(root):
                return True
            key = (f.start, root)
            if key in seen:
                continue
            seen.add(key)
            if len(seen) > MAX_IDENTS:
                return False
            for b, bf in self.bindings(root, f):
                if _ARGV.search(b):
                    return True
                stack.append((b, bf))
        return False

    def format_string(self, at: int) -> str | None:
        """The format string of the macro call containing offset `at`."""
        macro = None
        for m in _MACRO.finditer(self.bl, 0, at):
            macro = m
        if macro is None:
            return None
        lit = _STR.search(self.src, macro.end(), at)
        return lit.group(0)[1:-1] if lit else None


# Words in the message that mean the value is a *file name* rather than just
# an argument. The distinction changes what the comment should warn about: a
# path may hold a newline and so can forge a line of output, which is the
# whole point, whereas a subcommand name is merely untrusted.
_PATHY = re.compile(
    r"\b(?:file|path|opening|editing|rendering|processing|reading|writing|"
    r"written|dump|script|config|directory|crontab|from|to)\b",
    re.I,
)


def why_for(crate: Path) -> tuple[str | None, str]:
    """`(rationale, note)` for `crate`, or `(None, reason)`."""
    main = crate / "src" / "main.rs"
    if not main.is_file():
        return None, "no src/main.rs"
    src = main.read_text(encoding="utf-8")
    t = Tracer(src)
    calls = [m for m in _QUOTE_CALL.finditer(src) if t.bl[m.start()] != " "]
    if not calls:
        # Almost always an ordering slip rather than a real finding: this
        # reads the calls `quote-names.py --fix` has already written, so a
        # crate that has not been through `--fix` yet has nothing to explain
        # and reports as unresolved en masse.
        return None, "no quote call yet -- run `quote-names.py --fix` first"

    parts = []
    for c in calls:
        expr = c.group("expr")
        if not t.traces_to_argv(expr, c.start()):
            return None, f"cannot trace `{expr}` to argv"
        msg = t.format_string(c.start())
        if msg is None:
            return None, f"no format string around `{expr}`"
        # Collapse the escapes so the comment reads as the message does.
        msg = msg.replace('\\"', '"').replace("\\n", " ").strip()
        parts.append((msg, expr))

    seen = set()
    lines = []
    for msg, expr in parts:
        if (msg, expr) in seen:
            continue
        seen.add((msg, expr))
        kind = (
            "a path from the command line, and a path may hold any byte but "
            "`/` and NUL -- a newline included"
            if _PATHY.search(msg)
            else "an argv word, so it is whatever the user typed and not text "
            "this program chose"
        )
        lines.append(f'It interpolates `{expr}` into "{msg}". That value is {kind}.')
    return " ".join(lines), ""


# --------------------------------------------------------------------------
# Selftest -- every case is a shape taken from a crate in this tail, named
# after it, so a regression names the crate that will start failing.
# --------------------------------------------------------------------------


def selftest() -> int:
    fails = 0

    def check(name: str, src: str, want: bool) -> None:
        nonlocal fails
        t = Tracer(src)
        m = _QUOTE_CALL.search(src)
        if m is None:
            print(f"FAIL {name}: no quote call in the fixture", file=sys.stderr)
            fails += 1
            return
        got = t.traces_to_argv(m.group("expr"), m.start())
        if got != want:
            print(f"FAIL {name}: traced={got} want={want}", file=sys.stderr)
            fails += 1

    # apk-cli: a `let` straight off `args`.
    check(
        "let-off-args",
        "fn main() {\n"
        "    let args: Vec<String> = env::args().collect();\n"
        '    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");\n'
        '    println!("apk: {} completed", quoteaf_os(&subcmd));\n'
        "}\n",
        True,
    )
    # adyen-cli: `let`-else, through a match arm and a function parameter.
    # The most common shape in the tail.
    check(
        "let-else-through-param",
        "fn run(args: &[String], prog: &str) -> i32 {\n"
        "    let Some(sub) = args.first() else { return 0; };\n"
        "    match sub.as_str() {\n"
        '        "help" => 0,\n'
        "        other => {\n"
        '            eprintln!("{prog}: unknown subcommand {}", quoteaf_os(&other));\n'
        "            2\n"
        "        }\n"
        "    }\n"
        "}\n"
        "fn main() {\n"
        "    let args: Vec<String> = env::args().collect();\n"
        "    let rest: Vec<String> = args.into_iter().skip(1).collect();\n"
        "    process::exit(run(&rest, &prog));\n"
        "}\n",
        True,
    )
    # cockroachdb: the enclosing `match` is not the nearest one above the arm,
    # because an earlier arm holds a `match` of its own.
    check(
        "arm-after-a-nested-match",
        "fn run(args: Vec<String>) -> i32 {\n"
        '    let cmd = args.first().cloned().unwrap_or_default();\n'
        "    match cmd.as_str() {\n"
        '        "sql" => match sub.as_str() {\n'
        '            "x" => 0,\n'
        "            _ => 1,\n"
        "        },\n"
        '        other => { eprintln!("db: unknown command {}", quoteaf_os(&other)); 1 }\n'
        "    }\n"
        "}\n",
        True,
    )
    # coredumpctl: an accumulator filled by assignment inside the argument
    # loop, whose `let` says only `String::new()`.
    check(
        "assignment-into-an-accumulator",
        "fn main() {\n"
        "    let args: Vec<String> = env::args().collect();\n"
        "    let mut output_path = String::new();\n"
        "    let mut i = 0;\n"
        "    while i < args.len() {\n"
        '        if args[i] == "--output" { i += 1; output_path = args[i].clone(); }\n'
        "        i += 1;\n"
        "    }\n"
        '    println!("Core dump written to {}", quoteaf_os(&output_path));\n'
        "}\n",
        True,
    )
    # cgroup: a `Vec` accumulator, bound by what is pushed rather than by the
    # `Vec::new()` it started as, then handed out by a `for` loop.
    check(
        "push-then-for-loop",
        "fn main() {\n"
        "    let args: Vec<String> = env::args().collect();\n"
        "    let mut groups: Vec<String> = Vec::new();\n"
        "    for s in &args { groups.push(s.to_string()); }\n"
        "    for group in &groups {\n"
        '        eprintln!("cgset: {}", quotef_os(&group));\n'
        "    }\n"
        "}\n",
        True,
    )
    # cargo-nextest: the right-hand side is an `if`/`else`, so its `{` opens
    # the value instead of ending the statement. Truncating at that brace
    # leaves only the *condition*, which is the one part of the statement the
    # value does not come from.
    check(
        "let-from-a-block-expression",
        "fn run(args: Vec<String>) -> i32 {\n"
        '    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");\n'
        '    let subargs: Vec<String> = if cmd == "nextest" {\n'
        "        args[1..].to_vec()\n"
        "    } else {\n"
        "        args\n"
        "    };\n"
        '    let subcmd = subargs.first().map(|s| s.as_str()).unwrap_or("");\n'
        '    eprintln!("Error: unknown command {}.", quoteaf_os(&subcmd));\n'
        "    1\n"
        "}\n",
        True,
    )
    # cgroup again: the arm that collects a positional argument carries a
    # guard, which sits between the binder and the `=>`.
    check(
        "match-arm-with-a-guard",
        "fn cmd(args: &[String]) {\n"
        "    let mut groups: Vec<String> = Vec::new();\n"
        "    let mut i = 0;\n"
        "    while i < args.len() {\n"
        "        match args[i].as_str() {\n"
        "            s if !s.starts_with('-') => { groups.push(s.to_string()); }\n"
        "            _ => {}\n"
        "        }\n"
        "        i += 1;\n"
        "    }\n"
        "    for group in &groups {\n"
        '        eprintln!("cgset: {}", quotef_os(&group));\n'
        "    }\n"
        "}\n",
        True,
    )
    # cheat-sh: a method call *with* arguments still passes its receiver's
    # value on.
    check(
        "receiver-of-a-method-with-args",
        "fn main() {\n"
        "    let args: Vec<String> = env::args().collect();\n"
        '    let topic = args.join("/");\n'
        '    println!("(cheat sheet for {})", quoteaf_os(&topic));\n'
        "}\n",
        True,
    )
    # The positional match is what makes the parameter edge honest: a
    # parameter filled from a constant must not resolve merely because some
    # other parameter of the same call came from argv.
    check(
        "param-position-is-respected",
        "fn run(args: &[String], label: &str) -> i32 {\n"
        '    eprintln!("x: {}", quoteaf_os(&label));\n'
        "    0\n"
        "}\n"
        "fn main() {\n"
        "    let args: Vec<String> = env::args().collect();\n"
        '    process::exit(run(&args, "built-in"));\n'
        "}\n",
        False,
    )
    # cpupower: two functions, each with a local called `governor`. Only one
    # of them comes from argv, and the other must not borrow its provenance.
    check(
        "same-name-in-another-function",
        "fn probe(base: &str) -> Cpu {\n"
        "    let governor = read_sysfs_string(base);\n"
        '    println!("Setting governor {}", quoteaf_os(&governor));\n'
        "    Cpu { governor }\n"
        "}\n"
        "fn set(args: &[String]) {\n"
        "    let mut governor: Option<String> = None;\n"
        "    governor = Some(args[1].clone());\n"
        "}\n",
        False,
    )
    # A value the program chose itself does not become argv-derived by sitting
    # next to something that is.
    check(
        "program-owned-value",
        "fn main() {\n"
        "    let args: Vec<String> = env::args().collect();\n"
        '    let name = "slate";\n'
        '    eprintln!("x: {}", quoteaf_os(&name));\n'
        "}\n",
        False,
    )
    # A `quote*` call that only *looks* like one because it sits inside a
    # string literal is not a site at all.
    t = Tracer('fn f() {\n    println!("write quoteaf_os(x) to wrap it");\n}\n')
    if [m for m in _QUOTE_CALL.finditer(t.src) if t.bl[m.start()] != " "]:
        print("FAIL call-inside-a-string-literal", file=sys.stderr)
        fails += 1

    # The format string is the one inside the enclosing macro, not the nearest
    # `"` anywhere above it -- the defect that made this tool emit slabs of
    # code as "the message".
    for name, src, want in [
        (
            "format-string-is-the-enclosing-one",
            'fn f() {\n    println!("a banner with a { in it");\n'
            "    if x {\n"
            '        eprintln!("cut: cannot open {}: {}", quotef_os(path), e);\n'
            "    }\n}\n",
            "cut: cannot open {}: {}",
        ),
        (
            "format-string-across-lines",
            "fn f() {\n    eprintln!(\n"
            '        "tar: {}: unsupported type flag {}",\n'
            "        quotef_os(&entry.path),\n"
            "        quoteaf(&entry.typeflag)\n    );\n}\n",
            "tar: {}: unsupported type flag {}",
        ),
    ]:
        t = Tracer(src)
        got = t.format_string(_QUOTE_CALL.search(src).start())
        if got != want:
            print(f"FAIL {name}: {got!r}", file=sys.stderr)
            fails += 1

    print("selftest failed" if fails else "selftest ok", file=sys.stderr)
    return 1 if fails else 0


def main() -> int:
    # This tool's stdout is a *data file* -- `quote-names-wire.py --batch`
    # reads it back as UTF-8 -- so its encoding is the format's, not the
    # console's. Left to Python's Windows default, a message containing an em
    # dash (`blender-cli`, `cheat-sh` and a dozen others) either dies with a
    # UnicodeEncodeError or, worse, survives as a `?` that then goes into a
    # manifest comment as the crate's own words.
    for stream in (sys.stdout, sys.stderr):
        stream.reconfigure(encoding="utf-8", errors="backslashreplace")

    ap = argparse.ArgumentParser()
    ap.add_argument("target", type=Path, nargs="?")
    ap.add_argument("--batch", action="store_true", help="target is a list of crates")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()

    if a.selftest:
        return selftest()
    if a.target is None:
        ap.error("a target is required unless --selftest is given")

    if not a.batch:
        why, note = why_for(a.target if a.target.is_absolute() else ROOT / a.target)
        if why is None:
            print(note, file=sys.stderr)
            return 1
        print(why)
        return 0

    names = [n for n in a.target.read_text().split() if n]
    unresolved = []
    for name in names:
        why, note = why_for(ROOT / "userspace" / name)
        if why is None:
            unresolved.append(f"{name}: {note}")
            continue
        print(f"{name}\t{why}")
    if unresolved:
        print(f"\n{len(unresolved)} unresolved:", file=sys.stderr)
        for u in unresolved:
            print(f"  {u}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
