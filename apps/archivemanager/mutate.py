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
        '        matches!(self, Self::Zip | Self::Tar | Self::TarGz | Self::TarBz2)',
        '        matches!(self, Self::Zip | Self::Tar | Self::TarGz)',
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
