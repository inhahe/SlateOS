"""Mutation test for the archive manager's format recognition.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-26: one table of names per format
(`ArchiveFormat::patterns`), read by both the name detection and the file
dialogs -- whose Open filter said `*.zip` alone after TAR and TAR.GZ could be
opened -- and the TAR.XZ, TAR.BZ2 and 7z refusals, by name and by the bytes
when the name says TAR.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

# main.rs
TGZ = "test_format_from_path_tgz"
TXZ = "test_format_from_path_tar_xz"
CASE = "test_format_from_path_case_insensitive"
OWN = "every_format_is_recognised_by_its_own_extension"
DIALOGS = "the_dialogs_list_what_the_program_recognises_and_writes"
# backend.rs
REFUSED = "opening_something_that_is_not_an_archive_says_which_thing_it_is_not"
GZIPPED = "a_gzipped_tar_is_inflated_and_listed_whatever_it_is_called"

MAIN = [
    (
        "a .tgz is not a TAR.GZ",
        '            Self::TarGz => &["*.tar.gz", "*.tgz"],',
        '            Self::TarGz => &["*.tar.gz"],',
        [TGZ],
    ),
    (
        "a .txz is not a TAR.XZ",
        '            Self::TarXz => &["*.tar.xz", "*.txz"],',
        '            Self::TarXz => &["*.tar.xz"],',
        [TXZ],
    ),
    (
        "names are matched case and all",
        "        let name = path.file_name()?.to_str()?.to_lowercase();",
        "        let name = path.file_name()?.to_str()?.to_owned();",
        [CASE, OWN],
    ),
    (
        "TAR.XZ is taken for readable",
        "        matches!(self, Self::Zip | Self::Tar | Self::TarGz)",
        "        matches!(self, Self::Zip | Self::Tar | Self::TarGz | Self::TarXz)",
        [DIALOGS, REFUSED],
    ),
    (
        "the filter keeps every format",
        "            .filter(|f| keep(*f))\n",
        "",
        [DIALOGS],
    ),
    (
        "the Open dialog shows only ZIP",
        '                .with_filter("Archives", &ArchiveFormat::patterns_where(|_| true))',
        '                .with_filter("Archives", &["*.zip"])',
        [DIALOGS],
    ),
    (
        "the New dialog offers what it cannot write",
        "                    &ArchiveFormat::patterns_where(ArchiveFormat::writable),",
        "                    &ArchiveFormat::patterns_where(|_| true),",
        [DIALOGS],
    ),
]

BACKEND = [
    (
        "a refused name is read anyway",
        "    if !format.readable() {\n        return Err(ArchiveError::NotYetReadable { format });\n    }",
        "",
        [REFUSED],
    ),
    (
        "a wrapper this build cannot undo is parsed as a TAR",
        "    if let Some(format) = wrapped.filter(|f| !f.readable()) {",
        "    if let Some(format) = wrapped.filter(|_| false) {",
        [REFUSED],
    ),
    (
        "gzip is not recognised by its bytes",
        "        [0x1F, 0x8B, ..] => Some(ArchiveFormat::TarGz),",
        "        [0x1F, 0x8B, ..] => None,",
        [GZIPPED],
    ),
    (
        "any file beginning BZh is bzip2",
        "        [b'B', b'Z', b'h', b'1'..=b'9', ..] => Some(ArchiveFormat::TarBz2),",
        "        [b'B', b'Z', b'h', ..] => Some(ArchiveFormat::TarBz2),",
        [REFUSED],
    ),
    (
        "xz is not recognised by its bytes",
        "        [0xFD, b'7', b'z', b'X', b'Z', 0x00, ..] => Some(ArchiveFormat::TarXz),",
        "        [0xFD, b'7', b'z', b'X', b'Z', 0x00, ..] => None,",
        [REFUSED],
    ),
]

TABLES = {
    "main.rs": MAIN,
    "backend.rs": BACKEND,
}

if __name__ == "__main__":
    only = sys.argv[1:]
    names = [name for rows in TABLES.values() for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    worst = 0
    for file, rows in TABLES.items():
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        print(f"\n######## {file} ########")
        worst = max(worst, sweep(SRC / file, rows, "archivemanager", timeout=900, only=mine))
    raise SystemExit(worst)
