r"""Which programs still deny a capability they have since acquired?

The fourth of the set, and the one the other three are structurally unable to
find:

  find-reachable-fixtures.py   programs that say too much
  find-silent-incapacity.py    programs that say too little
  find-stranded-serialisers.py work that is finished and cannot be used
  this one                     an admission that used to be true

`apps/dbviewer` is the case that prompted it. Its sidebar said, in a constant
written for the purpose and entirely honestly:

    No database open -- this program cannot open one
    It has no filesystem access and no database driver, so nothing was read

That was true the day it was written. It became false the moment Import could
reach a file, and nothing anywhere would have noticed: the fixture scanner sees
no invented data, the silence scanner sees a crate that *has* a capability and
skips it, and the serialiser scanner sees a door and skips it too. Every door
added to this tree manufactures a fresh opportunity for this, which is why it
is worth a check of its own rather than a habit of remembering.

**A banner that denies a capability the program has is the same defect as one
that claims a capability it lacks, pointed the other way.** Both leave the user
believing something about the program that is not so, and this direction is the
more expensive of the two, because the user who believes it stops looking for
the feature. A false promise is discovered by trying it; a false denial is not
discovered at all.

HOW IT DECIDES

Capabilities and denials are paired, rather than both being matched loosely and
any co-occurrence reported. A crate that reads files and truthfully says it
cannot reach the network is not a finding, and an unpaired check would call it
one. The pairs:

    file      FileDialog, safeio::, std::fs::read/write, list_directory
              vs "cannot open", "no filesystem access", "cannot save", ...
    network   std::net
              vs "cannot fetch", "no network access", "never sent", ...
    process   std::process::Command
              vs "cannot run", "no way to run", ...

WHAT IT CANNOT SEE, stated plainly:

  * **A crate can hold a capability in one place and honestly deny it in
    another**, and this cannot tell the two apart. `apps/pdfviewer` can open a
    file and still cannot render one; a sentence saying so is correct and will
    be reported here. Read the sentence against the code before changing it.
  * A denial can be stale in a way no vocabulary catches -- "the list below is
    empty because nothing fills it" names no capability at all.
  * It matches string literals, not what is drawn. A constant with no reader is
    reported; a sentence built with `format!` from pieces is not.
  * The capability regexes look for a *mention*, so a capability reached only
    on a branch nothing takes still counts as held. `apps/netscan` is the
    example to keep in mind: it names `std::net` to parse an address into an
    `IpAddr`, which is not network access, and its "no packet was sent" is
    perfectly true.
  * A standing denial that happens to carry a format placeholder is counted
    with the error messages and not printed. "Cannot open {path}: this program
    has no file access" would be missed.

Report-only, no --check, for the reason the others have none: several entries
will be legitimate, and a gate teaches the next reader to silence it rather
than read it.

Usage:  python scripts/find-stale-admissions.py [--roots=apps,gui]
"""

import pathlib
import re
import sys

from rustlex import live_code, string_literals

PAIRS = {
    "file": (
        re.compile(r"FileDialog|safeio::|std::fs::(read|write)|list_directory"),
        re.compile(
            r"cannot open|can't open|cannot save|can't save|cannot write|cannot read"
            r"|no (filesystem|file system|disk) access|nothing (here )?can (open|save|read|write)"
            r"|no way to (open|save|read|write)|has no file|no file (dialog|picker)",
            re.I,
        ),
    ),
    "network": (
        re.compile(r"\bstd::net\b"),
        re.compile(
            r"cannot fetch|can't fetch|cannot reach|cannot connect|no network access"
            r"|never (fetched|sent|contacted)|no way to (fetch|reach|connect)",
            re.I,
        ),
    ),
    "process": (
        re.compile(r"std::process::Command"),
        re.compile(r"cannot run|can't run|no way to run|cannot (launch|execute)", re.I),
    ),
}

def production_source(crate):
    """Every .rs file in the crate, with its test code blanked.

    `rustlex.live_code` rather than a cut at the first `#[cfg(test)]`. That
    attribute is ordinary on a single helper, and cutting there discards every
    item after it: lane A measured the same mistake at 35,706 lines, 7% of
    `userspace/`, invisible to a checker whose output looked fine.
    """
    return "".join(
        live_code(f.read_text(encoding="utf-8", errors="replace"))[0]
        for f in sorted((crate / "src").rglob("*.rs"))
    )


def main():
    roots = ["apps"]
    for arg in sys.argv[1:]:
        if arg.startswith("--roots="):
            roots = [r for r in arg.split("=", 1)[1].split(",") if r]
        else:
            print(f"unknown argument: {arg}", file=sys.stderr)
            return 2

    findings = []
    reported = []
    crates = 0
    for root in roots:
        base = pathlib.Path(root)
        if not base.is_dir():
            print(f"no such directory: {root}", file=sys.stderr)
            return 2
        for crate in sorted(p for p in base.iterdir() if (p / "src").is_dir()):
            crates += 1
            prod = production_source(crate)
            said = string_literals(prod)
            for kind, (capability, denial) in PAIRS.items():
                if not capability.search(prod):
                    continue
                hits = [s for s in said if denial.search(s)]
                # A denial carrying a format placeholder is a report about one
                # attempt that failed just now -- "cannot read {path}: {err}" --
                # not a standing claim about what the program can ever do. Both
                # match the same vocabulary and they are opposite things: the
                # first is the program telling you what happened, which is
                # exactly what it should do. Counted, not printed.
                standing = [s for s in hits if "{" not in s]
                reported.extend(s for s in hits if "{" in s)
                if standing:
                    findings.append((f"{root}/{crate.name}", kind, standing))

    total = sum(len(h) for _, _, h in findings)
    print(
        f"{total} standing sentence(s) denying a capability the crate has, "
        f"across {len(findings)} crate/kind pair(s) in {crates} crate(s)\n"
        f"({len(reported)} more carry a format placeholder, so they report one "
        f"failed attempt rather than claim a standing incapacity)\n"
    )
    print("  READ THE SENTENCE AGAINST THE CODE. A crate can hold a capability")
    print("  in one place and truthfully deny it in another -- pdfviewer can")
    print("  open a file and still cannot render one.\n")
    for name, kind, hits in sorted(findings, key=lambda kv: (-len(kv[2]), kv[0])):
        print(f"{name}  [{kind}]")
        for h in hits:
            shown = h if len(h) <= 96 else h[:93] + "..."
            print(f"    {shown}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
