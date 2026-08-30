#!/usr/bin/env python3
"""Rebuild kernel/ada/prebuilt/ from the Ada sources, and re-run the proof.

WHY THIS EXISTS

kernel/build.rs links a *committed* object rather than compiling Ada on every
build, because the cross GNAT is a ~1 GB install and the boot test builds the
whole workspace -- requiring it would block lanes B and C over a component
neither touches.  The obvious danger is a committed object that stops matching
the source it was proved from, so build.rs refuses to build when the recorded
stamp disagrees with the sources.  This script is the other half: the only
supported way to move that stamp forward.

It deliberately does more than compile.  Regenerating the object is exactly the
moment the proof stops applying, so `--prove` (on by default) re-runs gnatprove
and refuses to write anything if any check is unproved.  A prebuilt object that
is fresh but unproved is worse than a stale one, because the stamp then asserts
freshness the proof does not back.

USAGE

    python kernel/ada/regen-prebuilt.py            # compile, prove, write
    python kernel/ada/regen-prebuilt.py --check    # verify only, write nothing
    python kernel/ada/regen-prebuilt.py --no-prove # compile and write, skip proof

--check is what CI should run: it exits non-zero if the committed object or
stamp is not what the current sources produce, without touching the tree.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ADA = Path(__file__).resolve().parent
PREBUILT = ADA / "prebuilt"

# Must stay identical to ADA_FLAGS in kernel/build.rs and to the switches in
# virtqueue.gpr.  Three copies is two too many, but the alternatives are worse:
# build.rs cannot parse a .gpr, and gnatprove cannot read a Rust const.  The
# stamp is what makes a divergence loud -- change these and every build fails
# until the object is regenerated, which is when you would notice.
FLAGS = ["-mno-red-zone", "-mcmodel=kernel", "-O2", "-gnatwa", "-gnatw.X"]

UNITS = ["virtqueue_descriptors"]


NO_GCC = (
    "Could not find the x86_64-elf cross GNAT.\n"
    "Install it with:  alr toolchain --install gnat_x86_64_elf\n"
    "or point SLATEOS_ADA_GCC at the compiler.  See design-decisions.md §205."
)


def find_gcc(required: bool = True) -> Path | None:
    """Locate the x86_64-elf cross GNAT, mirroring build.rs's search order."""
    env = os.environ.get("SLATEOS_ADA_GCC")
    if env and Path(env).is_file():
        return Path(env)

    exe = "x86_64-elf-gcc.exe" if os.name == "nt" else "x86_64-elf-gcc"
    found = shutil.which(exe)
    if found:
        return Path(found)

    home = Path(os.environ.get("USERPROFILE") or Path.home())
    for cache in (
        home / "AppData/Local/alire/cache/toolchains",
        home / ".local/share/alire/toolchains",
    ):
        if cache.is_dir():
            for d in sorted(cache.iterdir()):
                if d.name.startswith("gnat_x86_64_elf"):
                    c = d / "bin" / exe
                    if c.is_file():
                        return c

    if required:
        sys.exit(NO_GCC)
    return None


def find_gnatprove() -> Path | None:
    found = shutil.which("gnatprove")
    if found:
        return Path(found)
    home = Path(os.environ.get("USERPROFILE") or Path.home())
    c = home / ".alire" / "bin" / ("gnatprove.exe" if os.name == "nt" else "gnatprove")
    return c if c.is_file() else None


def stamp() -> str:
    """SHA-256 over the flags and every source input, in build.rs's exact order.

    If this disagrees with `ada_stamp` in kernel/build.rs by so much as an
    ordering, every build fails with a stamp mismatch that no regeneration can
    clear -- so the two are written to be read side by side.
    """
    h = hashlib.sha256()
    h.update(" ".join(FLAGS).encode())
    h.update(b"\n")

    inputs = [ADA / "rts/adainclude/system.ads", ADA / "x86_64-elf.atp"]
    for unit in UNITS:
        inputs.append(ADA / "src" / f"{unit}.ads")
        inputs.append(ADA / "src" / f"{unit}.adb")

    for p in inputs:
        # Strip CR so a CRLF checkout on Windows and an LF checkout in CI hash
        # identically -- the compiler does not distinguish them, so the stamp
        # must not either.
        h.update(p.read_bytes().replace(b"\r", b""))
        h.update(b"\n")
    return h.hexdigest()


def compile_units(gcc: Path, outdir: Path) -> None:
    for unit in UNITS:
        cmd = [
            str(gcc), "-c", str(ADA / "src" / f"{unit}.adb"),
            # GNAT insists the object be named after the unit; it rejects any
            # other basename with "incorrect object file name".
            "-o", str(outdir / f"{unit}.o"),
            f"--RTS={ADA / 'rts'}",
            *FLAGS,
            f"-gnateT={ADA / 'x86_64-elf.atp'}",
        ]
        r = subprocess.run(cmd, capture_output=True, text=True)
        if r.returncode != 0 or r.stderr.strip():
            # -gnatwa is on, so any stderr at all is a warning we have chosen
            # not to tolerate in a component whose whole claim is rigour.
            sys.stderr.write(r.stdout + r.stderr)
            sys.exit(f"Ada compilation of {unit} failed or warned; refusing to write.")


def run_proof() -> None:
    gp = find_gnatprove()
    if gp is None:
        sys.exit(
            "gnatprove not found, and regenerating without proving is not the\n"
            "default: the object would be fresh but its proof would not be\n"
            "re-established.  Install with:  alr install gnatprove\n"
            "or pass --no-prove if you have deliberately proved it another way."
        )

    env = dict(os.environ)
    env["PATH"] = str(find_gcc().parent) + os.pathsep + env.get("PATH", "")

    r = subprocess.run(
        [str(gp), "-P", str(ADA / "virtqueue.gpr"), "--level=1", "--report=all", "-j0", "-f"],
        capture_output=True, text=True, env=env, cwd=str(ADA),
    )
    out = r.stdout + r.stderr
    summary = ADA / "build/gnatprove/gnatprove.out"
    text = summary.read_text() if summary.is_file() else out

    # Parse the "Total" row: the last column is the unproved count, printed as
    # "." when zero.  Checking the row rather than gnatprove's exit status is
    # deliberate -- gnatprove exits 0 with unproved checks, because "could not
    # prove" is a normal outcome for it and a release-blocking one for us.
    total = next((l for l in text.splitlines() if l.strip().startswith("Total")), None)
    if total is None:
        sys.stderr.write(out)
        sys.exit("Could not find the proof summary; refusing to write.")

    unproved = total.split()[-1]
    print(f"  proof: {total.strip()}")
    if unproved != ".":
        sys.stderr.write(text)
        sys.exit(
            f"gnatprove reports {unproved} unproved check(s).  Refusing to write a\n"
            "prebuilt object whose proof is incomplete -- see the module header for\n"
            "why this component exists at all."
        )


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true", help="verify only; write nothing, exit non-zero on mismatch")
    ap.add_argument("--no-prove", dest="prove", action="store_false", help="skip gnatprove")
    args = ap.parse_args()

    want = stamp()
    have = (PREBUILT / "stamp.txt").read_text().strip() if (PREBUILT / "stamp.txt").is_file() else None

    # The stamp half of --check needs no compiler, only the sources and the
    # recorded digest.  That is deliberate: it is the half that catches the
    # dangerous case (a source edited without regenerating), so it must be
    # runnable by lanes B and C and by any CI image, none of which have a 1 GB
    # Ada toolchain.  Only the byte-comparison half needs GNAT, and it degrades
    # to a skip rather than a failure.
    gcc = find_gcc(required=not args.check)
    if gcc:
        print(f"compiler: {gcc}")

    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        if gcc:
            compile_units(gcc, tmp)

        objs_match = gcc is not None and all(
            (PREBUILT / f"{u}.o").is_file()
            and (PREBUILT / f"{u}.o").read_bytes() == (tmp / f"{u}.o").read_bytes()
            for u in UNITS
        )

        if args.check:
            ok = True
            if have != want:
                print(f"STAMP MISMATCH\n  recorded: {have}\n  expected: {want}")
                print("  the Ada sources changed without regenerating the object.")
                ok = False
            missing = [u for u in UNITS if not (PREBUILT / f"{u}.o").is_file()]
            if missing:
                print(f"OBJECT MISSING: {', '.join(u + '.o' for u in missing)}")
                ok = False
            if gcc is None:
                print("  (no Ada toolchain: stamp checked, object bytes not verified)")
            elif not objs_match and not missing:
                print("OBJECT MISMATCH: the committed object is not what the compiler produces.")
                ok = False
            if ok:
                print("prebuilt is current.")
            return 0 if ok else 1

        if args.prove:
            run_proof()

        PREBUILT.mkdir(parents=True, exist_ok=True)
        for u in UNITS:
            shutil.copy2(tmp / f"{u}.o", PREBUILT / f"{u}.o")
        (PREBUILT / "stamp.txt").write_text(want + "\n")

        print(f"  wrote {', '.join(u + '.o' for u in UNITS)} and stamp.txt")
        print(f"  stamp {want}")
        if have == want and objs_match:
            print("  (unchanged)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
