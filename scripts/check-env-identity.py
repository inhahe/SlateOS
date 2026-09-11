#!/usr/bin/env python3
"""Refuse a new read of the caller's identity from the environment.

## Why this exists

Five times, a program took a value that decides an authorization outcome from
an environment variable the caller sets. Every one of them had a correct
implementation sitting beside it.

| Program | Variable | What it decided |
|---|---|---|
| `sudo` | `$UID` | the caller's uid |
| `sudo` | `$HOSTNAME` | which sudoers rules apply (the host half) |
| `sudo` | `$USER` | which rules apply, whose password, **which credential cache** |
| `login` | `$TTY` | whether root may log in at all (`/etc/securetty`) |
| `at` | `$USER` | the submitter a job is filed under, checked before it runs |

Two of those were privilege escalation rather than mis-attribution:

* `sudo`'s `$USER` keyed the credential cache -- `timestamp_path` is
  `TIMESTAMP_DIR/<username>` -- so `USER=alice sudo` found alice's live
  timestamp, **asked for no password**, and ran under alice's rules.
* `login`'s `$TTY` defaulted to `"console"` when unset, which is the name a
  securetty file is likeliest to list, so the control passed by default on a
  terminal nobody had looked at.

The defect is never in the checking code. `login`'s `check_securetty` was
correct; only its input was wrong. `sudo`'s `effective_uid` already read
`getuid(2)` and carried a note saying "The fallback was the caller's to set"
while the username two lines away still read `$USER`.

## Why this is a ratchet over the READ, and not taint analysis

The obvious rule is "a value from `env::var` must not reach an authorization
decision". It is also the wrong one to implement here: the value crosses
several functions, the checkers in this tree are regex-based, and a taint
analysis that is wrong in the permissive direction is worse than none because
it reads as coverage.

So this does not try to prove a read reaches a decision. It makes every read of
an identity variable say, once, why it is not one of the five above. The
population is small enough for that to be reviewable: most of what is pinned is
`$HOME` for a config path or `$SHELL` for which shell to launch, which is what
those variables are for.

## Two rules considered and rejected

* **Key on crates that depend on `authlib` or `userdb`.** Their manifests
  declare that a crate deals in identities. It would have MISSED `at`, which
  depended on neither until the day it was fixed.
* **List the security-relevant crates by name.** An enumeration that needs one
  entry per instance misses the next one by construction, and the miss is
  silent. That is the defect shape this tree keeps finding; it is not one to
  build a new gate on.

## Usage

    python scripts/check-env-identity.py              # list every site
    python scripts/check-env-identity.py --check      # 1 if an unpinned one appeared
    python scripts/check-env-identity.py --selftest   # this script's own fixtures
    python scripts/check-env-identity.py --update-baseline
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path
from typing import NamedTuple

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gittree  # noqa: E402
import rustlex  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
BASELINE_REL = "scripts/env-identity-baseline.txt"
BASELINE = Path(__file__).resolve().parent / "env-identity-baseline.txt"

# The trees lane B owns. `posix` is included: it implements `gethostname` and
# friends, so it is exactly where this defect would be most load-bearing.
SCANNED = ("userspace", "posix", "services", "init")

# Variables that name WHO or WHERE the caller is. Not a list of everything an
# attacker can set -- every variable is that -- but of the ones that have been
# mistaken for an identity.
#
# `HOME` and `SHELL` are deliberately absent. They are settings, not identity
# claims: `$HOME` is where to look for a config file and `$SHELL` is which
# shell to start, and a caller choosing those for themselves is the documented
# behaviour of every Unix. Adding them would put roughly forty legitimate sites
# in the baseline and teach the reader to skim it.
IDENTITY_VARS = (
    "USER",
    "LOGNAME",
    "USERNAME",
    "UID",
    "EUID",
    "GID",
    "EGID",
    "HOSTNAME",
    "TTY",
    "SUDO_USER",
    "SUDO_UID",
    "SUDO_GID",
)

# `env::var("USER")`, `std::env::var_os("TTY")`, `env::var(&"UID".to_string())`
# is not matched and does not need to be: the point is to make the ordinary
# spelling say why, not to defeat someone routing around the gate on purpose.
SITE = re.compile(
    r"(?:std::)?env::var(?:_os)?\s*\(\s*\"(" + "|".join(IDENTITY_VARS) + r")\"\s*\)"
)

# Any environment read at all, for the floor below.
ANY_ENV = re.compile(r"(?:std::)?env::var(?:_os)?\s*\(")

# Floors. A scan that stopped seeing must not be spelled as a clean tree: every
# pinned site would report as fixed, so total blindness arrives as the best
# possible news.
#
# MEASURED, NOT GUESSED, and the first values here were guessed: `MIN_ENV` went
# in at 150 against a real count of 138, so the checker refused to answer on a
# healthy tree the first time it was run. That is the floor working -- it is
# meant to refuse rather than reassure -- but a floor above the population is a
# gate that can only ever be red, which is a gate that gets deleted.
#
# 2026-09-11: 2,731 .rs files, 138 live `env::var` calls. The env floor sits
# about a quarter below that, because this population is meant to SHRINK as
# reads are replaced with real lookups, and a floor that fires on the fix is
# worse than no floor. Tighten both when they have room again.
MIN_FILES = 2000
MIN_ENV = 100


class Scan(NamedTuple):
    """What was found, and -- separately -- how much was looked at."""

    found: list[str]
    files: int
    env_reads: int


def scan_is_too_thin(scan: Scan) -> bool:
    """Did we actually look? Separate from whether we found anything."""
    return scan.files < MIN_FILES or scan.env_reads < MIN_ENV


def survey(tree) -> Scan:
    """Every identity-variable read in live code, one entry per site."""
    found: list[str] = []
    files = 0
    env_reads = 0
    for prefix in SCANNED:
        for rel in sorted(tree.files_under(prefix)):
            if not rel.endswith(".rs"):
                continue
            src = tree.read_text(rel)
            if src is None:
                continue
            files += 1
            # COMMENTS BLANKED, STRING LITERALS KEPT. Neither view `live_code`
            # returns is the one this needs, and getting that wrong is the
            # first thing the fixtures below caught:
            #
            #   its first view keeps comments, so the doc comment recording a
            #   FIXED site counts as a live one -- and every one of these five
            #   fixes quotes the line it replaced, so the cheapest way to lower
            #   the number would be to delete the explanation;
            #
            #   its second view blanks string CONTENTS, so `env::var("USER")`
            #   reads as `env::var("    ")` and the variable name -- the whole
            #   thing being matched -- is gone. The sibling checkers do not hit
            #   this because their patterns key on a function name, not on a
            #   literal.
            #
            # `strip_noise(.., keep_literals=True)` is the third view: comments
            # gone, literals intact. Both passes preserve length, so offsets
            # still index `original`.
            original, _fully_masked = rustlex.live_code(src)
            body = rustlex.strip_noise(original, keep_literals=True)
            env_reads += len(ANY_ENV.findall(body))
            crate = rel.split("/")[1] if rel.startswith("userspace/") else rel.split("/")[0]
            seen: dict[str, int] = {}
            for m in SITE.finditer(body):
                call = " ".join(original[m.start() : m.end()].split())
                key = f"{crate}: {call}"
                seen[key] = seen.get(key, 0) + 1
                if seen[key] > 1:
                    # `[2]`, not `#2`: `read_baseline` strips after a `#`, so a
                    # `#2` suffix is eaten on the way back in and every
                    # second-and-later identical call looks new forever. The
                    # sibling checker learned this the expensive way.
                    key = f"{key}  [{seen[key]}]"
                found.append(key)
    return Scan(sorted(found), files, env_reads)


def read_baseline(tree) -> set[str] | None:
    """The pinned set, out of the REVISION rather than the disk.

    `None` means the revision carries no baseline at all, which is not the same
    as an empty one: absent says this revision has no policy -- it predates the
    ratchet, or it is a fixture -- and judging a commit by a file it does not
    carry is judging it by a rule it never had.
    """
    text = tree.read_text(BASELINE_REL)
    if text is None:
        return None
    pinned = set()
    for line in text.splitlines():
        line = line.split("#")[0].strip()
        if line:
            pinned.add(line)
    return pinned


HEADER = """\
# Reads of an identity variable from the environment, one per site.
# Generated by `scripts/check-env-identity.py --update-baseline`.
#
# THIS FILE SHOULD ONLY EVER SHRINK. A line here is a program taking WHO or
# WHERE the caller is from a value the caller sets. Most are harmless -- a
# label in a log line, a path under /tmp -- and the point of pinning them is
# that the next one has to be looked at rather than written.
#
# Five were not harmless. See the module docstring of the checker: two of them
# were privilege escalation, and in both the checking code was correct and only
# its input was wrong.
#
# Do NOT add a line to turn a red --check green. The fix is to read the real
# thing: `authlib::identity::caller_uid` for the uid, `userdb` to resolve it to
# a name, `ttyname(0)` for the terminal, `/proc/sys/kernel/hostname` for the
# host. All four already exist in this tree and all four were sitting beside
# the code that did not use them.
"""


class _FakeTree:
    """The two methods `survey` uses, backed by a dict.

    A real `gittree` needs a git repository; these fixtures must run anywhere,
    including a checkout with no history, which is the point of running the
    self-test before the check.
    """

    def __init__(self, files: dict[str, str]) -> None:
        self._files = files

    def files_under(self, prefix: str):
        return [r for r in self._files if r.startswith(prefix + "/")]

    def read_text(self, rel: str):
        return self._files.get(rel)


def _self_test() -> int:
    """Fixtures for every way this could be wrong, including as a gate."""
    failures = 0

    def expect(label: str, got: object, want: object) -> None:
        nonlocal failures
        ok = got == want
        failures += not ok
        print(f"  {'ok  ' if ok else 'FAIL'}  {label}")
        if not ok:
            print(f"          got  {got!r}")
            print(f"          want {want!r}")

    tree = _FakeTree(
        {
            # the real shapes, from the five that occurred
            "userspace/alpha/src/main.rs": (
                'fn u() -> String { std::env::var("USER").unwrap_or_default() }\n'
                'fn h() -> String { env::var("HOSTNAME").unwrap_or_default() }\n'
            ),
            # var_os counts too: `login` read the tty with `var`, `sudo` with
            # `var_os`, and the defect is identical.
            "userspace/beta/src/main.rs": 'fn t() { env::var_os("TTY"); }\n',
            # A SETTING IS NOT AN IDENTITY. These must not be findings, or the
            # baseline fills with forty legitimate sites and stops being read.
            "userspace/gamma/src/main.rs": (
                'fn home() { env::var("HOME"); }\n'
                'fn shell() { env::var_os("SHELL"); }\n'
                'fn term() { env::var("TERM"); }\n'
            ),
            # THE EPITAPH CASE. Every one of these five fixes records the old
            # line in a doc comment. A checker that counted prose would make
            # the record of the fix into a finding, and the cheapest way to
            # lower the number would be to delete the explanation.
            "userspace/delta/src/main.rs": (
                '/// It used to be `env::var("USER")`, which the caller sets.\n'
                "fn u() { authlib::identity::caller_uid(); }\n"
            ),
            # ...and so is a test that proves the environment is ignored. Those
            # exist in sudo, login and at, and all three set the variable.
            "userspace/epsilon/src/main.rs": (
                "fn u() { real(); }\n"
                "#[cfg(test)]\n"
                'mod t { fn x() { std::env::set_var("USER", "nope"); '
                'env::var("USER"); } }\n'
            ),
            "userspace/alpha/README.md": 'env::var("USER")',
        }
    )
    scan = survey(tree)

    expect("finds exactly the three live identity reads", len(scan.found), 3)
    expect(
        "...in the right crates",
        sorted({f.split(":")[0] for f in scan.found}),
        ["alpha", "beta"],
    )
    expect(
        "a setting is not an identity: HOME, SHELL and TERM are not findings",
        any(f.startswith("gamma") for f in scan.found),
        False,
    )
    expect(
        "the doc comment recording a FIXED site is not a finding",
        any(f.startswith("delta") for f in scan.found),
        False,
    )
    expect(
        "nor is the test that proves the environment is ignored",
        any(f.startswith("epsilon") for f in scan.found),
        False,
    )
    expect("only .rs files are read", scan.files, 5)

    # Two identical calls in one crate stay distinct, numbered `[2]`.
    dup = _FakeTree(
        {
            "userspace/z/src/main.rs": (
                'fn a() { env::var("USER"); }\nfn b() { env::var("USER"); }\n'
            )
        }
    )
    dscan = survey(dup)
    expect("two identical reads in one crate stay distinct", len(dscan.found), 2)
    expect(
        "...and the second is numbered `[2]`, not `#2`",
        any(f.endswith("[2]") for f in dscan.found),
        True,
    )

    # The floors, both directions. A floor never observed to refuse is a claim
    # with no evidence behind it.
    expect(
        "a full scan is not too thin",
        scan_is_too_thin(Scan([], MIN_FILES, MIN_ENV)),
        False,
    )
    expect(
        "a scan that read almost no files is refused",
        scan_is_too_thin(Scan([], MIN_FILES - 1, MIN_ENV)),
        True,
    )
    expect(
        "a scan whose files survived but whose CONTENT was blanked is refused too",
        scan_is_too_thin(Scan([], MIN_FILES, MIN_ENV - 1)),
        True,
    )

    # An absent baseline is not an empty one.
    expect(
        "a revision carrying no baseline has no policy, not an empty one",
        read_baseline(_FakeTree({})),
        None,
    )
    expect(
        "a comment-only baseline is an empty policy, not an absent one",
        read_baseline(_FakeTree({BASELINE_REL: "# nothing yet\n"})),
        set(),
    )

    print(
        f"check-env-identity: self-test "
        f"{'FAILED' if failures else 'passed'} ({failures} failure(s))"
    )
    return 1 if failures else 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "--check", action="store_true", help="exit 1 if an unpinned site appeared"
    )
    ap.add_argument("--update-baseline", action="store_true", dest="update")
    ap.add_argument(
        "--self-test",
        "--selftest",
        dest="self_test",
        action="store_true",
        help="run this script's own fixtures",
    )
    ap.add_argument(
        "--head", metavar="REV", help="judge this revision rather than the working tree"
    )
    args = ap.parse_args()

    if args.self_test:
        return _self_test()

    try:
        tree = gittree.open_tree(str(ROOT), args.head)
    except gittree.GitTreeError as exc:
        # Exit 2, not 1: a revision that cannot be opened is not a finding
        # against anyone's code, and `run_checker` reads 1 as one.
        print(f"check-env-identity: cannot read {args.head!r}: {exc}", file=sys.stderr)
        return 2

    with tree:
        scan = survey(tree)
        pinned = read_baseline(tree)
    found = scan.found

    if scan_is_too_thin(scan):
        for line in [
            "check-env-identity: REFUSING TO ANSWER -- the scan is too thin to "
            "mean anything.",
            f"  .rs files read   {scan.files:>5}  (floor {MIN_FILES})",
            f"  env::var calls   {scan.env_reads:>5}  (floor {MIN_ENV})",
            "",
            "This is not a clean tree. It is a scan that stopped seeing, and "
            "every pinned",
            "site would report as fixed -- so a total failure to look would "
            "arrive spelled",
            "as the best possible news.",
        ]:
            print(line, file=sys.stderr)
        return 2

    if args.update:
        BASELINE.write_text(
            HEADER + "".join(f"{f}\n" for f in found),
            encoding="utf-8",
            # newline="" so Python does not translate to CRLF on Windows, which
            # would leave the file dirty against the repo's `eol=lf` attribute.
            newline="",
        )
        print(f"wrote {BASELINE.relative_to(ROOT)} with {len(found)} entries")
        return 0

    if not args.check:
        print(
            f"inspected {scan.files} .rs file(s), "
            f"{scan.env_reads} live env::var call(s)"
        )
        print(f"{len(found)} identity-variable read(s):\n")
        for f in found:
            print(f"  {f}")
        return 0

    if pinned is None:
        print("check-env-identity: this revision carries no baseline; nothing to check")
        return 0

    new = [f for f in found if f not in pinned]
    stale = sorted(pinned - set(found))

    if new:
        print(
            f"\n{len(new)} NEW read(s) of an identity variable from the "
            "environment:\n"
        )
        for f in new:
            print(f"  {f}")
        print(
            "\nWHO or WHERE the caller is must not come from a value the caller "
            "sets.\n"
            "Five times it has, and twice that was privilege escalation -- the "
            "checker's\n"
            "module docstring lists them. The real thing is already in this "
            "tree:\n"
            "  uid       authlib::identity::caller_uid\n"
            "  username  userdb, resolved from that uid\n"
            "  terminal  ttyname(0)\n"
            "  hostname  /proc/sys/kernel/hostname\n"
            "\n"
            "If this read genuinely decides nothing -- a label in a log line, a "
            "path\n"
            "under /tmp -- add it to the baseline with the reason in a comment "
            "beside it.\n"
        )
        return 1

    if stale:
        print(f"{len(stale)} baseline line(s) name sites that are gone:\n")
        for f in stale:
            print(f"  {f}")
        print("\nIf you fixed them, run --update-baseline in the SAME commit.")
        return 1

    print(
        f"ok -- {len(found)} pinned identity read(s), none new; "
        f"inspected {scan.files} file(s)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
