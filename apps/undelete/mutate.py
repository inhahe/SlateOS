"""Mutation test for the file recovery tool's signature table.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what changed on 2026-09-28: a file of the ISO base media family (an
`ftyp` box first) is named by its brand.  The bare `ftyp` recovered a phone's
HEIC photographs and every AVIF as `.mp4` videos.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

BRAND = "an_iso_media_file_is_named_by_its_brand"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a HEIC photograph is recovered as an MP4",
        '        FileSignature::new(FileSignatureKind::Heic, 4, b"ftyp").with_secondary(8, b"heic"),\n',
        "",
        [BRAND],
    ),
    (
        "an AVIF is recovered as an MP4",
        '        FileSignature::new(FileSignatureKind::Avif, 4, b"ftyp").with_secondary(8, b"avif"),\n',
        "",
        [BRAND],
    ),
    (
        "M4A audio is filed as a video",
        "            Self::Mp3 | Self::Flac | Self::Ogg | Self::Wav | Self::M4a => FileCategory::Audio,\n"
        "            Self::Mp4 | Self::Mov | Self::Avi | Self::Mkv => FileCategory::Video,",
        "            Self::Mp3 | Self::Flac | Self::Ogg | Self::Wav => FileCategory::Audio,\n"
        "            Self::Mp4 | Self::M4a | Self::Mov | Self::Avi | Self::Mkv => FileCategory::Video,",
        [BRAND],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "undelete", timeout=900, only=only))
