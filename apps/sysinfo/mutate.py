"""Mutation test for System Information's unreadable-versus-empty categories.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-27: each category keeps its read as a
`Result`, so a category that could not be read says why -- naming the path the
read used -- and one that was read and found nothing says "None found".  Both
used to draw an empty pane, and the one-value categories named
`/sys/hardware/...` paths nothing reads any more.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

UNREADABLE = "an_unreadable_category_is_not_an_empty_one"
SUMMARY = "the_summary_invents_no_version_build_or_manufacturer"

MUTATIONS = [
    (
        "an empty list draws an empty pane",
        "            Ok(list) if !list.is_empty() => Ok(list),",
        "            Ok(list) => Ok(list),",
        [UNREADABLE],
    ),
    (
        "an unreadable list draws an empty pane",
        "            Err(e) => Err(Self::unreadable(what, e)),",
        "            Err(_) => Ok(&[]),",
        [UNREADABLE],
    ),
    (
        "the reason names no path",
        '                format!("Not available — nothing on this system provides {path}")',
        '                String::from("Not available")',
        [UNREADABLE],
    ),
    (
        "the processor's reason names a path nothing reads",
        '            Err(e) => return Self::unreadable("Processor", e),',
        '            Err(_) => return vec![Property::new("Processor", "Not available — nothing on this system provides /sys/hardware/cpu")],',
        [UNREADABLE],
    ),
    (
        "the kernel version is invented again",
        "                    Ok(release) => release.as_str(),",
        '                    Ok(_) => "0.1.0-slateos",',
        [SUMMARY],
    ),
    (
        "the manufacturer is invented again",
        '            Property::new("System Manufacturer", Self::NOT_REPORTED),',
        '            Property::new("System Manufacturer", "SMBIOS: To Be Filled By O.E.M."),',
        [SUMMARY],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "sysinfo-app", timeout=600, only=only))
