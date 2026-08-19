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

    Three claims, none of which the sweep can check for itself once it is
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
#: `D:\visual studio projects\...\boot-test.sh`, and the `bash` on PATH here is
#: MSYS: it cannot open a drive-letter path with backslashes, and says so as the
#: generic `No such file or directory`, exit 127. That killed the first real
#: release sweep -- after its 34-minute self-test had passed, on the first arm,
#: with a message claiming a script that is plainly present was missing.
BOOT_TEST = "scripts/boot-test.sh"


def preflight_boot_test(script: str = BOOT_TEST,
                        root: str = PROJECT_ROOT) -> tuple[bool, str]:
    """Can `bash` find, read and parse the boot test, from where the sweep runs?

    Returns `(ok, message)`; the message is always worth printing, because the
    successful case is the receipt that this check ran at all.

    `bash -n` parses the script without executing a line of it, so this proves
    the *exact* argv the sweep loop is about to use resolves to a readable file
    this bash understands -- in about fifty milliseconds. Every part of that
    matters: the failure it exists to catch is invisible until the first arm,
    which is one full build and one full boot into a run that costs hours, and
    it reports itself as a missing file rather than an unusable path.

    Deliberately not `os.path.exists`: the file existed the whole time. The
    question is not whether Python can see it, it is whether *bash* can open it
    under the exact string the sweep will pass, and only bash can answer that.
    """
    try:
        parsed = subprocess.run(["bash", "-n", script], cwd=root,
                                capture_output=True, text=True, check=False)
    except (OSError, subprocess.SubprocessError) as exc:
        return False, (f"layout-sweep: cannot run bash at all ({exc}); the "
                       f"sweep needs it to invoke {script}.")
    if parsed.returncode != 0:
        detail = (parsed.stdout + parsed.stderr).strip()[-500:]
        return False, (
            f"layout-sweep: `bash -n {script}` failed (exit "
            f"{parsed.returncode}) with cwd {root}, so the sweep would fail on "
            f"its first arm -- after a full build and boot. Refusing to "
            f"start.\n  {detail}")
    return True, f"[layout-sweep] preflight: bash can run {script} from {root}"


def run_sweep(pads: list[int], profile: str, serial: str) -> int:
    """Build and `--bench`-boot the kernel once per pad value.

    Each run is tagged `BENCH_EXPERIMENT`, so `bench-history.py` records it but
    never uses it as a baseline for an ordinary run.

    After every run the serial log is re-read and the `textpad=` the *kernel*
    reported is compared against the pad that was *requested*. They can only
    disagree via a stale build, and a stale build is the one failure that would
    otherwise be completely invisible: the sweep would record two identical
    layouts under two different labels and report a band of zero.
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
    ok, message = preflight_boot_test(script)
    print(message)
    if not ok:
        return 1

    for pad in pads:
        print(f"\n=== layout sweep: SLATEOS_TEXT_PAD={pad} "
              f"({profile}) ===", flush=True)
        env = dict(os.environ)
        env["SLATEOS_TEXT_PAD"] = str(pad)
        env["BENCH_EXPERIMENT"] = (
            f"layout sweep: textpad={pad} (identical source, deliberately "
            f"perturbed code placement; see scripts/layout-sweep.py)")
        proc = subprocess.run(["bash", script, "--bench",
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
