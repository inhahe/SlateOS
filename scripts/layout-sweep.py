#!/usr/bin/env python3
"""Measure how much of a benchmark's movement is caused by code *placement*.

# The problem

Every cross-commit benchmark comparison this project makes under QEMU's TCG
carries a confound that repetition cannot remove. TCG bounds a translation block
at the guest 4 KiB page, so a hot loop whose backward branch crosses a page
costs roughly 1.7x per iteration. Whether it crosses is a property of the loop's
*address*, and relinking the kernel moves the address of every function after
the file that was edited. So an unrelated one-line change can make an unrelated
benchmark 1.7x slower, and that slowdown replicates perfectly on every re-run --
which is exactly the signature the harness had been reading as "confirmed, not
noise, therefore caused by the diff". On 2026-08-19 that produced a run in which
ten of thirteen perfectly-replicating movers got *faster* on a scheduler-only
commit.

`scripts/bench-history.py` warns about this in prose and
`scripts/straddle-check.py` can confirm it after the fact for one named
function. Neither *measures* it, so neither can say how large a movement has to
be before it means anything.

# What this does

`kernel/src/layout_pad.rs` puts a build-time-selectable block of padding at the
very front of `.text`, chosen by the `SLATEOS_TEXT_PAD` environment variable.
Building the *same source* at several pad values produces kernels of identical
semantics whose code sits at different addresses. Benchmarking each one gives,
per benchmark, the spread attributable to placement alone -- its **layout
band**. A later movement inside that band is not evidence about a diff, however
perfectly it replicates.

    # Verify the mechanism actually works (builds only, no QEMU):
    python scripts/layout-sweep.py --self-test

    # Run the sweep (builds + a full --bench boot per pad; hours):
    python scripts/layout-sweep.py --pads 0,1024,2048,3072

Sweep runs are recorded to `bench/history.jsonl` tagged with
`BENCH_EXPERIMENT`, so they are stored and analysable but never become the
baseline for an ordinary run -- a padded kernel is not one any checkout
reproduces.

# Choosing pad values

**Not multiples of 4096.** A pad of exactly one page shifts every function by
exactly one page, preserving every page-straddle relationship in the kernel: it
would look like a perturbation and perturb nothing. The self-test below includes
that as an explicit negative control, because it is the single easiest way to
run a sweep that measures zero and read the zero as "layout does not matter
here".
"""

from __future__ import annotations

import argparse
import hashlib
import os
import platform
import re
import shutil
import subprocess
import sys

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_ROOT = os.path.dirname(SCRIPT_DIR)

#: Pad values used by `--self-test`. 3072 is deliberately not a multiple of
#: 4096 (see the module docstring); 4096 is, and is the negative control.
SELF_TEST_PAD = 3072
PAGE_PAD = 4096

#: Values that must make the build *fail*. A silent fallback to 0 would record
#: a duplicate of the baseline as if it were a distinct layout sample, which
#: would shrink the measured band -- i.e. fail in the direction that dismisses
#: real regressions.
BAD_PADS = {
    "1O24": "decimal byte count",   # capital O for zero
    "4k": "decimal byte count",     # a size suffix
    "-8": "decimal byte count",     # a sign
    "1048577": "capped",            # one past the cap
}

PAD_SYMBOL = "LAYOUT_PAD"


def toolchain_bin(name: str) -> str:
    """Locate an llvm-* tool from the rustup toolchain, or fall back to PATH.

    Same approach as `scripts/straddle-check.py`; these ship with rustup, so no
    binutils install is required.
    """
    roots = os.path.join(os.path.expanduser("~"), ".rustup", "toolchains")
    if os.path.isdir(roots):
        for toolchain in sorted(os.listdir(roots)):
            candidate = os.path.join(
                roots, toolchain, "lib", "rustlib",
                toolchain.split("-", 1)[-1], "bin", name)
            for path in (candidate, candidate + ".exe"):
                if os.path.isfile(path):
                    return path
    return name


def kernel_path(profile: str) -> str:
    sub = "release" if profile == "release" else "debug"
    return os.path.join(PROJECT_ROOT, "target", "x86_64-unknown-none", sub,
                        "kernel")


def build(pad: str | None, profile: str) -> subprocess.CompletedProcess:
    """Build the kernel crate at a given pad. Returns the completed process.

    `pad=None` means "leave `SLATEOS_TEXT_PAD` unset", which is a genuinely
    different case from `pad="0"` as far as cargo's fingerprinting is
    concerned, and is what every ordinary build does.
    """
    env = dict(os.environ)
    if pad is None:
        env.pop("SLATEOS_TEXT_PAD", None)
    else:
        env["SLATEOS_TEXT_PAD"] = pad
    cmd = ["cargo", "build", "-p", "kernel"]
    if profile == "release":
        cmd.append("--release")
    return subprocess.run(cmd, cwd=PROJECT_ROOT, env=env,
                          capture_output=True, text=True, errors="replace",
                          check=False)


_NM_RE = re.compile(r"^([0-9a-fA-F]+)\s+(\S)\s+(.+)$")


def symbols(binary: str) -> dict[str, tuple[int, str]]:
    """`{symbol: (address, nm_type)}` for every defined symbol.

    The type letter is kept because the shift analysis has to restrict itself
    to `.text` (`t`/`T`). `.rodata`, `.data` and `.bss` are each `ALIGN(4K)` in
    `linker.ld`, so a 3072-byte pad moves them by either 0 or 4096 depending on
    where `.text` happened to end. Mixing them into the shift distribution
    would make the honest assertion "everything moved by at least the pad" fail
    for a build that is entirely correct.

    Mangled names are kept verbatim -- unlike `straddle-check.py`, which
    demangles in order to match *across* builds, we compare two builds of
    identical source, so the mangled names are identical too and demangling
    would only lose the disambiguation between same-named statics.
    """
    nm = toolchain_bin("llvm-nm")
    proc = subprocess.run([nm, "--defined-only", binary],
                          capture_output=True, text=True, errors="replace",
                          check=False)
    if proc.returncode != 0:
        raise SystemExit(f"layout-sweep: {nm} failed ({proc.returncode}):\n"
                         f"{proc.stderr[:2000]}")
    out: dict[str, tuple[int, str]] = {}
    for line in proc.stdout.splitlines():
        match = _NM_RE.match(line.strip())
        if match:
            out[match.group(3)] = (int(match.group(1), 16), match.group(2))
    return out


def text_shifts(before: dict[str, tuple[int, str]],
                after: dict[str, tuple[int, str]]) -> dict[str, int]:
    """Per-symbol address delta, over `.text` symbols common to both builds.

    The pad static and its bracketing linker symbols are excluded, as is
    `__text_start` -- all four are section markers at or before the padding, so
    by construction they do not move and would only dilute the distribution.
    """
    out = {}
    skip = {"__text_start", "__layout_pad_start", "__layout_pad_end"}
    for name, (addr, kind) in after.items():
        if kind not in ("t", "T") or name not in before:
            continue
        if PAD_SYMBOL in name or name in skip:
            continue
        out[name] = addr - before[name][0]
    return out


def sha256(path: str) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


#: Source of the on-target self-test whose branches must all survive the
#: optimiser. Read, not copied -- see `on_target_messages`.
LAYOUT_PAD_SRC = os.path.join("kernel", "src", "layout_pad.rs")

#: The function inside it whose messages are checked.
LAYOUT_PAD_FN = "self_test_pad_is_first_in_text"

# `\\[\s\S]` rather than `\\.`, because the escape that matters most here is a
# backslash before a *newline* -- Rust's line continuation -- and `.` does not
# match a newline. With `\\.` the multi-line FAIL message was skipped entirely
# and the extractor found two branches instead of three.
_SERIAL_PRINTLN_RE = re.compile(r'serial_println!\s*\(\s*"((?:[^"\\]|\\[\s\S])*)"')


def on_target_messages(src_path: str | None = None,
                       root: str = PROJECT_ROOT) -> list[str]:
    """The longest literal fragment of each message `LAYOUT_PAD_FN` can print.

    # What this is for

    `self_test_pad_is_first_in_text` compares two linker symbols. LLVM
    guarantees distinct globals have distinct addresses, so on the 2026-08-19
    release sweep it answered the comparison itself -- always "different" --
    and deleted the success branch outright. The kernel then halted on every
    padded release boot. The evidence was that the success string did not
    occur anywhere in the binary while the failure string did.

    That is the check this reproduces: build a padded kernel, and require
    *every* branch's message to be present in the image. A branch the
    optimiser has folded away takes its string with it, because nothing else
    references it. So string-presence is a direct, cheap proxy for
    "the comparison is still being made at run time".

    # Why the strings are extracted rather than written down here

    A copy in this file would have to be kept in step with the kernel by hand,
    and the moment it drifts the check reports on a message the kernel no
    longer prints. Reading them from the source means rewording a message
    updates the check in the same edit, and *deleting a branch* -- the thing
    actually being guarded against -- is what makes the count drop.

    # Why the longest fragment

    `core::fmt` stores a format string as its literal pieces with the
    arguments interleaved, so `"[layout_pad] {pad} pad byte(s) ..."` is not
    contiguous in the binary. Searching for a whole formatted message would
    never match. The pieces between placeholders *are* contiguous, and the
    longest of them is both long enough to be unique and free of the shared
    `[layout_pad] ` prefix that all three messages start with.

    Raises `RuntimeError` rather than returning a short list if the function
    or its messages cannot be found: a check that silently verifies fewer
    branches than exist is the failure mode this whole module is about.
    """
    path = src_path or os.path.join(root, LAYOUT_PAD_SRC)
    with open(path, "r", encoding="utf-8") as handle:
        source = handle.read()

    marker = f"pub fn {LAYOUT_PAD_FN}("
    start = source.find(marker)
    if start < 0:
        raise RuntimeError(
            f"{path} no longer defines `{LAYOUT_PAD_FN}`, so this check does "
            f"not know what it is checking. Point LAYOUT_PAD_FN at whatever "
            f"replaced it -- do not delete the check.")
    # The function is the last item in the file; if that ever stops being
    # true, a following `\npub fn ` / `\nfn ` bounds it.
    rest = source[start + len(marker):]
    end = min((i for i in (rest.find("\npub fn "), rest.find("\nfn "),
                           rest.find("\npub const "))
               if i >= 0), default=-1)
    body = rest if end < 0 else rest[:end]

    fragments = []
    for literal in _SERIAL_PRINTLN_RE.findall(body):
        # Rust's line continuation: a backslash before a newline eats the
        # newline *and* the following indentation.
        text = re.sub(r"\\\s*\n\s*", "", literal)
        text = text.replace('\\"', '"').replace("\\\\", "\\")
        pieces = [p for p in re.split(r"\{[^{}]*\}", text) if p.strip()]
        if not pieces:
            raise RuntimeError(
                f"a `serial_println!` in `{LAYOUT_PAD_FN}` is entirely "
                f"placeholders ({literal!r}), so it leaves no literal in the "
                f"binary to look for.")
        fragments.append(max(pieces, key=len))

    if len(fragments) < 3:
        raise RuntimeError(
            f"expected at least 3 `serial_println!` messages in "
            f"`{LAYOUT_PAD_FN}` (unpadded / misplaced / OK) but found "
            f"{len(fragments)}: {fragments!r}. If a branch was legitimately "
            f"removed, update this count; if one was *optimised* away, that "
            f"is the bug this exists to catch and it has escaped into the "
            f"source.")
    if len(set(fragments)) != len(fragments):
        raise RuntimeError(
            f"two messages in `{LAYOUT_PAD_FN}` share their longest literal "
            f"fragment ({fragments!r}), so finding one in the binary would "
            f"not distinguish which branch survived. Reword one of them.")
    return fragments


def branches_present(binary: str, fragments: list[str]) -> list[str]:
    """Which of `fragments` are missing from `binary`. Empty means all present."""
    with open(binary, "rb") as handle:
        image = handle.read()
    return [f for f in fragments if f.encode("utf-8") not in image]


# ---------------------------------------------------------------------------
# --self-test
# ---------------------------------------------------------------------------

_FAILURES: list[str] = []


def check(label: str, got, want) -> None:
    if got == want:
        print(f"  ok   {label}")
    else:
        print(f"  FAIL {label}\n         got:  {got!r}\n         want: {want!r}")
        _FAILURES.append(label)


def check_true(label: str, got: bool, detail: str = "") -> None:
    if got:
        print(f"  ok   {label}")
    else:
        print(f"  FAIL {label}" + (f"\n         {detail}" if detail else ""))
        _FAILURES.append(label)


def self_test(profile: str) -> int:
    """Prove the padding mechanism does what the sweep assumes it does.

    Builds only -- no QEMU -- so this is minutes, not hours, and is the thing to
    re-run after any change to `layout_pad.rs`, `linker.ld` or `build.rs`.

    Four claims, none of which the sweep can check for itself once it is
    running:

      1. A malformed `SLATEOS_TEXT_PAD` fails the build rather than defaulting
         to 0. A sweep that silently built the baseline four times would report
         a layout band of zero and thereby certify every future false regression
         as real.
      2. A non-page-multiple pad actually moves the code, and by at least the
         pad's size.
      3. A pad of exactly 4096 moves everything by exactly 4096 -- the negative
         control. It is a *correct* build that is *useless* as a sample, and the
         only way to know that is to demonstrate it.
      4. The kernel's *own* on-target placement check still exists in the
         optimised image. Claims 1-3 all read the ELF, and on 2026-08-19 they
         all passed for a kernel that then halted on every boot: LLVM had
         folded the on-target comparison to a constant and deleted its success
         branch. An ELF that is correct says nothing about whether the code
         that inspects it at run time was kept.
    """
    print(f"[layout-sweep] self-test on the '{profile}' profile\n")

    # --- 1. Malformed values must fail the build -----------------------------
    print("[layout-sweep] rejection cases (each of these must FAIL to build):")
    for value, expected in BAD_PADS.items():
        proc = build(value, profile)
        combined = proc.stdout + proc.stderr
        check_true(f"SLATEOS_TEXT_PAD={value!r} fails the build",
                   proc.returncode != 0,
                   "the build SUCCEEDED, so a typo would silently produce an "
                   "unpadded kernel labelled as a padded one")
        check_true(f"...and says why ({expected!r})", expected in combined,
                   f"stderr tail: {combined[-400:]!r}")

    # --- 2. A real pad moves the code ---------------------------------------
    print("\n[layout-sweep] baseline (SLATEOS_TEXT_PAD unset):")
    proc = build(None, profile)
    check("the unpadded kernel builds", proc.returncode, 0)
    if proc.returncode != 0:
        print(proc.stderr[-2000:])
        return 1
    base_path = kernel_path(profile)
    base_sha = sha256(base_path)
    base_syms = symbols(base_path)
    base_copy = base_path + ".layout-baseline"
    shutil.copyfile(base_path, base_copy)
    print(f"  kernel_sha {base_sha[:16]}  ({len(base_syms)} defined symbols)")

    try:
        results = {}
        for pad in (SELF_TEST_PAD, PAGE_PAD):
            print(f"\n[layout-sweep] SLATEOS_TEXT_PAD={pad}:")
            proc = build(str(pad), profile)
            check(f"the pad={pad} kernel builds", proc.returncode, 0)
            if proc.returncode != 0:
                print(proc.stderr[-2000:])
                return 1
            path = kernel_path(profile)
            this_sha = sha256(path)
            this_syms = symbols(path)
            results[pad] = (this_sha, this_syms)
            print(f"  kernel_sha {this_sha[:16]}")

            check_true(f"pad={pad} produces a different image",
                       this_sha != base_sha,
                       "identical SHA-256: cargo reused the cached build, so "
                       "`cargo:rerun-if-env-changed=SLATEOS_TEXT_PAD` is "
                       "missing from kernel/build.rs")

            # The pad static must exist and sit at the very start of .text.
            pad_syms = [n for n in this_syms if PAD_SYMBOL in n]
            check_true(f"pad={pad} emits the {PAD_SYMBOL} static",
                       bool(pad_syms), f"no symbol containing {PAD_SYMBOL!r}")
            text_start = this_syms.get("__text_start")
            check_true("__text_start is defined", text_start is not None)
            if pad_syms and text_start is not None:
                check("...and the pad sits exactly there (linker.ld places "
                      "KEEP(*(.text.slateos_layout_pad)) first)",
                      this_syms[pad_syms[0]][0], text_start[0])

            # The compiler-said / linker-did cross-check. It lives here rather
            # than in the kernel because doing it on the target would mean
            # comparing a linker symbol against the Rust constant, and that
            # comparison compiles to a different immediate per pad -- which is
            # itself a code difference between arms, i.e. the confound this
            # whole tool exists to remove.
            lo_sym = this_syms.get("__layout_pad_start")
            hi_sym = this_syms.get("__layout_pad_end")
            check_true("the pad is bracketed by linker symbols",
                       lo_sym is not None and hi_sym is not None,
                       "__layout_pad_start/__layout_pad_end missing from "
                       "linker.ld; the kernel cannot then report its own pad "
                       "size and would have to trust the compiler's constant")
            if lo_sym is not None and hi_sym is not None:
                check(f"the linker emitted exactly the {pad} bytes the "
                      f"compiler was asked for",
                      hi_sym[0] - lo_sym[0], pad)

            # Everything above inspects the *ELF*, which is why it all passed
            # on 2026-08-19 while the kernel it described could not boot. The
            # kernel's own on-target check had been folded to a constant
            # "wrong" and its success path deleted, so the sweep died on its
            # second arm after a 43-minute build. See `on_target_messages`.
            missing = branches_present(path, on_target_messages())
            check_true(
                f"pad={pad}: every branch of {LAYOUT_PAD_FN}() survives the "
                f"optimiser",
                not missing,
                f"missing from the image: {missing!r}\n         "
                f"A branch whose message is absent was compiled out, which "
                f"means the comparison is no longer being made at run time -- "
                f"the check has become a constant. Read the two linker "
                f"symbols through layout_pad::opaque_addr().")

            # Compare the *distribution* of shifts rather than one sampled
            # function: a single symbol proves nothing about the rest of the
            # image, and the whole claim is that everything moved.
            shifts = text_shifts(base_syms, this_syms)
            distinct = set(shifts.values())
            moved = {s for s in distinct if s != 0}
            check_true(f"pad={pad} moved something",
                       bool(moved),
                       "no .text symbol changed address at all")
            if moved:
                lo, hi = min(distinct), max(distinct)
                unmoved = sum(1 for s in shifts.values() if s == 0)
                print(f"  shift across {len(shifts)} shared .text symbols: "
                      f"{lo:+d} .. {hi:+d} bytes ({unmoved} unmoved)")
                check_true(f"pad={pad} moved *every* .text function by at "
                           f"least the pad",
                           lo >= pad,
                           f"smallest shift {lo} is below the {pad}-byte pad: "
                           f"something in .text is placed ahead of the "
                           f"padding, so a sweep would perturb only part of "
                           f"the image")

        # --- 3. The page-multiple negative control --------------------------
        print("\n[layout-sweep] negative control:")
        _, page_syms = results[PAGE_PAD]
        page_shifts = set(text_shifts(base_syms, page_syms).values())
        check("a 4096-byte pad shifts every function by exactly one page, so "
              "it preserves every page-straddle relationship and is worthless "
              "as a sweep sample",
              page_shifts, {PAGE_PAD})
    finally:
        # Leave the tree holding the ordinary, unpadded kernel: a sweep
        # self-test that quietly left a padded binary in place would poison the
        # next boot test, which is exactly the class of invisible contamination
        # this whole file is about.
        shutil.move(base_copy, base_path)
        print(f"\n[layout-sweep] restored the unpadded kernel at {base_path}")

    print()
    if _FAILURES:
        print(f"[layout-sweep] {len(_FAILURES)} FAILED: "
              f"{'; '.join(_FAILURES)}")
        return 1
    print("[layout-sweep] self-test passed: the padding mechanism moves code, "
          "refuses malformed input, and\n               a page-sized pad is "
          "confirmed useless as a sample.")
    return 0


# ---------------------------------------------------------------------------
# the sweep proper
# ---------------------------------------------------------------------------

def bench_history():
    """Import `bench-history.py` by path (its name is not an identifier).

    Imported rather than reimplemented, deliberately, for the two facts this
    script needs from it: how to read a `textpad=` banner, and how many distinct
    layouts it takes before it will compute a band at all.

    A local copy of either would drift, and both drift *silently*. A stale copy
    of the banner regex would make this script's stale-build guard stop matching
    the parser that will actually read the log, so the sweep would certify runs
    the analyser cannot attribute. A hardcoded `3` would let this script run a
    two-arm sweep to completion -- forty minutes of QEMU per arm -- and produce
    a history from which no band can ever be computed, reported as a successful
    sweep. Both are the project's recurring failure: a check that cannot fire,
    presenting as a check that found nothing.
    """
    import importlib.util

    path = os.path.join(SCRIPT_DIR, "bench-history.py")
    spec = importlib.util.spec_from_file_location("bench_history", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


#: How the sweep names the boot test when it hands it to `bash`.
#:
#: **Repo-relative, resolved by the child's cwd -- never the absolute path**
#: `os.path.join(SCRIPT_DIR, ...)` would build. On Windows that absolute path is
#: `D:\visual studio projects\...\boot-test.sh`, which WSL's bash cannot open;
#: it says so as the generic `No such file or directory`, exit 127. That killed
#: the first real release sweep -- after its 34-minute self-test had passed, on
#: the first arm, with a message claiming a script that is plainly present was
#: missing.
#:
#: The relative name is still right, but note that it was only ever half the
#: story: the reason a *WSL* bash was reading it at all is `find_bash()` below,
#: and until that existed, fixing the path merely moved the failure two lines
#: down to "qemu-system-x86_64 not found".
BOOT_TEST = "scripts/boot-test.sh"

#: Where to look for a bash that can actually run the boot test, in order.
#:
#: `"bash"` is deliberately **last**, and that ordering is the entire point of
#: this list; see `find_bash()`.
BASH_CANDIDATES = (
    r"C:\Program Files\Git\usr\bin\bash.exe",
    r"C:\Program Files\Git\bin\bash.exe",
    r"C:\Program Files (x86)\Git\usr\bin\bash.exe",
    "/bin/bash",
    "bash",
)


def extract_dependency_probe(script_path: str | None = None) -> str:
    """The boot test's own QEMU and OVMF discovery, lifted out verbatim.

    Returned as a shell fragment that exits non-zero exactly when the boot test
    would exit non-zero for a missing dependency, and silently otherwise.

    Extracted rather than restated. The candidate lists are four MSYS-style and
    Windows-style paths that differ per machine and per bash flavour, and a
    Python translation of them would be a second opinion about where QEMU is --
    one that can say "found" while the boot test says "not found", which is the
    worst possible answer from a preflight because it certifies the sweep and
    then the sweep dies anyway. `test-boot-test.py` extracts the dirty check the
    same way and for the same reason.
    """
    path = script_path or os.path.join(PROJECT_ROOT, BOOT_TEST)
    with open(path, "r", encoding="utf-8", errors="replace") as handle:
        lines = handle.read().splitlines()

    try:
        start = next(i for i, line in enumerate(lines)
                     if line.strip() == "# Find QEMU")
    except StopIteration:
        raise RuntimeError(
            f"{path} no longer contains a '# Find QEMU' block. This probe "
            f"cannot be built, and rather than preflight nothing while looking "
            f"like it preflighted something, it refuses.") from None

    # Through the end of the OVMF guard: both dependencies, and nothing that
    # depends on state set earlier in the script.
    end = None
    for index in range(start, len(lines)):
        if lines[index].strip() == 'if [ -z "$OVMF" ]; then':
            for close in range(index, len(lines)):
                if lines[close].strip() == "fi":
                    end = close + 1
                    break
            break
    if end is None:
        raise RuntimeError(
            f"{path} has a '# Find QEMU' block but no OVMF guard after it, so "
            f"the extracted fragment would check half the dependencies while "
            f"reporting on all of them.")

    fragment = "\n".join(lines[start:end])
    for needed in ("qemu-system-x86_64", "OVMF"):
        if needed not in fragment:
            raise RuntimeError(
                f"the fragment extracted from {path} does not mention "
                f"{needed!r}; the block moved and this probe is checking the "
                f"wrong lines.")
    return fragment


def bash_can_run_boot_test(bash: str, script: str = BOOT_TEST,
                           root: str = PROJECT_ROOT) -> tuple[bool, str]:
    """Can *this* bash parse the boot test **and** find what it needs to run?

    Both halves, because on this machine they fail separately and the first
    passing hid the second for a whole sweep attempt.

    `bash -n` parses without executing a line, proving the exact argv the sweep
    will use resolves to a readable file this bash understands. Deliberately not
    `os.path.exists`: the file existed the whole time; the question is whether
    *bash* can open it under the exact string that will be passed, and only bash
    can answer that.

    Then the boot test's own dependency discovery is run under the same bash.
    That is what separates the two flavours here: both parse the script fine,
    and only one of them can see QEMU.
    """
    try:
        parsed = subprocess.run([bash, "-n", script], cwd=root,
                                capture_output=True, text=True, check=False)
    except (OSError, subprocess.SubprocessError) as exc:
        return False, f"cannot be run at all ({exc})"
    if parsed.returncode != 0:
        detail = (parsed.stdout + parsed.stderr).strip()[-300:]
        return False, (f"cannot parse {script} from {root} "
                       f"(exit {parsed.returncode}): {detail}")

    try:
        probe = subprocess.run([bash, "-c", extract_dependency_probe()],
                               cwd=root, capture_output=True, text=True,
                               check=False)
    except (OSError, subprocess.SubprocessError) as exc:
        return False, f"cannot run the dependency probe ({exc})"
    if probe.returncode != 0:
        detail = (probe.stdout + probe.stderr).strip()[-300:]
        return False, (f"parses {script} but cannot find what it needs to run "
                       f"it: {detail}")
    return True, "parses the boot test and can find QEMU and OVMF"


def find_bash(candidates=BASH_CANDIDATES, script: str = BOOT_TEST,
              root: str = PROJECT_ROOT) -> tuple[str | None, str]:
    """The first bash that can actually run the boot test, and why the rest can't.

    Returns `(path_or_None, message)`. The message is always printed: naming the
    bash that was chosen is the receipt, and on failure the per-candidate
    reasons are the entire diagnosis.

    # Why bare `bash` is the last resort and not the first

    Windows `CreateProcess` does not search `PATH` first. It searches the
    application directory, the current directory, **`C:\\Windows\\System32`**,
    the Windows directory, and only then `PATH`. On a machine with WSL
    installed there is a `System32\\bash.exe`, so `subprocess.run(["bash", ...])`
    launches **WSL's** bash no matter what `PATH` says -- and no matter what
    `shutil.which("bash")` reports, because `which` implements a `PATH` search
    and therefore answers a different question than the one that decides.
    Measured here: `shutil.which("bash")` returns Git's
    `C:\\Program Files\\Git\\usr\\bin\\bash.EXE`, while an actual
    `subprocess.run(["bash", "-c", "pwd"])` prints `/mnt/d/...` -- the WSL
    mount layout.

    That distinction is not cosmetic. WSL's bash is a different machine: it has
    its own filesystem view (`/mnt/d` rather than `/d`), its own `PATH`, and
    **no QEMU, no MSVC and no Windows Rust toolchain**. It parses the boot test
    perfectly and then cannot run it.

    This cost the second release sweep attempt. The first died on the absolute
    path -- WSL's bash cannot open `D:\\...\\boot-test.sh` and reported it as a
    missing file. The relative-name fix made that error go away, which read as
    the bug being solved; it was not. The sweep was still under WSL bash, and
    the very next arm died at `ERROR: qemu-system-x86_64 not found`. One root
    cause, two symptoms, and fixing the visible one moved the failure two lines
    down the script.

    So bash is resolved *explicitly*, most-specific first, and every candidate
    must demonstrate it can find QEMU before it is accepted. `bash` remains in
    the list because on a real Linux host it is the correct answer and the
    absolute Windows paths are not -- but it is last, so it only wins when
    nothing better exists.
    """
    reasons = []
    for candidate in candidates:
        if os.path.isabs(candidate) and not os.path.exists(candidate):
            continue
        ok, detail = bash_can_run_boot_test(candidate, script, root)
        if ok:
            note = ""
            if candidate == "bash":
                note = ("\n[layout-sweep]   (this is whatever the OS resolves "
                        "`bash` to, which on Windows is System32 before PATH)")
            return candidate, (f"[layout-sweep] preflight: using {candidate} "
                               f"-- it {detail}{note}")
        reasons.append(f"  {candidate}: {detail}")

    return None, (
        "layout-sweep: no usable bash. The sweep needs one that can both read "
        f"{script} and find QEMU/OVMF the way that script looks for them, and "
        "none of the candidates can:\n" + "\n".join(reasons) +
        "\n  Refusing to start rather than failing on the first arm after a "
        "full build.")


def check_arm_counts(bh, pad: int, profile: str,
                     previous=None) -> tuple[bool, str, object]:
    """Will the row this arm just wrote actually be *counted* as an arm, *and
    counted beside the ones before it*?

    Returns `(ok, message, group_key)`. The message is always printed: on
    success it is the receipt that the check ran, which is the only thing
    distinguishing "the arm was accepted" from "nobody asked". `group_key` is
    this arm's `bh.arm_group_key`, to be handed back as `previous` on the next
    call; `previous` is `None` for the first arm and otherwise the
    `(pad, group_key)` of the arm before it.

    # Two different questions, and why one guard cannot answer both

    `bh.layout_arm_rejection` is a predicate over a *single* record: it answers
    "will this row be kept?". A band, though, is not made of kept rows -- it is
    made of kept rows that land in the **same group**, and no per-record
    predicate can see that, because grouping is a relation between two records
    and the predicate is only ever shown one.

    That gap is not hypothetical; it is precisely the shape of the bug that
    voided the 2026-08-19 sweep. All six arms passed `layout_arm_rejection`
    individually -- every one was clean, committed, unloaded, correctly
    profiled, correctly tagged -- and the sweep still produced no band, because
    each landed in a group of one. Six green receipts, three hours, no result.
    `arm_group_key` fixes the specific cause (a docs commit is no longer a new
    identity), but it does not make the *class* of failure impossible: an arm
    whose source digest differs for any reason -- a real edit landing mid-sweep,
    a rebuilt service binary, a regenerated `rootfs.ext4`, an accelerator that
    silently fell back to TCG on one run -- still splits the sweep in exactly
    the same silent way. Comparing consecutive keys is the only check that sees
    it, and it sees all of those causes at once without enumerating any of them.

    Stopping is the right response rather than warning-and-continuing: once two
    arms disagree about their source, every arm after this one is being spent to
    fill a group that cannot reach `MIN_PADS_FOR_LAYOUT_BAND` either. Better to
    surrender the hour still ahead than to spend it.

    # Why this is not paranoia

    Everything up to here verifies that the *run* succeeded -- it built, it
    booted, it printed the pad it was asked for. None of that is the question.
    The question is whether `layout_arms()` will keep the record, and it has
    four other ways to say no (`dirty`, a loaded host, a missing commit, the
    wrong profile) that a perfectly successful run satisfies just as easily as
    a failed one.

    Two sweeps have been voided already. The second is the instructive one: it
    would have built and booted six kernels over ~3 hours, printed six
    confirmations, exited 0, and produced no band whatsoever, because a bug in
    the boot test made every run after the first record itself as `dirty`. The
    only signal would have been `--layout-bands` printing nothing, hours later,
    at which point the cause is a guess. Asking after arm one costs one
    `load_history()` and turns three silent hours into a twenty-minute failure
    with the reason attached.

    The predicate is `bh.layout_arm_rejection`, which is the function
    `layout_arms` itself uses -- not a copy. A copy would be a second statement
    of the same rule, free to agree with the first today and disagree after the
    next edit to either, which is exactly the class of bug this whole guard
    exists to catch.

    # Why this runs *after* an arm rather than before the sweep

    A pre-flight version would save the ~20 minutes this one spends before it
    can speak. It was considered and rejected: the rejection reasons are
    properties of a *record*, and the record does not exist until a run
    produces it. `dirty` in particular is computed by `boot-test.sh`, with its
    own pathspec exclusions; predicting it here would mean restating that
    command a third time, in a third language, and a preflight that disagrees
    with the real check is worse than no preflight -- it would clear a sweep
    that then records six discarded arms, with a green preflight standing as
    evidence that the tree was fine. Twenty minutes is the price of having one
    statement of each rule instead of two.
    """
    records = bh.load_history(bh.DEFAULT_HISTORY)
    if not records:
        return False, ("layout-sweep: the run finished but wrote no history "
                       f"row at all ({bh.DEFAULT_HISTORY} is empty or "
                       f"unreadable), so this arm cannot be part of a band."), None
    record = records[-1]
    # Confirm it is *our* row before judging it. If something else appended
    # after the boot test did, this would otherwise pass or fail an arm on
    # evidence from a different run entirely -- and report the verdict as
    # though it were about this one.
    if record.get("text_pad") != pad:
        return False, (
            f"layout-sweep: the newest history row reports text_pad="
            f"{record.get('text_pad')!r}, not the {pad} this arm just ran. "
            f"Something else appended to the history during the sweep, so the "
            f"arms cannot be trusted to be the runs this script performed. "
            f"Stopping."), None
    host = platform.node() or "unknown"
    reason = bh.layout_arm_rejection(record, host, profile)
    if reason is not None:
        return False, (
            f"layout-sweep: the pad={pad} arm ran successfully but "
            f"`layout_arms()` will DISCARD its record: {reason}.\n"
            f"  Every remaining arm would be discarded for the same reason, so "
            f"the sweep would spend hours and produce no band. Stopping now "
            f"instead."), None

    key = bh.arm_group_key(record)
    if previous is not None:
        previous_pad, previous_key = previous
        if key != previous_key:
            return False, (
                f"layout-sweep: the pad={pad} arm is a valid arm, but it does "
                f"not belong to the same group as the pad={previous_pad} arm "
                f"before it, so the two will never be banded together:\n"
                f"    pad={previous_pad}: {previous_key!r}\n"
                f"    pad={pad}: {key!r}\n"
                f"  The key is `(source digest, accelerator)`. A change in the "
                f"first means the build inputs moved under the sweep -- an "
                f"edit, a rebuilt service binary, a regenerated rootfs; a "
                f"change in the second means QEMU used a different "
                f"accelerator, which rescales every measurement.\n"
                f"  Each arm would land in a group of one and no group would "
                f"reach {bh.MIN_PADS_FOR_LAYOUT_BAND} pads, so the remaining "
                f"arms would be hours spent on a band that cannot form. "
                f"Stopping now instead. Re-run the sweep on a tree nothing "
                f"else is touching."), key
    return True, (f"[layout-sweep] confirmed: the pad={pad} record is accepted "
                  f"as an arm by bench-history.py, in group {key!r}"), key


def run_sweep(pads: list[int], profile: str, serial: str) -> int:
    """Build and `--bench`-boot the kernel once per pad value.

    Each run is tagged `BENCH_EXPERIMENT`, so `bench-history.py` records it but
    never uses it as a baseline for an ordinary run.

    After every run the serial log is re-read and the `textpad=` the *kernel*
    reported is compared against the pad that was *requested*. They can only
    disagree via a stale build, and a stale build is the one failure that would
    otherwise be completely invisible: the sweep would record two identical
    layouts under two different labels and report a band of zero.

    Each arm's record is then checked twice: that `layout_arms()` will keep it
    at all, and that it lands in the same group as the arm before it. The
    second check is the one that catches a tree changing under a running sweep,
    which every per-record check is structurally blind to.
    """
    bh = bench_history()
    minimum = bh.MIN_PADS_FOR_LAYOUT_BAND
    if len(pads) < minimum:
        print(f"layout-sweep: {len(pads)} pad(s) requested, but "
              f"bench-history.py needs {minimum} distinct layouts before it "
              f"will compute a band (two points define an interval containing "
              f"both by construction, so a two-arm 'spread' has no residual "
              f"and no way to be wrong).\n"
              f"Refusing up front rather than spending ~{len(pads)} QEMU boots "
              f"to produce a history no band can be computed from.")
        return 2

    script = BOOT_TEST
    bash, message = find_bash(script=script)
    print(message)
    if bash is None:
        return 1

    # The previous arm's `(pad, group key)`, so each arm can be checked against
    # the one before it rather than only against the rejection predicate, which
    # is blind to grouping. See `check_arm_counts`.
    previous = None
    for pad in pads:
        print(f"\n=== layout sweep: SLATEOS_TEXT_PAD={pad} "
              f"({profile}) ===", flush=True)
        env = dict(os.environ)
        env["SLATEOS_TEXT_PAD"] = str(pad)
        # The prefix is `bh.LAYOUT_SWEEP_TAG`, not a literal, because
        # `layout_arm_rejection` matches on it to tell a deliberate sweep arm
        # from an ordinary run that merely happens to be unpadded. Two copies
        # of that string would let the producer and the consumer drift, and the
        # symptom would be a sweep whose arms are all silently rejected.
        env["BENCH_EXPERIMENT"] = (
            f"{bh.LAYOUT_SWEEP_TAG}{pad} (identical source, deliberately "
            f"perturbed code placement; see scripts/layout-sweep.py)")
        proc = subprocess.run([bash, script, "--bench",
                               f"--profile={profile}"],
                              cwd=PROJECT_ROOT, env=env, check=False)
        if proc.returncode != 0:
            print(f"layout-sweep: boot-test failed for pad={pad} "
                  f"(exit {proc.returncode}); stopping.")
            return proc.returncode

        try:
            with open(serial, "r", encoding="utf-8", errors="replace") as fh:
                match = bh.TEXTPAD_RE.search(fh.read())
        except OSError as exc:
            print(f"layout-sweep: cannot re-read {serial}: {exc}")
            return 1
        if match is None:
            print(f"layout-sweep: the kernel that just ran printed no "
                  f"textpad= banner, so this run cannot be attributed to a "
                  f"layout. Stopping rather than recording an unlabelled "
                  f"sample.")
            return 1
        reported = int(match.group(1))
        if reported != pad:
            print(f"layout-sweep: requested pad={pad} but the kernel reports "
                  f"textpad={reported}. This is a stale build -- the sample "
                  f"would be a duplicate of another arm under a false label. "
                  f"Stopping.")
            return 1
        print(f"[layout-sweep] confirmed: the kernel that ran reports "
              f"textpad={reported}")

        accepted, verdict, key = check_arm_counts(bh, pad, profile, previous)
        print(verdict, flush=True)
        if not accepted:
            return 1
        previous = (pad, key)
    print(f"\n[layout-sweep] {len(pads)} arm(s) recorded. Analyse with:\n"
          f"    python scripts/bench-history.py --list")
    return 0


def parse_pads(text: str) -> list[int]:
    pads = []
    for token in text.split(","):
        token = token.strip()
        if not token:
            continue
        value = int(token)
        if value < 0:
            raise argparse.ArgumentTypeError("pad values must be >= 0")
        if value and value % 4096 == 0:
            print(f"layout-sweep: WARNING: pad {value} is a multiple of 4096, "
                  f"so it shifts every function by a whole number of guest "
                  f"pages and preserves every straddle. It will contribute a "
                  f"sample that measures nothing.")
        pads.append(value)
    if len(set(pads)) != len(pads):
        raise argparse.ArgumentTypeError("duplicate pad values")
    return pads


def main(argv=None) -> int:
    # Line-buffer stdout, because this script is never run interactively.
    #
    # Every invocation goes through `run-timeout.py` or a background job, so
    # stdout is a pipe, and Python block-buffers pipes: a six-arm release sweep
    # emits *nothing* for two and a half hours and then everything at once. That
    # is indistinguishable from a hang, which is precisely the condition
    # run-timeout.py exists to make visible -- its own docs promise output
    # "streamed live (no buffering that hides progress)", and this script was
    # quietly defeating that promise. Observed on the first real release run:
    # four minutes in, the only evidence it was alive came from inspecting the
    # process table.
    #
    # Line buffering rather than per-print `flush=True` because the latter has
    # to be remembered at all 27 call sites and at every one added later, and
    # forgetting it fails silently in the direction of looking hung.
    try:
        sys.stdout.reconfigure(line_buffering=True)
    except (AttributeError, ValueError):
        # Only on an exotic stdout; unbuffered output is a convenience here,
        # never a correctness requirement, so a failure must not stop the sweep.
        pass

    parser = argparse.ArgumentParser(
        description=__doc__.split("\n\n")[0],
        formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--self-test", action="store_true",
                        help="verify the padding mechanism (builds only, no "
                             "QEMU) and exit")
    parser.add_argument("--pads", default="0,1024,2048,3072",
                        help="comma-separated byte counts to sweep "
                             "(default: 0,1024,2048,3072). Avoid multiples of "
                             "4096.")
    parser.add_argument("--profile", default="release",
                        choices=("debug", "release"),
                        help="cargo profile (default: release -- the one the "
                             "benchmarks are actually measured on)")
    parser.add_argument("--serial", default=os.path.join(
        PROJECT_ROOT, "build", "serial-test.txt"),
        help="serial log boot-test.sh writes, re-read after each arm to "
             "confirm the kernel that ran is the one that was asked for")
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test(args.profile)
    return run_sweep(parse_pads(args.pads), args.profile, args.serial)


if __name__ == "__main__":
    sys.exit(main())
