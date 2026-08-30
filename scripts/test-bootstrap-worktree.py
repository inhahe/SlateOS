#!/usr/bin/env python3
"""Tests for `scripts/bootstrap-worktree.sh --check`.

Why this exists
---------------

`bootstrap-worktree.sh` is the answer to "this checkout cannot build", and
`boot-test.sh` now refuses to start until its `--check` mode says the tree is
provisioned.  That makes `--check` load-bearing in a specific and unforgiving
way: it is consulted precisely when nobody knows what is wrong, so a wrong
answer from it is not a wrong answer about one file -- it is a wrong answer
about what the problem *is*.

Two of its failure shapes are worth asserting rather than trusting:

1. **Reporting "all present" from a scan that found nothing to look for.**  The
   service list used to be a literal `SERVICES=(init hello ticker ...)` kept in
   sync by hand with the kernel's `include_bytes!` calls, and the script's own
   comment admitted it "cannot discover a service that was added to the kernel
   and not added here".  The list is now derived from those calls, so the test
   that matters is the one no hand-maintained list could pass: embed a service
   nobody has ever heard of and require it to be checked.

2. **Conflating "cannot build" with "will quietly test less".**  A missing
   service binary stops the build; a missing `rootfs.ext4` does not -- it lets
   the boot test pass while skipping ~58 Path-Z rungs.  Refusing on the second
   throws away a useful run; ignoring it lets a green result stand for more
   than it measured.  They exit 1 and 3 respectively, and both are asserted.

Every test builds a throwaway tree that looks like a worktree -- a `kernel/src`
with `include_bytes!` lines in it, `services/*/target/.../release/` artifacts, a
`limine/BOOTX64.EFI` and a `rootfs.ext4` -- and runs the real script against it.
Nothing is stubbed, so what passes here is the script that ships.

Run:  python scripts/test-bootstrap-worktree.py
Exit: 0 all passed, 1 one or more failed, 2 could not run (no bash).
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "bootstrap-worktree.sh")

# The exit codes documented in the script's header.  Named here so a failure
# reads as "expected BLOCKING, got DEGRADED" rather than "expected 1, got 3".
OK = 0
BLOCKING = 1
USAGE = 2
DEGRADING = 3

FAILURES: list[str] = []


def check(name: str, cond: bool, detail: str = "") -> None:
    if cond:
        print("  ok    %s" % name)
    else:
        print("  FAIL  %s %s" % (name, detail))
        FAILURES.append(name)


# ---------------------------------------------------------------------------
# Fixture
# ---------------------------------------------------------------------------


def make_tree(parent: str, name: str, services, *, limine=True, rootfs="good",
              embeds=None):
    """Build a fake worktree and return its root.

    `services` are the names to *embed* in the fake kernel.  `embeds`, when
    given, overrides the artifact path written into the `include_bytes!` line
    for a service -- that is how the two-paths-one-service case is built.
    Artifacts are created for every service in `services` unless the caller
    removes them afterwards, which keeps "what the kernel wants" and "what the
    tree has" independently controllable, exactly as they are in reality.
    """
    root = os.path.join(parent, name)
    os.makedirs(os.path.join(root, "scripts"), exist_ok=True)
    os.makedirs(os.path.join(root, "kernel", "src"), exist_ok=True)
    shutil.copy2(SCRIPT, os.path.join(root, "scripts", "bootstrap-worktree.sh"))

    lines = ["// fake kernel source for the bootstrap-worktree.sh tests\n"]
    for svc in services:
        paths = (embeds or {}).get(svc) or [
            "services/%s/target/x86_64-unknown-none/release/%s" % (svc, svc)
        ]
        for rel in paths:
            lines.append('    include_bytes!("../../%s");\n' % rel)
            artifact = os.path.join(root, rel.replace("/", os.sep))
            os.makedirs(os.path.dirname(artifact), exist_ok=True)
            with open(artifact, "wb") as fh:
                fh.write(b"\x7fELF fake\n")
    with open(os.path.join(root, "kernel", "src", "main.rs"), "w") as fh:
        fh.writelines(lines)

    if limine:
        os.makedirs(os.path.join(root, "limine"), exist_ok=True)
        with open(os.path.join(root, "limine", "BOOTX64.EFI"), "wb") as fh:
            fh.write(b"MZ fake\n")

    if rootfs in ("good", "bad"):
        # `looks_like_ext4` reads the two bytes at offset 1080 and wants 53 ef.
        blob = bytearray(b"\0" * 2048)
        if rootfs == "good":
            blob[1080:1082] = b"\x53\xef"
        with open(os.path.join(root, "rootfs.ext4"), "wb") as fh:
            fh.write(blob)

    return root


def run_check(root: str, *args):
    """Run `--check` in `root` and return (exit status, combined output)."""
    proc = subprocess.run(
        [BASH, os.path.join(root, "scripts", "bootstrap-worktree.sh"),
         "--check", *args],
        cwd=root, capture_output=True, text=True,
    )
    return proc.returncode, proc.stdout + proc.stderr


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


def test_fully_provisioned_tree_passes(tmp):
    root = make_tree(tmp, "complete", ["init", "hello", "netstack"])
    status, out = run_check(root)
    check("complete tree exits 0", status == OK, "got %d\n%s" % (status, out))
    for svc in ("init", "hello", "netstack"):
        check("complete tree reports %s present" % svc,
              ("present  %s" % svc) in out, out)


def test_service_the_kernel_added_is_discovered(tmp):
    """The regression test for the hand-maintained list.

    `unheard-of-svc` appears in no literal list anywhere and never did.  A
    script that knew its services by name would report this tree fully
    provisioned; deriving them from the kernel's own `include_bytes!` calls is
    what makes the missing binary visible.  Asserting on the *absent* case
    rather than the present one is deliberate -- the present case passes even
    if the derivation silently returns nothing.
    """
    root = make_tree(tmp, "new-service", ["init", "unheard-of-svc"])
    os.remove(os.path.join(root, "services", "unheard-of-svc", "target",
                           "x86_64-unknown-none", "release", "unheard-of-svc"))
    status, out = run_check(root)
    check("a newly-embedded service is checked at all",
          "unheard-of-svc" in out, out)
    check("a newly-embedded missing service blocks", status == BLOCKING,
          "got %d\n%s" % (status, out))
    check("it is reported MISSING, not present",
          "MISSING  unheard-of-svc" in out, out)


def test_missing_service_binary_blocks(tmp):
    root = make_tree(tmp, "no-binary", ["init", "hello"])
    os.remove(os.path.join(root, "services", "hello", "target",
                           "x86_64-unknown-none", "release", "hello"))
    status, out = run_check(root)
    check("missing service binary exits 1", status == BLOCKING,
          "got %d\n%s" % (status, out))
    check("missing service binary names the remedy",
          "bootstrap-worktree.sh" in out, out)
    check("missing service binary does not claim the tree is ready",
          "All build and boot prerequisites present" not in out, out)


def test_missing_limine_blocks(tmp):
    root = make_tree(tmp, "no-limine", ["init"], limine=False)
    status, out = run_check(root)
    check("missing limine exits 1", status == BLOCKING,
          "got %d\n%s" % (status, out))
    check("missing limine is named", "MISSING  limine" in out, out)


def test_missing_rootfs_degrades_but_does_not_block(tmp):
    """The distinction boot-test.sh depends on.

    Everything needed to build and boot is here; only the Path-Z image is not.
    That run is worth doing and must not be refused -- but it tests ~58 rungs
    fewer than it looks like it does, so it must not be silent either.
    """
    root = make_tree(tmp, "no-rootfs", ["init", "hello"], rootfs=None)
    status, out = run_check(root)
    check("missing rootfs.ext4 exits 3, not 1", status == DEGRADING,
          "got %d\n%s" % (status, out))
    check("missing rootfs.ext4 says the tests will skip",
          "SKIP" in out, out)
    check("missing rootfs.ext4 still confirms the build can proceed",
          "Everything needed to build and boot is present" in out, out)


def test_truncated_rootfs_is_not_treated_as_present(tmp):
    """A file of the right name with no ext4 superblock is not a rootfs.

    This is the shape a copy interrupted mid-write leaves behind, and taking
    its existence for provisioning would turn a 256 MiB partial file into a
    mysterious mount failure at boot instead of a named missing prerequisite.
    """
    root = make_tree(tmp, "bad-rootfs", ["init"], rootfs="bad")
    status, out = run_check(root)
    check("rootfs.ext4 without a superblock exits 3", status == DEGRADING,
          "got %d\n%s" % (status, out))


def test_no_embeds_at_all_is_an_error_not_a_pass(tmp):
    """An empty scan must never read as a clean bill of health.

    If the kernel stops embedding services, or the pattern stops matching how
    they are written, the honest answer is "I could not tell" -- which is a
    different thing from "nothing is missing" and must not exit 0.
    """
    root = make_tree(tmp, "no-embeds", [])
    status, out = run_check(root)
    check("a tree with no embedded services exits 2", status == USAGE,
          "got %d\n%s" % (status, out))
    check("it says why", "found no include_bytes!" in out, out)
    check("it does not claim everything is present",
          "All build and boot prerequisites present" not in out, out)


def test_one_service_two_artifacts_is_refused(tmp):
    """Two embedded paths for one service directory cannot both be built.

    Picking the first would provision one and leave the other missing -- a
    fresh worktree that fails to build while the checker says it is ready,
    which is the exact failure the derivation exists to prevent.
    """
    root = make_tree(tmp, "two-paths", ["init"], embeds={"init": [
        "services/init/target/x86_64-unknown-none/release/init",
        "services/init/target/x86_64-unknown-none/release/init-alt",
    ]})
    status, out = run_check(root)
    check("one service with two artifacts exits 2", status == USAGE,
          "got %d\n%s" % (status, out))
    check("both paths are printed", "init-alt" in out, out)


def test_unknown_service_argument_is_rejected(tmp):
    root = make_tree(tmp, "unknown-arg", ["init"])
    status, out = run_check(root, "not-a-service")
    check("an unknown service name exits 2", status == USAGE,
          "got %d\n%s" % (status, out))
    check("the known names are listed", "init" in out, out)


def test_need_scopes_the_question_to_what_a_run_uses(tmp):
    """`--no-build --no-stage` boots the staged image and uses neither class.

    Refusing that run because a service binary or limine is absent would be
    refusing a run that would have worked, and a gate that blocks correct runs
    is a gate that gets switched off.  So `--need` narrows what counts as
    missing -- and the narrowing has to be real, not cosmetic: the same tree
    must block without it.
    """
    root = make_tree(tmp, "need-scope", ["init", "hello"], limine=False)
    os.remove(os.path.join(root, "services", "hello", "target",
                           "x86_64-unknown-none", "release", "hello"))

    status, out = run_check(root)
    check("unscoped, the same tree blocks", status == BLOCKING,
          "got %d\n%s" % (status, out))

    status, out = run_check(root, "--need=rootfs")
    check("scoped to rootfs only, it passes", status == OK,
          "got %d\n%s" % (status, out))
    check("scoped to rootfs, services are not reported", "hello" not in out, out)
    check("scoped to rootfs, limine is not reported", "limine" not in out, out)

    status, out = run_check(root, "--need=limine,rootfs")
    check("scoped to limine+rootfs, the missing limine still blocks",
          status == BLOCKING, "got %d\n%s" % (status, out))
    check("scoped to limine+rootfs, services stay out of it",
          "hello" not in out, out)

    status, out = run_check(root, "--need=services")
    check("scoped to services, the missing binary blocks", status == BLOCKING,
          "got %d\n%s" % (status, out))
    check("scoped to services, limine stays out of it", "limine" not in out, out)


def test_need_rejects_an_unknown_class(tmp):
    """A typo must not silently widen or narrow the check.

    `--need=servcies` quietly checking nothing would be the worst outcome
    available: a green report from a scan that looked at nothing.
    """
    root = make_tree(tmp, "need-typo", ["init"])
    status, out = run_check(root, "--need=servcies")
    check("an unknown --need class exits 2", status == USAGE,
          "got %d\n%s" % (status, out))
    check("the known classes are listed", "services, limine, rootfs" in out, out)


def test_need_without_check_is_refused(tmp):
    """Provisioning has no scope, so accepting --need there would be a lie."""
    root = make_tree(tmp, "need-no-check", ["init"])
    proc = subprocess.run(
        [BASH, os.path.join(root, "scripts", "bootstrap-worktree.sh"),
         "--need=rootfs"],
        cwd=root, capture_output=True, text=True,
    )
    out = proc.stdout + proc.stderr
    check("--need without --check exits 2", proc.returncode == USAGE,
          "got %d\n%s" % (proc.returncode, out))
    check("--need without --check says why", "only applies to --check" in out,
          out)


def test_named_subset_checks_only_that_service(tmp):
    root = make_tree(tmp, "subset", ["init", "hello"])
    os.remove(os.path.join(root, "services", "hello", "target",
                           "x86_64-unknown-none", "release", "hello"))
    status, out = run_check(root, "init")
    check("a subset check ignores services it was not asked about",
          status == OK, "got %d\n%s" % (status, out))
    check("the unasked-for service is not reported", "hello" not in out, out)


def main() -> int:
    tmp = tempfile.mkdtemp(prefix="bootstrap-test-")
    print("bootstrap-worktree.sh tests (scratch: %s)" % tmp)
    tests = (
        test_fully_provisioned_tree_passes,
        test_service_the_kernel_added_is_discovered,
        test_missing_service_binary_blocks,
        test_missing_limine_blocks,
        test_missing_rootfs_degrades_but_does_not_block,
        test_truncated_rootfs_is_not_treated_as_present,
        test_no_embeds_at_all_is_an_error_not_a_pass,
        test_one_service_two_artifacts_is_refused,
        test_unknown_service_argument_is_rejected,
        test_named_subset_checks_only_that_service,
        test_need_scopes_the_question_to_what_a_run_uses,
        test_need_rejects_an_unknown_class,
        test_need_without_check_is_refused,
    )
    try:
        for test in tests:
            try:
                test(tmp)
            except Exception as exc:  # noqa: BLE001 - reporting, not handling
                print("  FAIL  %s raised %s: %s"
                      % (test.__name__, type(exc).__name__, exc))
                FAILURES.append(test.__name__)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    if FAILURES:
        print("FAILED: %d of the checks above" % len(FAILURES))
        return 1
    print("PASSED")
    return 0


BASH = shutil.which("bash")

if __name__ == "__main__":
    if BASH is None:
        # Exit 2, not 0.  "The suite did not run" must not be reported as
        # "the suite passed" -- the same distinction the script under test is
        # being asserted to make.
        print("SKIPPED: no bash interpreter on PATH; cannot exercise "
              "bootstrap-worktree.sh")
        sys.exit(2)
    sys.exit(main())
