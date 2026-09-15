"""Which invented-data builders can a *shipping* build reach?

Every app in this tree has functions that manufacture content -- `sample_*`,
`mock_*`, `demo_*`, `simulate_*`. Most are fixtures and entirely fine. The
question that separates a fixture from a fabrication is not what it is called
or what it returns, it is **who calls it**: a builder only tests reach is a
fixture, and the same builder called from `App::new` is what the user sees.

So this reports call sites in non-test code, and stays quiet about the rest.

Found `netscan`'s `simulate_traceroute`, `simulate_whois` and `send_wol` on
2026-09-15, hours after that same file's fabricated host scan was removed and
the fix declared done -- the reasoning in that commit ("this crate has no
network access") covered the whole file and had been applied to one function.
That is what this is for: the instance is easy to see, the class is not.

WHAT IT CANNOT SEE, stated plainly because a scanner that is trusted past its
evidence is worse than none:

  * It matches names. A builder called `default_servers` or `initial_state`
    invents just as freely and is invisible here. The name list below is a
    convention this tree happens to follow, not a rule anything enforces.
  * It does not know whether the data is *wrong*. A `sample_` function that
    reads the real thing and is badly named will be reported; a fabrication
    with an honest-sounding name will not.
  * "Reachable" means lexically outside `#[cfg(test)]`, not proven live. A
    reported call in dead production code is a true positive for this tool and
    may still be nothing.

Report-only, and deliberately has no --check mode. The count is not a number
to hold at zero: several of these are legitimate (a `sample_rate` on an audio
buffer is a sampling rate, not a fixture), and a gate would train the next
reader to silence it rather than read it.

Usage:  python scripts/find-reachable-fixtures.py [--roots=apps,gui]
"""

import pathlib
import re
import sys

WORDS = (
    "simulate", "simulated", "fake", "dummy", "mock", "demo", "placeholder",
    "populate", "preload", "stub", "synthes", "sample", "seed_",
)

# A word that this tree uses for something other than invented data. Listed
# rather than dropped from WORDS so the reason survives.
EXEMPT = {
    "sample_rate": "an audio sampling rate, not a fixture",
    "bits_per_sample": "an audio format field",
    "push_samples": "appends real captured audio",
    "process_samples": "transforms real captured audio",
    "amplitude_from_samples": "computes over real captured audio",
    "sample_count": "how many measurements were taken",
    "samples": "accessor for recorded measurements",
    "record_sample": "the door a real producer comes through",
    "add_sample": "appends a real measurement",
    "active_graph_samples": "accessor for recorded measurements",
    "sample_ts": "formats a timestamp",
}

FN_DEF = re.compile(r"^(\s*)(?:pub(?:\([^)]*\))?\s+)?(?:const\s+|async\s+|unsafe\s+)*fn\s+([a-z_0-9]+)")


def gated_lines(lines):
    """Line indices that sit inside `#[cfg(test)]` code.

    Handles both a `#[cfg(test)] mod tests` block and an individually gated
    item at any indentation -- the second is what an earlier version of this
    script missed, which made it report `netmanager`'s fixture constructor as
    production and cost a false lead.
    """
    gated = set()
    i = 0
    while i < len(lines):
        if lines[i].strip() == "#[cfg(test)]":
            indent = len(lines[i]) - len(lines[i].lstrip())
            # Find the item this attribute belongs to, then take its block.
            j = i + 1
            while j < len(lines) and (
                lines[j].strip().startswith("#[") or lines[j].strip().startswith("//")
            ):
                j += 1
            if j >= len(lines):
                break
            # A braced item runs to the matching close at the same indent; a
            # one-liner (`type X = Y;`, `const N: u8 = 1;`) ends on its line.
            if "{" in lines[j]:
                depth = lines[j].count("{") - lines[j].count("}")
                k = j
                while depth > 0 and k + 1 < len(lines):
                    k += 1
                    depth += lines[k].count("{") - lines[k].count("}")
                for n in range(i, k + 1):
                    gated.add(n)
                i = k + 1
                continue
            end = j
            while end < len(lines) and not lines[end].rstrip().endswith(";"):
                end += 1
            for n in range(i, min(end + 1, len(lines))):
                gated.add(n)
            i = end + 1
            continue
        i += 1
    return gated


def main():
    roots = ["apps"]
    for arg in sys.argv[1:]:
        if arg.startswith("--roots="):
            roots = [r for r in arg.split("=", 1)[1].split(",") if r]
        else:
            print(f"unknown argument: {arg}", file=sys.stderr)
            return 2

    findings = []
    exempted = 0
    impossible = 0
    files = 0
    for root in roots:
        base = pathlib.Path(root)
        if not base.is_dir():
            print(f"no such directory: {root}", file=sys.stderr)
            return 2
        for path in sorted(base.glob("**/*.rs")):
            files += 1
            lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
            gated = gated_lines(lines)

            names = {}
            for n, line in enumerate(lines):
                m = FN_DEF.match(line)
                if m and any(w in m.group(2).lower() for w in WORDS):
                    names[m.group(2)] = n

            for name, def_line in sorted(names.items()):
                if name in EXEMPT:
                    exempted += 1
                    continue
                calls = [
                    n for n, line in enumerate(lines)
                    if n != def_line
                    and n not in gated
                    and re.search(rf"\b{re.escape(name)}\s*\(", line)
                    and not FN_DEF.match(line)
                ]
                if not calls:
                    continue
                if def_line in gated:
                    # A `#[cfg(test)]` definition cannot have a caller outside
                    # the test build -- that would not compile. So these call
                    # sites are test code this script failed to recognise as
                    # gated, and the finding is provably wrong. Counted rather
                    # than printed: the report stays trustworthy and the blind
                    # spot stays visible.
                    impossible += 1
                    continue
                findings.append((str(path).replace("\\", "/"), name,
                                 len(calls), calls[0] + 1))

    print(f"{len(findings)} builder(s) reachable from non-test code, "
          f"across {files} file(s) in {', '.join(roots)}")
    print(f"({exempted} skipped as known false positives; see EXEMPT. "
          f"{impossible} discarded by the compile-consistency check -- the "
          f"definition is #[cfg(test)], so a production caller could not "
          f"build, so this script mis-read where test code begins.)\n")

    by_file = {}
    for path, name, count, first in findings:
        by_file.setdefault(path, []).append((name, count, first))
    for path, items in sorted(by_file.items(), key=lambda kv: (-len(kv[1]), kv[0])):
        print(f"{path}")
        for name, count, first in items:
            print(f"    {name}: {count} call(s), first at line {first}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
