r"""Which programs tell the user they DID something they cannot do?

The sixth of the set, and the one the other five are structurally unable to
find:

  find-reachable-fixtures.py    programs that invent the data
  find-silent-incapacity.py     programs that say too little
  find-stranded-serialisers.py  work that is finished and cannot be used
  find-stale-admissions.py      a denial that used to be true
  find-overstated-records.py    a record that was never true
  this one                      a program that reports an act it cannot perform

Three found on 2026-09-15, in one afternoon, in three apps that had each been
swept already:

    apps/renamer    "Renamed {count} files"            no std::fs in the crate
    apps/sysinfo    "Exported system info to file"     let _report = ...; dropped
    apps/sysinfo    "Value copied to clipboard"        no clipboard anywhere

**Why none of the five could see them.** The fixture scanner looks at who
calls a builder, and it did report `renamer`'s invented file list -- among
twenty-eight other reachable builders, most of which are genuinely fine, which
is exactly how a real one sits in that list indefinitely. The silence scanner
asks whether a program admits an incapacity, and `renamer` admits nothing
because it does not think it has one. The stale-admissions scanner looks for
denials, and this is the opposite: a *claim*. **A sentence in the past tense is
the one thing none of them read.**

That matters more than the count suggests. Every other fabrication in this
sweep costs the user time or trust and leaves the situation recoverable. A
completed-act claim is *acted upon*: a person told a batch rename succeeded
looks for their files under the new names, concludes they are lost, and may
delete the copy they kept. The rename having "worked" is what makes the backup
look redundant.

HOW IT DECIDES

Claims and capabilities are paired by kind, the way `find-stale-admissions`
pairs denials, so a crate that writes files and claims nothing about the
network is not reported for either:

    file       std::fs, safeio::, FileDialog, read_dir
               vs "Renamed 6 files", "Saved to disk", "Exported ... to file"
    clipboard  clipboard::, Clipboard, set_clipboard
               vs "Copied to clipboard"
    network    std::net, TcpStream, UdpSocket
               vs "Sent 4 packets", "Downloaded ...", "Connected to ..."
    process    std::process::Command
               vs "Launched ...", "Killed process ..."

A claim must be in the **past tense or otherwise complete** -- "Renamed",
"Saved", "Wrote", "Copied to clipboard". An imperative is a button label and
promises nothing: "Save to file" and "Copy to clipboard" are what the controls
are *for*, and reporting them would bury the real ones under every toolbar in
the tree.

WHAT IT CANNOT SEE, stated plainly:

  * **A capability held in one place does not make every claim true.** A crate
    that can write one file can still lie about having written another. This
    reports nothing for such a crate. `apps/benchmark` was exactly that shape
    before today -- it had `safeio` nowhere, but had it had it for some other
    purpose, its discarded export would have been invisible here.
  * It matches string literals, so a sentence assembled from pieces at runtime
    is not seen, and a constant nothing draws is reported anyway.
  * A claim can be true and still reported, if the act happens in another
    crate the program talks to. Read the sentence against the code.
  * The capability regexes look for a *mention*. `apps/netscan` names
    `std::net` to parse an address, which is not network access -- the same
    blind spot `find-stale-admissions` documents, and for the same reason.

THE FOUR IT STILL REPORTS, so nobody investigates them twice. All four are
correct code, read against the source on 2026-09-15:

  * `apps/terminal` [process] -- "terminated by {s:?}". A `Display` impl for an
    exit-status enum. It describes how a child process ended; it does not claim
    this program ended it. The vocabulary cannot tell a report of someone
    else's act from a claim about one's own.
  * `apps/tmux` [process] -- "Killed session: {name}". A tmux session here is
    one of the app's own panes in its own `Vec`, and it really is removed. The
    `process` kind assumes the object is external, and this one is not.
  * `apps/vpnmanager` [network] -- "Connected to {name}", twice. The *success*
    text handed to `report()` beside a `connect()` that can only fail on this
    system. Unreachable rather than false, and it is the sentence that becomes
    correct the day a tunnel exists.

Narrowing the vocabulary to silence any of these was considered and rejected,
for the reason `find-stale-admissions` gives about its own three: **narrowing
to silence a true-by-the-rules entry is how a check starts missing real ones.**
Rewording the source to satisfy a checker is the same mistake from the other
end, and is exactly what the report-only design exists to avoid.

Report-only, no --check mode, like the rest of the set.

Usage:  python scripts/find-claimed-acts.py [--roots=apps,gui]
"""

import importlib.util
import pathlib
import re
import sys

from rustlex import live_code, string_literals

ROOT = pathlib.Path(__file__).resolve().parent.parent


def _silence_module():
    """The sibling scanner, loaded once, for the pieces shared with it."""
    path = ROOT / "scripts" / "find-silent-incapacity.py"
    spec = importlib.util.spec_from_file_location("_silent_incapacity", path)
    if spec is None or spec.loader is None:
        print(f"cannot load {path}", file=sys.stderr)
        raise SystemExit(2)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


# Completed-act vocabulary, per kind, matched inside string literals only.
#
# Past tense throughout. The imperative forms -- "Save to file", "Copy to
# clipboard", "Export report" -- are button labels; they promise nothing and
# there is one on nearly every toolbar in this tree, so admitting them would
# bury the real findings.
KINDS = {
    "file": (
        re.compile(r"std::fs\b|safeio::|FileDialog|read_dir\b|list_directory"),
        re.compile(
            r"\b(renamed|saved|wrote|written|exported|imported|deleted|removed"
            r"|moved|created|restored|backed up|overwrote|recovered|applied)\b"
            r"[^\"]{0,40}?"
            r"\b(files?|disks?|drives?|folders?|director(?:y|ies)|paths?|documents?"
            r"|to disk|to file|on disk)\b"
            r"|\b(saved|wrote|written|exported|copied|moved) to [\\/~.]"
            r"|\b(wrote|saved|exported|written)\b[^\"]{0,20}\bbytes?\b",
            re.I,
        ),
    ),
    "clipboard": (
        re.compile(r"clipboard::|Clipboard\b|set_clipboard"),
        re.compile(r"\b(copied|cut|pasted)\b[^\"]{0,30}?\bclipboard\b", re.I),
    ),
    "network": (
        re.compile(r"std::net\b|TcpStream|UdpSocket|TcpListener"),
        re.compile(
            r"\b(sent|downloaded|uploaded|fetched|connected to|pinged|transferred)\b",
            re.I,
        ),
    ),
    "process": (
        re.compile(r"std::process::Command"),
        re.compile(r"\b(launched|spawned|executed|killed|terminated)\b", re.I),
    ),
}

# A claim has to read as a sentence to a person rather than as an identifier.
#
# The first version of this asked for letter-space-letter, and rejected
# "Renamed {count} files" and "Downloaded 4 updates" -- the two shapes a status
# line most often takes, because the interesting word is a placeholder or a
# number and the letters never end up adjacent to a space on both sides. **The
# heuristic excluded precisely the sentences this scanner exists to find**, and
# its own self-test said so before it was ever run against the tree.
#
# A space and not-an-identifier is the whole test now.
IDENTIFIER = re.compile(r"^[A-Za-z0-9_.:/-]+$")


def looks_like_prose(lit):
    return " " in lit.strip() and not IDENTIFIER.match(lit.strip())


# A past-tense verb alone is not a claim. Two things have to hold as well.
#
# **It must not be negated.** "not sent: bad address", "Not Downloaded",
# "nothing was ever fetched" -- the last is `apps/podcast` admitting, in a
# sentence written for the purpose, that it downloads nothing. Reporting a
# denial as a claim is the tool getting the answer exactly backwards, and this
# scanner's sibling `find-stale-admissions` exists to read those same sentences
# the other way round.
NEGATED = re.compile(
    r"\b(not|never|nothing|no|cannot|can't|would|could|if|unless|once|when)\b\W{0,3}$",
    re.I,
)

# **It must report a particular act, not name a category.** "Downloaded
# Updates", "Downloaded Episodes", "Old downloaded package archives" and
# "Bytes Sent" are labels on lists and columns -- noun phrases, not sentences
# about something that happened. What a real claim carries is the detail of
# *which* act: a placeholder, a count, or a target after a preposition.
PARTICULAR = re.compile(r"[{}]|\b\d|\b(to|from|at|into|onto)\b", re.I)


def is_claim(lit, claim):
    """Whether `lit` reports a completed act rather than labelling or denying one."""
    for m in claim.finditer(lit):
        if NEGATED.search(lit[: m.start()]):
            continue
        # The detail has to follow the verb: "Bytes Sent" has a digit nowhere
        # and "Total Sent:" has the verb at the end, which is what a column
        # heading looks like.
        if PARTICULAR.search(lit[m.start() : m.start() + 60]):
            return True
    return False


def production_source(crate):
    """Every non-test line of a crate's source, joined.

    Per file, via `rustlex.live_code`: concatenating the crate first and
    truncating at the first `#[cfg(test)]` discards every file that sorts after
    it, which is how the door model of this whole sweep once appeared on the
    list of programs that have no door.
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
        elif arg == "--self-test":
            return self_test()
        else:
            print(f"unknown argument: {arg}", file=sys.stderr)
            return 2

    findings, scanned = [], 0
    for root in roots:
        base = ROOT / root
        if not base.is_dir():
            print(f"no such directory: {root}", file=sys.stderr)
            return 2
        for crate in sorted(p for p in base.iterdir() if (p / "src").is_dir()):
            scanned += 1
            prod = production_source(crate)
            literals = string_literals(prod)
            for kind, (capability, claim) in KINDS.items():
                if capability.search(prod):
                    continue
                for lit in literals:
                    if not looks_like_prose(lit):
                        continue
                    if is_claim(lit, claim):
                        findings.append((crate.name, kind, lit.strip()))

    if not scanned:
        print("no crates found -- refusing to call that a pass", file=sys.stderr)
        return 2

    print(
        f"{len(findings)} claim(s) of an act the crate cannot perform, "
        f"across {scanned} crate(s) in {', '.join(roots)}\n"
    )
    if findings:
        print("  READ THE SENTENCE against the code. A claim can be true if the")
        print("  act happens in a crate this one talks to.\n")
    last = None
    for crate, kind, lit in findings:
        if crate != last:
            print(f"    {crate}")
            last = crate
        print(f"      [{kind}] {lit[:96]}")
    return 0


# The three that prompted this, plus the button labels that must stay silent.
SELF_TEST = [
    ("Renamed {count} files", "file", True),
    ("Exported system info to file", "file", True),
    ("Value copied to clipboard", "clipboard", True),
    ("Undid rename of {count} files", "file", False),  # no completed-act verb
    ("Save to file", "file", False),
    ("Copy to clipboard", "clipboard", False),
    ("Export report (Ctrl+E)", "file", False),
    ("Wrote 412 bytes to /tmp/x.txt", "file", True),
    ("renamed_files", "file", False),  # an identifier, not a sentence
    ("Downloaded 4 updates", "network", True),
    ("Download updates", "network", False),
    # Labels on lists and columns, not reports of an act.
    ("Downloaded Updates", "network", False),
    ("Downloaded Episodes", "network", False),
    ("Old downloaded package archives", "network", False),
    ("Bytes Sent", "network", False),
    ("Total Sent:", "network", False),
    # Denials. Reporting one as a claim is the answer exactly backwards.
    ("Not Downloaded", "network", False),
    ("not sent: no recipients", "network", False),
    ("An empty mailbox here does not mean no new mail -- nothing was ever fetched.",
     "network", False),
    # Real ones, from the same run.
    ("Killed process {name} (PID {pid})", "process", True),
    ("Connected to vpn.company.com", "network", True),
    ("sent \"{subject}\" to {recipients} recipient(s)", "network", True),
]


def self_test():
    bad = 0
    for lit, kind, should in SELF_TEST:
        _, claim = KINDS[kind]
        fired = looks_like_prose(lit) and is_claim(lit, claim)
        if fired != should:
            bad += 1
            print(f"  FAIL  {'fired' if fired else 'stayed silent'}: {lit!r}")
    print(f"{len(SELF_TEST) - bad}/{len(SELF_TEST)} self-test case(s) as expected")
    return 1 if bad else 0


if __name__ == "__main__":
    raise SystemExit(main())
