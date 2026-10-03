"""Mutation test for the archive manager's format recognition.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-26: one table of names per format
(`ArchiveFormat::patterns`), read by both the name detection and the file
dialogs -- whose Open filter said `*.zip` alone after TAR and TAR.GZ could be
opened -- and the 7z refusal, by name and by the bytes when the name says TAR.
Since then: the compressed TARs, TAR.BZ2 (2026-10-03, read and written) and
TAR.XZ (read; written once the `xz` crate has a compressor), and the cap
each is decompressed under.

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
TXZ_READ = "a_tar_xz_that_xz_wrote_is_listed_and_extracted"
BUDGET = "a_compressed_tar_past_the_budget_is_too_big_not_damaged"


def cap_arm(variant, error, verb):
    """The `Display` arm that names a decoder's cap as this program's."""
    return (
        f"            Self::{variant}({error}::Error::OutputTooLarge) => write!(\n"
        "                f,\n"
        f'                "it {verb} to more than {{}}, the most this program reads",\n'
        "                guitk::bytes::iec(MAX_ARCHIVE_BYTES)\n"
        "            ),\n"
    )

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
        "7z is taken for readable",
        "            Self::Zip | Self::Tar | Self::TarGz | Self::TarBz2 | Self::TarXz\n",
        "            Self::Zip | Self::Tar | Self::TarGz | Self::TarBz2 | Self::TarXz | Self::SevenZip\n",
        [REFUSED],
    ),
    (
        "TAR.XZ is not readable",
        "            Self::Zip | Self::Tar | Self::TarGz | Self::TarBz2 | Self::TarXz\n",
        "            Self::Zip | Self::Tar | Self::TarGz | Self::TarBz2\n",
        [TXZ_READ],
    ),
    (
        "TAR.XZ is taken for writable",
        "        matches!(self, Self::Zip | Self::Tar | Self::TarGz | Self::TarBz2)",
        "        matches!(self, Self::Zip | Self::Tar | Self::TarGz | Self::TarBz2 | Self::TarXz)",
        [DIALOGS],
    ),
    (
        "TAR.BZ2 is not writable",
        "        matches!(self, Self::Zip | Self::Tar | Self::TarGz | Self::TarBz2)",
        "        matches!(self, Self::Zip | Self::Tar | Self::TarGz)",
        [DIALOGS],
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
    (
        'a folder chosen in the pane is not shown',
        '                    if folder != self.current_dir {\n                        self.navigate_to(&folder);\n                    }\n',
        '',
        ['a_click_on_a_folder_shows_it', 'f6_gives_the_folder_pane_the_keys'],
    ),
    (
        'a folder shown from elsewhere is not chosen in the pane',
        '        self.list_scroll_y = 0.0;\n        self.show_dir_in_tree();\n    }\n',
        '        self.list_scroll_y = 0.0;\n    }\n',
        ['a_folder_shown_another_way_is_chosen_in_the_pane'],
    ),
    (
        'Back does not keep the pane in step',
        '        self.nav_position = prev;\n        self.current_dir = dir;\n        self.list_scroll_y = 0.0;\n        self.show_dir_in_tree();\n',
        '        self.nav_position = prev;\n        self.current_dir = dir;\n        self.list_scroll_y = 0.0;\n',
        ['a_folder_shown_another_way_is_chosen_in_the_pane'],
    ),
    (
        'Forward does not keep the pane in step',
        '        self.nav_position = next;\n        self.current_dir = dir;\n        self.list_scroll_y = 0.0;\n        self.show_dir_in_tree();\n',
        '        self.nav_position = next;\n        self.current_dir = dir;\n        self.list_scroll_y = 0.0;\n',
        ['a_folder_shown_another_way_is_chosen_in_the_pane'],
    ),
    (
        'F6 does not give the pane the keys',
        '        if key.key == Key::F6 && self.sidebar_visible && self.archive.is_some() {',
        '        if false {',
        ['f6_gives_the_folder_pane_the_keys'],
    ),
    (
        'the pane with the keys does not hear them',
        '        if self.tree_has_keys && self.sidebar_visible && Self::moves_in_tree(key) {',
        '        if false {',
        ['f6_gives_the_folder_pane_the_keys'],
    ),
    (
        'a click on the pane does not give it the keys',
        '            self.tree_has_keys = true;\n            let press = MouseEvent {',
        '            let press = MouseEvent {',
        ['a_click_on_a_folder_shows_it'],
    ),
    (
        'the wheel over the pane scrolls the list',
        '        if self.over_tree(mouse.x, mouse.y, size) {\n            let before = self.tree.first_visible();',
        '        if false {\n            let before = self.tree.first_visible();',
        ['the_wheel_over_the_folder_pane_scrolls_it'],
    ),
    (
        "a folder's folders are someone else's",
        '            node = node.children.iter().find(|child| &child.name == name)?;',
        '            node = node.children.first()?;',
        ['the_folders_are_read_by_name_under_the_archive'],
    ),
    (
        "the archive's own row is not at the top",
        '            return Some(vec![folder_item(self.0, String::new())]);',
        '            return Some(Vec::new());',
        ['the_folders_are_read_by_name_under_the_archive', 'a_folders_arrow_opens_it_without_showing_it'],
    ),
    (
        'a folder with folders in it has no arrow',
        '    let item = if node.children.is_empty() {',
        '    let item = if true {',
        ['a_folders_arrow_opens_it_without_showing_it'],
    ),
    (
        'the pane is not drawn in the window',
        '        state.tree.draw(&state.palette, frame, |_| Target::Tree);\n',
        '',
        ['a_folders_arrow_opens_it_without_showing_it', 'a_click_on_a_folder_shows_it'],
    ),
    (
        'TAR.BZ2 is not readable',
        "            Self::Zip | Self::Tar | Self::TarGz | Self::TarBz2 | Self::TarXz\n",
        "            Self::Zip | Self::Tar | Self::TarGz | Self::TarXz\n",
        ['a_bzipped_tar_is_decompressed_and_listed_whatever_it_is_called', 'a_tar_and_a_compressed_tar_are_rewritten_in_their_own_format'],
    ),
]

BACKEND = [
    (
        "a refused name is read anyway",
        "    if !format.readable() {\n        return Err(ArchiveError::NotYetReadable { format });\n    }",
        "",
        [REFUSED],
    ),
    # No row for `open`'s early refusal of a 7z under a TAR's name: since
    # TAR.XZ became readable, `decompress_tar` refuses 7z in the same words,
    # so removing the early check changes nothing a test can see -- it only
    # stops the file being read before it is refused (an equivalent mutant;
    # backend.rs says why the check stays).
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
    (
        'a bzipped tar is not decompressed',
        '            bzip2::decompress_limited(compressed, limit).map_err(ArchiveError::Bzip2)',
        '            Err(ArchiveError::NotYetReadable { format: ArchiveFormat::TarBz2 })',
        ['a_bzipped_tar_is_decompressed_and_listed_whatever_it_is_called'],
    ),
    (
        'a tar.bz2 is saved gzipped',
        '        ArchiveFormat::TarBz2 => Ok(bzip2::compress(tar, bzip2::Level::BEST)),',
        '        ArchiveFormat::TarBz2 => Ok(deflate::gzip(tar)),',
        ['a_tar_and_a_compressed_tar_are_rewritten_in_their_own_format'],
    ),
    (
        "a bzipped member's method says Stored",
        '        tararchive::Kind::File if format == ArchiveFormat::TarBz2 => String::from("Bzip2"),\n',
        '',
        ['a_bzipped_tar_is_decompressed_and_listed_whatever_it_is_called'],
    ),
    (
        "a TAR.BZ2's compressed size is its TAR's",
        '    if format.is_compressed_tar() {\n        model.total_compressed = on_disk;',
        '    if format == ArchiveFormat::TarGz {\n        model.total_compressed = on_disk;',
        ['a_bzipped_tar_is_decompressed_and_listed_whatever_it_is_called'],
    ),
    (
        'a new .tar.bz2 cannot be created',
        '        Some(format @ (ArchiveFormat::TarGz | ArchiveFormat::TarBz2)) => {\n            compress_tar(format, &empty_tar)?',
        '        Some(format @ (ArchiveFormat::TarGz | ArchiveFormat::TarBz2)) => {\n            return Err(SaveError::Unwritable { format });',
        ['a_new_archive_is_written_in_the_format_its_name_says'],
    ),
    (
        'a .tar.xz is not decompressed',
        '        ArchiveFormat::TarXz => xz::decompress_limited(compressed, limit).map_err(ArchiveError::Xz),',
        '        ArchiveFormat::TarXz => Err(ArchiveError::NotYetReadable { format: ArchiveFormat::TarXz }),',
        [TXZ_READ, REFUSED, BUDGET],
    ),
    (
        "an xz member's method says Stored",
        '        tararchive::Kind::File if format == ArchiveFormat::TarXz => String::from("XZ"),\n',
        '',
        [TXZ_READ],
    ),
    (
        "xz decompresses past the cap it is given",
        '        ArchiveFormat::TarXz => xz::decompress_limited(compressed, limit).map_err(ArchiveError::Xz),',
        '        ArchiveFormat::TarXz => xz::decompress(compressed).map_err(ArchiveError::Xz),',
        [BUDGET],
    ),
    (
        "bzip2 decompresses past the cap it is given",
        '            bzip2::decompress_limited(compressed, limit).map_err(ArchiveError::Bzip2)',
        '            bzip2::decompress(compressed).map_err(ArchiveError::Bzip2)',
        [BUDGET],
    ),
    (
        "gzip inflates past the cap it is given",
        '            deflate::gunzip_limited(compressed, limit).map_err(ArchiveError::Gzip)',
        '            deflate::gunzip_limited(compressed, usize::MAX).map_err(ArchiveError::Gzip)',
        [BUDGET],
    ),
    (
        "xz's cap is told as damage",
        cap_arm("Xz", "xz", "decompresses"),
        "",
        [BUDGET],
    ),
    (
        "bzip2's cap is told as damage",
        cap_arm("Bzip2", "bzip2", "decompresses"),
        "",
        [BUDGET],
    ),
    (
        "gzip's cap is told as damage",
        cap_arm("Gzip", "deflate", "inflates"),
        "",
        [BUDGET],
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
