#!/usr/bin/env python3
"""Refuse a warning in the libc's own sources, compiled as it ships.

`toolchain/build-sysroot.ps1` builds libc.a from `posix` and libstubs.a from
`toolchain/stubs`, for `posix/x86_64-slateos-libc.json`, and every userspace
program links both. That build prints its warnings and nothing reads them:
the script judges by cargo's exit status, as the push hook's gate 40 does of
its own compile of posix, and a warning exits 0. The host's tests and clippy
-- what is run on every change to posix -- cannot see them either, because
the code a target-only warning is about is `cfg(target_os = "none")`, which
a host build does not compile.

Found on 2026-09-30: `signal::dispatch_delivered`, dead on the target since
c11db7de7 moved the trampoline's dispatch onto the kernel's frame -- the
posix crate's one warning, printed by every sysroot build for six commits
behind the thirty-two the vendored libm prints, and fixed in dfc037260.

WHAT IT CHECKS
--------------
Each crate build-sysroot.ps1 builds -- read out of it: each `Push-Location`
whose block runs `cargo +nightly build` -- checked with the settings it is
built with: `$sysrootFlags` as RUSTFLAGS, `$buildStd`, `--release`, `$spec`,
and the script's `$env:` settings, as `cargo check --message-format=json`.
The script's cargo line must be the one mirrored here, or there is no
verdict: a flag added there and not here would make this check something
other than what ships.

A warning or an error whose crate is the tree's own -- its manifest under
this repository and not under a `vendor/` directory -- is a finding: posix,
toolchain/stubs, and what they depend on in the tree (tzrules). An error in
any crate is one too, as the libc then does not build.

NOT COUNTED: the toolchain's crates (core and compiler_builtins, built from
rust-src by -Zbuild-std) and vendored ones (posix/vendor/libm, rust-lang's
port of musl's libm: 32 warnings on the nightly of 2026-09-30, upstream's to
fix -- a vendored copy is kept as upstream wrote it). Their count is printed,
so that it is seen to be what it was.

A check, not a build: a lint that fires only at code generation is not seen.
Those are few (`large_assignments` and its kind), and a build of posix is
minutes where the check is seconds.

    python scripts/check-libc-target-warnings.py
    python scripts/check-libc-target-warnings.py --selftest

Exit: 0 no warning in the tree's own sources; 1 a warning or an error there,
or an error in any crate; 2 no verdict -- build-sysroot.ps1 could not be
read as this reads it, or cargo failed without a diagnostic, or ran out of
time; 3 could not run: no nightly toolchain with rust-src, which
build-sysroot.ps1 needs as well.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SYSROOT_SCRIPT = ROOT / "toolchain" / "build-sysroot.ps1"
PROG = "check-libc-target-warnings"

FLAGS_RE = re.compile(r'^\$sysrootFlags\s*=\s*"([^"]*)"\s*$', re.M)
BUILD_STD_RE = re.compile(r'^\$buildStd\s*=\s*"([^"]*)"\s*$', re.M)
SPEC_RE = re.compile(r'^\$spec\s*=\s*Join-Path\s+\$root\s+"([^"]+)"\s*$', re.M)
ENV_RE = re.compile(r'^\$env:(\w+)\s*=\s*"([^"]*)"\s*$', re.M)
RUSTFLAGS_RE = re.compile(r"^\$env:RUSTFLAGS\s*=\s*\$sysrootFlags\s*$", re.M)
PUSH_RE = re.compile(r'^Push-Location\s+\(Join-Path\s+\$(root|PSScriptRoot)\s+"([^"]+)"\)\s*$',
                     re.M)
POP_RE = re.compile(r"^Pop-Location\b", re.M)
CARGO_RE = re.compile(r"^cargo\s.*$", re.M)
# The one cargo line this mirrors, as `cargo +nightly check`.
MIRRORED = "cargo +nightly build --release --target $spec $buildStd"

# rustc's closing summaries, which are about the diagnostics before them and
# are not diagnostics of their own.
SUMMARY_RE = re.compile(r"^(aborting due to|\d+ warnings? emitted|could not compile)")


class PlanError(Exception):
    """build-sysroot.ps1 does not read as this reads it."""


@dataclass
class Plan:
    """How build-sysroot.ps1 builds the sysroot's crates."""

    crates: list[Path]
    spec: Path
    rustflags: str
    build_std: list[str]
    env: dict[str, str]


def read_plan(text: str, root: Path) -> Plan:
    """build-sysroot.ps1's crates and settings, from its text."""
    flags = FLAGS_RE.search(text)
    std = BUILD_STD_RE.search(text)
    spec = SPEC_RE.search(text)
    if not (flags and std and spec):
        missing = [n for n, m in (("$sysrootFlags", flags), ("$buildStd", std),
                                  ("$spec", spec)) if not m]
        raise PlanError("build-sysroot.ps1 assigns no " + ", no ".join(missing)
                        + " as this reads it")
    if not RUSTFLAGS_RE.search(text):
        raise PlanError("build-sysroot.ps1 does not set $env:RUSTFLAGS = $sysrootFlags, "
                        "which this mirrors")
    here = {"root": root, "PSScriptRoot": root / "toolchain"}
    crates = []
    for push in PUSH_RE.finditer(text):
        pop = POP_RE.search(text, push.end())
        block = text[push.end():pop.start() if pop else len(text)]
        lines = [m.group(0).strip() for m in CARGO_RE.finditer(block)]
        if not lines:
            continue
        for line in lines:
            if line != MIRRORED:
                raise PlanError(f"build-sysroot.ps1 builds {push.group(2)} with `{line}`, "
                                f"and this mirrors `{MIRRORED}` -- make the two agree")
        crates.append(here[push.group(1)] / push.group(2).replace("\\", "/"))
    if not crates:
        raise PlanError("build-sysroot.ps1 builds no crate as this reads it")
    env = {k: v for k, v in ENV_RE.findall(text) if k != "RUSTFLAGS"}
    return Plan(crates=crates, spec=root / spec.group(1).replace("\\", "/"),
                rustflags=flags.group(1), build_std=std.group(1).split(), env=env)


def first_party(manifest: str, root: Path) -> bool:
    """Whether a crate is the tree's own: its manifest under `root` and not
    under a `vendor/` directory."""
    if not manifest:
        return False
    try:
        rel = Path(manifest).resolve().relative_to(root.resolve())
    except (ValueError, OSError):
        return False
    return "vendor" not in rel.parts[:-1]


@dataclass
class Verdict:
    """What one or more checks said."""

    ours: list[str] = field(default_factory=list)
    errors_elsewhere: list[str] = field(default_factory=list)
    not_counted: dict[str, int] = field(default_factory=dict)
    unparsed: int = 0

    def add(self, other: Verdict) -> None:
        self.ours += other.ours
        self.errors_elsewhere += other.errors_elsewhere
        for k, n in other.not_counted.items():
            self.not_counted[k] = self.not_counted.get(k, 0) + n
        self.unparsed += other.unparsed

    def exit_code(self) -> int:
        return 1 if self.ours or self.errors_elsewhere else 0


def judge(lines: list[str], root: Path) -> Verdict:
    """The findings in cargo's `--message-format=json` lines."""
    v = Verdict()
    for line in lines:
        line = line.strip()
        if not line:
            continue
        try:
            m = json.loads(line)
        except json.JSONDecodeError:
            v.unparsed += 1
            continue
        if not isinstance(m, dict) or m.get("reason") != "compiler-message":
            continue
        msg = m.get("message") or {}
        level = str(msg.get("level", ""))
        text = str(msg.get("message", ""))
        if not (level == "warning" or level.startswith("error")) or SUMMARY_RE.match(text):
            continue
        rendered = str(msg.get("rendered") or f"{level}: {text}").rstrip()
        if first_party(str(m.get("manifest_path", "")), root):
            v.ours.append(rendered)
        elif level.startswith("error"):
            v.errors_elsewhere.append(rendered)
        else:
            name = str((m.get("target") or {}).get("name", "?"))
            v.not_counted[name] = v.not_counted.get(name, 0) + 1
    return v


def toolchain_missing() -> str | None:
    """Why the nightly toolchain build-sysroot.ps1 uses cannot run here, if
    it cannot."""
    try:
        r = subprocess.run(["rustc", "+nightly", "--print", "sysroot"], capture_output=True,
                           text=True, encoding="utf-8", errors="replace", timeout=300)
    except (OSError, subprocess.TimeoutExpired) as e:
        return f"cannot run `rustc +nightly` ({e})"
    if r.returncode != 0:
        first = (r.stderr.strip().splitlines() or ["no reason given"])[0]
        return f"no nightly toolchain: `rustc +nightly` says {first!r}"
    core = Path(r.stdout.strip()) / "lib" / "rustlib" / "src" / "rust" / "library" / "core"
    if not (core / "Cargo.toml").is_file():
        return ("the nightly toolchain has no rust-src, which -Zbuild-std needs "
                "(rustup component add rust-src --toolchain nightly)")
    return None


def check_crate(plan: Plan, crate: Path, timeout: int) -> tuple[Verdict | None, str]:
    """`cargo check` of one crate as build-sysroot.ps1 builds it: its verdict,
    or None and why there is none."""
    cmd = ["cargo", "+nightly", "check", "--release", "--target", str(plan.spec),
           *plan.build_std, "--message-format=json"]
    env = dict(os.environ)
    # RUSTFLAGS is what build-sysroot.ps1 sets; CARGO_ENCODED_RUSTFLAGS
    # would take precedence over it, so a caller's must not.
    env.pop("CARGO_ENCODED_RUSTFLAGS", None)
    env["RUSTFLAGS"] = plan.rustflags
    env.update(plan.env)
    try:
        # errors="replace": the output is shown, never used as a name, and
        # cargo's JSON is UTF-8 -- a byte that is not would be rustc's
        # rendering of one, which a reader is better off seeing than losing
        # the whole verdict to.
        proc = subprocess.run(cmd, cwd=crate, env=env, capture_output=True, text=True,
                              encoding="utf-8", errors="replace", timeout=timeout)
    except subprocess.TimeoutExpired:
        return None, f"`cargo check` of {crate} ran past {timeout}s"
    except OSError as e:
        return None, f"cannot run cargo in {crate}: {e}"
    v = judge(proc.stdout.splitlines(), ROOT)
    if proc.returncode != 0 and not (v.ours or v.errors_elsewhere):
        tail = "\n".join(proc.stderr.strip().splitlines()[-15:])
        return None, (f"`cargo check` of {crate} exited {proc.returncode} without a "
                      f"diagnostic:\n{tail}")
    return v, ""


def describe(plan: Plan, root: Path) -> str:
    """The crates and settings, as one line says them."""
    names = ", ".join(c.relative_to(root).as_posix() for c in plan.crates)
    return (f"{names} as toolchain/build-sysroot.ps1 builds them "
            f"({plan.spec.relative_to(root).as_posix()}, --release, "
            f"RUSTFLAGS={plan.rustflags}, {' '.join(plan.build_std)})")


def selftest() -> int:
    failures = 0
    cases = 0

    def check(what: str, ok: bool) -> None:
        nonlocal failures, cases
        cases += 1
        if not ok:
            failures += 1
            print(f"FAIL: {what}")

    # The real script reads, and names posix -- the libc, which must never
    # leave this check without it being said.
    try:
        plan = read_plan(SYSROOT_SCRIPT.read_text(encoding="utf-8"), ROOT)
    except (OSError, PlanError) as e:
        check(f"build-sysroot.ps1 reads ({e})", False)
    else:
        rels = [c.relative_to(ROOT).as_posix() for c in plan.crates]
        check("build-sysroot.ps1's crates are posix and toolchain/stubs",
              rels == ["posix", "toolchain/stubs"])
        check("each crate is a directory with a manifest",
              all((c / "Cargo.toml").is_file() for c in plan.crates))
        check("its spec is a file", plan.spec.is_file())
        check("its RUSTFLAGS are read", plan.rustflags.startswith("-C "))
        check("its -Zbuild-std is read", plan.build_std[:1] != []
              and plan.build_std[0].startswith("-Zbuild-std="))
        check("its $env: settings are read, RUSTFLAGS aside",
              plan.env.get("CARGO_UNSTABLE_JSON_TARGET_SPEC") == "true"
              and "RUSTFLAGS" not in plan.env)

    fixture = "\n".join([
        '$root = Split-Path $PSScriptRoot -Parent',
        '$spec = Join-Path $root "posix\\x.json"',
        '$sysrootFlags = "-C codegen-units=4096"',
        '$env:CARGO_UNSTABLE_JSON_TARGET_SPEC = "true"',
        '$buildStd = "-Zbuild-std=core,compiler_builtins"',
        'Push-Location (Join-Path $root "posix")',
        '$env:RUSTFLAGS = $sysrootFlags',
        MIRRORED,
        'Pop-Location',
        'Push-Location (Join-Path $root "docs")',
        'Write-Host "no cargo here"',
        'Pop-Location',
        'Push-Location (Join-Path $PSScriptRoot "stubs")',
        MIRRORED,
        'Pop-Location',
        '',
    ])
    root = Path(tempfile.gettempdir()) / "slateos-tgtwarn-selftest"
    try:
        p = read_plan(fixture, root)
        check("a fixture's crates are the Push-Locations that run cargo, in order",
              [c.relative_to(root).as_posix() for c in p.crates]
              == ["posix", "toolchain/stubs"])
        check("its spec's backslash is a path separator",
              p.spec.relative_to(root).as_posix() == "posix/x.json")
    except PlanError as e:
        check(f"a fixture reads ({e})", False)
    for what, bad in [
        ("a cargo line with more on it", fixture.replace(MIRRORED, MIRRORED + " --features x", 1)),
        ("no $sysrootFlags", fixture.replace("$sysrootFlags = ", "$otherFlags = ")),
        ("RUSTFLAGS not from $sysrootFlags", fixture.replace("$env:RUSTFLAGS = $sysrootFlags",
                                                             "$env:RUSTFLAGS = \"-O\"")),
        ("no crate", fixture.replace("Push-Location", "Set-Location")),
    ]:
        try:
            read_plan(bad, root)
            check(f"{what} is no plan", False)
        except PlanError:
            check(f"{what} is no plan", True)

    def msg(manifest: Path | str, level: str, text: str, name: str = "posix") -> str:
        return json.dumps({"reason": "compiler-message", "manifest_path": str(manifest),
                           "target": {"name": name},
                           "message": {"level": level, "message": text,
                                       "rendered": f"{level}: {text}\n"}})

    here = ROOT
    outside = Path(tempfile.gettempdir()) / "rustlib" / "core" / "Cargo.toml"
    v = judge([
        msg(here / "posix" / "Cargo.toml", "warning", "function `f` is never used"),
        msg(here / "tzrules" / "Cargo.toml", "warning", "unused import", "tzrules"),
        msg(here / "posix" / "vendor" / "libm" / "Cargo.toml", "warning", "shadowed", "libm"),
        msg(here / "posix" / "vendor" / "libm" / "Cargo.toml", "warning", "loop", "libm"),
        msg(outside, "warning", "in core", "core"),
        msg(here / "posix" / "Cargo.toml", "warning", "1 warning emitted"),
        msg(here / "posix" / "Cargo.toml", "failure-note", "For more information"),
        json.dumps({"reason": "compiler-artifact", "target": {"name": "posix"}}),
        "not json at all",
        "",
    ], here)
    check("a warning in posix and one in tzrules are the tree's own",
          v.ours == ["warning: function `f` is never used", "warning: unused import"])
    check("the vendored libm's and core's are counted apart, not refused",
          v.not_counted == {"libm": 2, "core": 1} and v.errors_elsewhere == [])
    check("a summary, a failure-note and an artifact are not diagnostics",
          len(v.ours) == 2)
    check("a line that is not JSON is counted, not fatal", v.unparsed == 1)
    check("and the verdict refuses", v.exit_code() == 1)
    e = judge([msg(here / "posix" / "vendor" / "libm" / "Cargo.toml", "error", "E0308", "libm"),
               msg(here / "posix" / "vendor" / "libm" / "Cargo.toml", "error",
                   "aborting due to 1 previous error", "libm")], here)
    check("an error in a vendored crate refuses too, once",
          e.errors_elsewhere == ["error: E0308"] and e.exit_code() == 1)
    clean = judge([msg(here / "posix" / "vendor" / "libm" / "Cargo.toml", "warning", "x",
                       "libm")], here)
    check("vendored warnings alone pass", clean.exit_code() == 0)
    both = Verdict()
    both.add(v)
    both.add(clean)
    check("verdicts add", both.not_counted["libm"] == 3 and len(both.ours) == 2)
    check("a vendor directory deeper down is still vendored",
          not first_party(str(here / "a" / "vendor" / "b" / "Cargo.toml"), here))
    check("a name that only begins with vendor is not",
          first_party(str(here / "vendorish" / "Cargo.toml"), here))
    check("no manifest is not the tree's", not first_party("", here))

    print(f"{PROG} self-test: {cases - failures}/{cases} cases pass")
    return 1 if failures else 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    ap.add_argument("--selftest", "--self-test", dest="selftest", action="store_true")
    ap.add_argument("--timeout", type=int, default=1800,
                    help="seconds for each crate's check (default 1800)")
    args = ap.parse_args(argv)
    if args.selftest:
        return selftest()
    try:
        plan = read_plan(SYSROOT_SCRIPT.read_text(encoding="utf-8"), ROOT)
    except (OSError, PlanError) as e:
        print(f"{PROG}: no verdict: {e}")
        return 2
    why = toolchain_missing()
    if why:
        print(f"{PROG}: could not run: {why}")
        return 3
    total = Verdict()
    for crate in plan.crates:
        v, why = check_crate(plan, crate, args.timeout)
        if v is None:
            print(f"{PROG}: no verdict: {why}")
            return 2
        total.add(v)
    for block in total.ours + total.errors_elsewhere:
        print(block)
        print()
    elsewhere = ", ".join(f"{n} in {k}" for k, n in sorted(total.not_counted.items()))
    note = f" Not counted: {elsewhere} (vendored or the toolchain's)." if elsewhere else ""
    if total.unparsed:
        note += f" {total.unparsed} line(s) of cargo's output were not JSON."
    if total.exit_code():
        print(f"{PROG}: {len(total.ours)} warning(s) or error(s) in the tree's own sources, "
              f"{len(total.errors_elsewhere)} error(s) elsewhere -- {describe(plan, ROOT)}."
              + note)
        sys.stdout.flush()  # the findings before the advice, however run
        print("The target's build shows what the host's tests and clippy cannot: code under "
              "cfg(target_os = \"none\"). Fix each one; a warning a target build prints and no "
              "verdict reads is how the last one lasted six commits.", file=sys.stderr)
        return 1
    print(f"{PROG}: OK -- {describe(plan, ROOT)}: no warning in the tree's own sources." + note)
    return 0


if __name__ == "__main__":
    sys.exit(main())
