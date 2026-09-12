#!/usr/bin/env python3
"""Find programs that take an unknown option as a FILE and then create it.

The bug this looks for was found physically, not by reading code: a file
named `--list.lock` was sitting in the repository root, holding the text
`exclusive:42160`. `flock` had not rejected `--list`; it had locked it,
because its parser accepted any argument as the file operand while none
had been seen yet, and `acquire_lock` appends `.lock` and creates the
file. An unrecognised option is not supposed to touch the filesystem.

That shape is invisible to a grep for "unrecognized option", because the
absence of a diagnostic is not the defect and is often correct -- `echo`,
`printf`, `test` and `expr` are all *required* by POSIX not to parse
options. The defect is the side effect. So this sweep is behavioural: run
each binary in an empty directory with nothing but a bogus long option,
and see whether a file appears.

    python scripts/unknown-option-sweep.py [--dir DIR] [--jobs N]
    python scripts/unknown-option-sweep.py --selftest

WHY THE SELFTEST EXISTS: a sweep that silently fails to launch anything
reports the same "0 found" as a clean tree. `--selftest` runs the same
detector over two synthetic commands whose behaviour is known -- one that
creates a file and one that does not -- and fails unless it flags exactly
the first. Proof it can find something is a precondition for believing it
when it finds nothing.
"""

import argparse
import glob
import io
import os
import importlib.util
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import selftestflag  # noqa: E402  (needs the path line above)

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

# The option we probe with. Deliberately not a prefix of any real option,
# so a parser doing prefix matching cannot resolve it to something valid.
PROBE = "--zzq-not-an-option"

PER_BINARY_TIMEOUT = 6

# Programs whose whole job is to change the machine, where "it ignored the
# unknown option and proceeded" is a cost we are not willing to pay to find
# out. They are reported as SKIPPED rather than silently dropped, because a
# denylist that shrinks the denominator without saying so is how a sweep
# comes to overstate its own coverage.
DANGEROUS = {
    "shutdown", "reboot", "halt", "poweroff", "kexec", "systemctl",
    "mkfs", "mkfs.ext4", "mke2fs", "fdisk", "sfdisk", "cfdisk", "parted",
    "wipefs", "blkdiscard", "dd", "shred", "fsck", "e2fsck", "resize2fs",
    "mkswap", "swapon", "swapoff", "mount", "umount", "losetup",
    "init", "telinit", "kill", "killall", "pkill", "reset",
    "chpasswd", "passwd", "useradd", "userdel", "usermod", "groupadd",
    "groupdel", "groupmod", "visudo", "vipw", "efibootmgr", "bootctl",
}


def _load_multicall():
    """The alias extractor from `scripts/multicall-aliases.py`.

    That tool already answers "what names does this program answer to", and
    answers it far better than a local copy would: it masks strings and
    comments before brace-matching, follows the invocation name through
    `let` rebindings, scopes the taint per function so one crate's `lower`
    does not leak into another's, and carries an audited IGNORE table for
    literals that look like tool names but are not. A second copy here
    would be a worse copy, and would drift.
    """
    here = os.path.dirname(os.path.abspath(__file__))
    spec = importlib.util.spec_from_file_location(
        "multicall_aliases", os.path.join(here, "multicall-aliases.py")
    )
    mod = importlib.util.module_from_spec(spec)
    if here not in sys.path:
        sys.path.insert(0, here)
    spec.loader.exec_module(mod)
    return mod


_MULTICALL = _load_multicall()


# POSIX requires these to treat every argument as an operand, so exiting 0
# on a leading dash is correct for them and is not a finding. `echo -q` must
# print `-q`, not complain about it.
NO_OPTION_PARSING = {
    "echo", "printf", "test", "[", "true", "false", "expr", "yes",
}


def created_entries(root):
    """Everything that appeared under `root`, relative and sorted."""
    out = []
    for base, dirs, files in os.walk(root):
        for name in list(dirs) + files:
            rel = os.path.relpath(os.path.join(base, name), root)
            out.append(rel.replace(os.sep, "/"))
    return sorted(out)


def probe(argv, label):
    """Run `argv` in an empty scratch directory; report what it created.

    Returns (label, exit_code, [created paths]) or (label, None, []) if the
    program could not be launched or had to be killed.
    """
    work = tempfile.mkdtemp(prefix="uosweep-")
    try:
        with open(os.devnull, "rb") as devnull:
            try:
                proc = subprocess.run(
                    argv,
                    cwd=work,
                    stdin=devnull,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    timeout=PER_BINARY_TIMEOUT,
                )
                code = proc.returncode
            except subprocess.TimeoutExpired:
                code = None
            except OSError:
                code = None
        return (label, code, created_entries(work))
    finally:
        shutil.rmtree(work, ignore_errors=True)


def selftest():
    """Prove the detector fires on a creator and stays quiet on a non-creator."""
    py = sys.executable
    creator = [py, "-c", "open('" + PROBE + ".lock', 'w').write('x')"]
    quiet = [py, "-c", "pass"]

    failures = 0

    _, _, made = probe(creator, "creator")
    if made == [PROBE + ".lock"]:
        print("  ok    a program that creates a file from the option is flagged")
    else:
        print("  FAIL  creator went undetected; saw %r" % (made,))
        failures += 1

    _, _, made = probe(quiet, "quiet")
    if made == []:
        print("  ok    a program that creates nothing is not flagged")
    else:
        print("  FAIL  non-creator reported %r" % (made,))
        failures += 1

    # A program that cannot be launched at all must not read as "clean".
    label, code, made = probe([py, "-c", "import sys; sys.exit(64)"], "refuser")
    if code == 64 and made == []:
        print("  ok    a refusing program is distinguishable from a silent one")
    else:
        print("  FAIL  refuser reported code=%r made=%r" % (code, made))
        failures += 1

    _, code, _ = probe(["this-binary-does-not-exist-zzq"], "missing")
    if code is None:
        print("  ok    a launch failure is reported as unknown, not as success")
    else:
        print("  FAIL  launch failure reported code=%r" % (code,))
        failures += 1

    print("selftest: %d failure(s)" % failures)
    return 1 if failures else 0


def stale_binaries(bindir, src_root):
    """Binaries older than the source they were built from.

    This sweep reads *binaries*, so a crate fixed but not rebuilt reports
    its old behaviour and the run says a defect is still there. That is the
    worst direction for a tool whose output is a list of accusations, and it
    nearly happened: 85 of 191 binaries here were older than their source,
    including three crates fixed the same day, because `cargo test` and
    `cargo clippy` had been run on them and `cargo build` had not.

    A count taken over stale inputs is not a smaller or larger count -- it
    is a different question's answer.
    """
    out = []
    for main_rs in glob.glob(os.path.join(src_root, "*", "src", "main.rs")):
        crate = os.path.basename(os.path.dirname(os.path.dirname(main_rs)))
        exe = os.path.join(bindir, crate + ".exe")
        if not os.path.exists(exe):
            continue
        try:
            if os.path.getmtime(main_rs) > os.path.getmtime(exe):
                out.append(crate)
        except OSError:
            continue
    return sorted(out)


def personality_map(src_root):
    """personality name -> the crate whose binary answers to it."""
    found = {}
    for main_rs in glob.glob(os.path.join(src_root, "*", "src", "main.rs")):
        crate = os.path.basename(os.path.dirname(os.path.dirname(main_rs)))
        try:
            text = io.open(main_rs, encoding="utf-8", errors="replace").read()
        except OSError:
            continue
        for name in _MULTICALL.invocation_aliases(text, crate):
            if name != crate:
                found.setdefault(name, crate)
    return found


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", default="target/x86_64-pc-windows-gnu/debug")
    ap.add_argument("--src", default="userspace")
    ap.add_argument(
        "--allow-stale",
        action="store_true",
        help="report even though some binaries predate their source",
    )
    ap.add_argument(*selftestflag.SPELLINGS, dest="selftest", action="store_true")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    exes = sorted(glob.glob(os.path.join(args.dir, "*.exe")))
    if not exes:
        print("no binaries under %s -- build first" % args.dir)
        return 2

    stale = stale_binaries(args.dir, args.src)
    if stale and not args.allow_stale:
        print("REFUSING to report: %d binary/binaries are older than their" % len(stale))
        print("source, so this run would describe code that is no longer there.")
        print("  %s" % ", ".join(stale[:12]))
        if len(stale) > 12:
            print("  ...and %d more" % (len(stale) - 12))
        print()
        print("Rebuild first:")
        print("  cargo build --workspace --target x86_64-pc-windows-gnu")
        print("or pass --allow-stale if you know what the difference is.")
        return 2

    # (label, path-to-run, argv0-name). A multi-call binary is probed once
    # per personality, from a copy named for it, because the dispatch reads
    # argv[0] and there is no `lockfile.exe` on disk to find otherwise.
    targets = []
    for exe in exes:
        name = os.path.basename(exe)[: -len(".exe")]
        targets.append((name, exe, name))

    pmap = personality_map(args.src)
    extra = 0
    bindir = tempfile.mkdtemp(prefix="uosbin-")
    try:
        for pname, crate in sorted(pmap.items()):
            host = os.path.join(args.dir, crate + ".exe")
            if not os.path.exists(host):
                continue
            copy = os.path.join(bindir, pname + ".exe")
            try:
                shutil.copyfile(host, copy)
            except OSError:
                continue
            targets.append((pname + " (via " + crate + ")", copy, pname))
            extra += 1

        creators, skipped, unlaunchable, accepted, expected = [], [], [], [], 0
        clean = 0
        for label, path, argv0 in targets:
            if argv0 in DANGEROUS:
                skipped.append(label)
                continue
            _, code, made = probe([path, PROBE], label)
            if code is None and not made:
                unlaunchable.append(label)
                continue
            if made:
                creators.append((label, code, made))
            elif code == 0:
                # Exited successfully on a command line it cannot have
                # understood. This is the broader signal: creating a file is
                # only the loudest way to get it wrong, and a program that
                # cleans up after itself gets it just as wrong silently.
                if argv0 in NO_OPTION_PARSING:
                    expected += 1
                else:
                    accepted.append(label)
            else:
                clean += 1
    finally:
        shutil.rmtree(bindir, ignore_errors=True)

    print("unknown-option sweep: probed with %s" % PROBE)
    print("  binaries on disk:   %d" % len(exes))
    print("  argv[0] aliases:    %d" % extra)
    print("  skipped (unsafe):   %d" % len(skipped))
    print("  did not launch:     %d" % len(unlaunchable))
    print("  refused (nonzero):  %d" % clean)
    print("  exit 0 by design:   %d" % expected)
    print("  ACCEPTED (exit 0):  %d" % len(accepted))
    print("  CREATED A FILE:     %d" % len(creators))
    for label, code, made in creators:
        print("    %-30s exit=%-5s %s" % (label, code, ", ".join(made[:4])))
    if accepted:
        print("  accepted the unknown option and exited 0:")
        for label in accepted:
            print("    %s" % label)
    print()
    print("NOT a clean bill of health when this reports 0: a program that")
    print("creates the file and removes it again before exiting is invisible")
    print("here -- verified, a create-then-remove probe reports nothing. The")
    print("stray --list.lock that prompted this sweep survived only because")
    print("that run never reached its cleanup.")
    return 1 if creators else 0


if __name__ == "__main__":
    sys.exit(main())
