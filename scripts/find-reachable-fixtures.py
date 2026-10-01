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

import os
import pathlib
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rustlex  # noqa: E402

WORDS = (
    "simulate", "simulated", "fake", "dummy", "mock", "demo", "placeholder",
    "populate", "preload", "stub", "synthes", "sample", "seed_",
)

# A word that this tree uses for something other than invented data. Listed
# rather than dropped from WORDS so the reason survives.
#
# READ THE FUNCTION BEFORE ADDING TO THIS LIST. On 2026-09-15 I was assembling
# these from the report by name -- sample_rate, bits_per_sample, sample_pixel,
# the lap `sample` in the stopwatch -- all obviously fine. `sample_pixel` was
# not: `apps/magnifier` computed screen colours as `x*7 + y*13 % 256` and
# magnified the result for a user who had opened a magnifier because they
# cannot check the screen by looking. It was the most consequential finding of
# the day and it looked exactly like the false positives beside it.
#
# Every entry below has been read. A name is evidence about a name.
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
    # Read 2026-09-28, each one.
    "sample_world_capitals": "flashcards' included deck: true statements bundled "
    "as content, and the window says three decks came with the app",
    "sample_programming": "as sample_world_capitals",
    "sample_science": "as sample_world_capitals",
    "decode_sample": "one audio sample's bytes, read (wavpcm)",
    "encode_sample": "one audio sample's bytes, written (wavpcm)",
    "placeholder": "an empty text field's hint -- HH, YYYY-MM-DD, Optional -- in "
    "alarmclock, calendar, compass, finance, qrcode, reminders and snippets",
    "placeholders": "the {name} slots in a clipboard template (clipmanager)",
    "populate_grid": "fills charmap's grid from the selected Unicode block",
    "render_placeholder": "draws diskanalyzer's empty-state message",
    "read_sample_description": "parses an MP4 stsd box (mediaprobe)",
    "build_placeholder_page": "settings' page for a section not built, which "
    "says it is under construction; the set is pinned by a test",
    "add_image_placeholder": "puts an empty image frame on a slide, as "
    "presentation programs do",
    "with_seed_and_difficulty": "a sudoku from a seed; the shipping one is "
    "seeded from the system, and a test asks for the same board twice",
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
            # Find the item this attribute belongs to, then take its block.
            j = skip_attributes(lines, i + 1)
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


def skip_attributes(lines, j):
    """The first line at or after `j` that is not an attribute, a comment or
    blank -- the item the attributes belong to.

    An attribute is skipped to the line that closes its brackets. This took a
    line starting with `#[` as the whole attribute, so the usual test module
    of this tree --

        #[cfg(test)]
        #[allow(
            clippy::unwrap_used,
            ...
        )]
        mod tests {

    -- stopped at `clippy::unwrap_used,`, read that as a one-line item, and
    gated only as far as the first `;` inside the module. Everything after
    `use super::*;` counted as production, and eighteen of the twenty-eight
    fixtures this reported on 2026-09-28 were test helpers called by tests.
    """
    while j < len(lines):
        stripped = lines[j].strip()
        if not stripped or stripped.startswith("//"):
            j += 1
            continue
        if not stripped.startswith("#["):
            return j
        depth = 0
        while j < len(lines):
            depth += lines[j].count("[") - lines[j].count("]")
            j += 1
            if depth <= 0:
                break
    return j


def _self_test():
    failures = 0

    def expect(label, got, want):
        nonlocal failures
        if got != want:
            failures += 1
            print(f"FAIL  {label}\n  got  {got!r}\n  want {want!r}")
        else:
            print(f"  ok    {label}")

    def gated_names(src):
        lines = src.splitlines()
        gated = gated_lines(lines)
        return [
            m.group(2)
            for n, line in enumerate(lines)
            if (m := FN_DEF.match(line)) and n in gated
        ]

    expect(
        "a test module under a multi-line attribute is gated whole",
        gated_names(
            "fn real() {}\n#[cfg(test)]\n#[allow(\n    clippy::unwrap_used,\n"
            "    clippy::panic\n)]\nmod tests {\n    use super::*;\n"
            "    fn sample() {}\n    #[test]\n    fn t() { sample(); }\n}\nfn after() {}\n"
        ),
        ["sample", "t"],
    )
    expect(
        "a single-line attribute, as before",
        gated_names("#[cfg(test)]\n#[allow(clippy::unwrap_used)]\nmod tests {\n    fn demo() {}\n}\n"),
        ["demo"],
    )
    expect(
        "a gated function by itself, as before",
        gated_names("#[cfg(test)]\nfn sample_app() -> u8 {\n    1\n}\nfn real() {}\n"),
        ["sample_app"],
    )
    expect(
        "a brace in a string does not end a test module early, once blanked",
        gated_names(
            rustlex.strip_noise(
                '#[cfg(test)]\nmod tests {\n    const J: &str = "}";\n'
                "    fn app_with_sample() {}\n}\n"
            )
        ),
        ["app_with_sample"],
    )
    expect(
        "a blank line and a comment between attribute and item",
        gated_names("#[cfg(test)]\n\n// fixtures\nmod tests {\n    fn seed_x() {}\n}\n"),
        ["seed_x"],
    )
    print(f"\n{failures} failure(s)")
    return 1 if failures else 0


def main():
    if any(a in ("--self-test", "--selftest", "--self_test") for a in sys.argv[1:]):
        return _self_test()
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
            # With strings, characters and comments blanked, line for line.
            # Read raw, a brace in a string literal unbalanced the count that
            # finds where a test module ends -- `apps/jsonviewer`'s tests are
            # full of JSON, and its module was taken to end 1,450 lines early
            # -- and a name in a comment was counted as a call (`sudoku`'s
            # "Was `with_seed_and_difficulty(42, ...)`").
            lines = rustlex.strip_noise(
                path.read_text(encoding="utf-8", errors="replace")
            ).splitlines()
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
