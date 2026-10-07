"""Mutation test for the archive manager's format recognition.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-26: one table of names per format
(`ArchiveFormat::patterns`), read by both the name detection and the file
dialogs -- whose Open filter said `*.zip` alone after TAR and TAR.GZ could be
opened -- and the 7z refusal, by name and by the bytes when the name says TAR.
Since then: the compressed TARs, TAR.BZ2 and TAR.XZ (2026-10-03, read and
written, TAR.XZ as `xz -6` writes it), and the cap each is decompressed
under; and 7z, read through `sevenz` (2026-10-03) -- listed, extracted and
tested, never written -- which retired the rows about refusing it.

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
SEVEN = "a_7z_that_7zip_wrote_is_listed_extracted_and_tested"
SEVEN_DAMAGED = "a_damaged_7z_gives_up_what_7zip_would"
SEVEN_LOCKED = "an_encrypted_7z_is_listed_and_its_files_wait_for_a_password"
SEVEN_SAVE = "a_7z_is_not_rewritten"
SEVEN_NAMES = "a_7z_name_that_is_not_text_is_kept_as_bytes"
SEVEN_TIME = "a_7z_time_is_read_from_windows_ticks"
SEVEN_FOUND = "a_7z_whose_start_header_was_never_written_is_found_from_its_end"
SEVEN_AFTER = "a_7z_with_data_after_a_block_is_whole_in_its_files_only"
SEVEN_ANTI = "a_7z_deletion_record_is_not_extracted_as_a_file"
SEVEN_RO = "a_7z_opens_read_only"


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
        "7z is taken for writable",
        "        self != Self::SevenZip\n    }\n\n    /// Whether this is a TAR inside",
        "        true\n    }\n\n    /// Whether this is a TAR inside",
        [DIALOGS, SEVEN_RO],
    ),
    (
        "TAR.XZ is not writable",
        "        self != Self::SevenZip\n    }\n\n    /// Whether this is a TAR inside",
        "        self != Self::SevenZip && self != Self::TarXz\n    }\n\n    /// Whether this is a TAR inside",
        [DIALOGS],
    ),
    (
        "TAR.BZ2 is not writable",
        "        self != Self::SevenZip\n    }\n\n    /// Whether this is a TAR inside",
        "        self != Self::SevenZip && self != Self::TarBz2\n    }\n\n    /// Whether this is a TAR inside",
        [DIALOGS],
    ),
    (
        "Add and Delete are offered on a 7z",
        "        .is_some_and(|a| a.source.is_some() && a.format.writable());",
        "        .is_some_and(|a| a.source.is_some());",
        [SEVEN_RO],
    ),
    (
        "a dead Add on a 7z blames the source",
        "                    Some(f) if !f.writable() => {",
        "                    Some(f) if !f.writable() && false => {",
        [SEVEN_RO],
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
    # -- the shortcut card's hold on the pointer
    (
        'a press goes through the shortcut card',
        '                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) if self.show_help => {\n'
        '                    self.show_help = false;\n'
        '                    Action::Redraw\n'
        '                }\n',
        '',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    (
        "a double-click's second press goes through the card",
        '                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) if self.show_help => {\n',
        '                MouseEventKind::Press(_) if self.show_help => {\n',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    (
        'only the left button puts the card away',
        '                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) if self.show_help => {\n',
        '                MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) if self.show_help => {\n',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    (
        'the wheel scrolls what the card covers',
        '                MouseEventKind::Scroll { .. } if self.show_help => Action::None,\n',
        '',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
]

BACKEND = [
    (
        "a .7z is opened as a TAR",
        "    if format == ArchiveFormat::SevenZip {\n        let whole",
        "    if false {\n        let whole",
        [REFUSED],
    ),
    (
        "a 7z under a TAR's name is decompressed as a TAR's wrapper",
        "        Some(ArchiveFormat::SevenZip) => {",
        "        Some(ArchiveFormat::SevenZip) if false => {",
        [SEVEN, REFUSED],
    ),
    (
        "a block's packed size is on every file",
        "                    *shown = true;\n",
        "",
        [SEVEN],
    ),
    (
        "a block's packed size is on no file",
        "                    archive.folder_packed_size(f)\n",
        "                    0\n",
        [SEVEN],
    ),
    (
        "a 7z's method is not named",
        "                method: folder.map_or_else(String::new, |f| archive.folder_method(f)),",
        "                method: String::new(),",
        [SEVEN],
    ),
    (
        "a 7z's times are not read",
        "                modified: entry.mtime().map_or(0, unix_from_filetime),",
        "                modified: 0,",
        [SEVEN],
    ),
    (
        "a 7z's encryption is not shown",
        "                encrypted: folder.is_some_and(|f| archive.is_folder_encrypted(f)),",
        "                encrypted: false,",
        [SEVEN_LOCKED],
    ),
    (
        "a 7z found from its end is not noted",
        "        if archive.was_recovered() {",
        "        if false {",
        [SEVEN_FOUND],
    ),
    (
        "a 7z's directories are written as files",
        "        if member.is_dir() {",
        "        if member.is_dir() && false {",
        [SEVEN],
    ),
    (
        "a 7z deletion record is extracted",
        "        if member.is_anti() {",
        "        if false {",
        [SEVEN_ANTI],
    ),
    (
        "an empty 7z file is not written",
        "            None => write_out(&mut report, entry, &target, &[]),",
        "            None => {}",
        [SEVEN],
    ),
    (
        "a 7z's decoded files are not written",
        "                        Some(Ok(data)) => write_out(&mut report, entry, &target, &data),",
        "                        Some(Ok(_)) => {}",
        [SEVEN, SEVEN_DAMAGED],
    ),
    (
        "a 7z file refused by its block is not reported",
        "                        Some(Err(e)) => report\n                            .skipped\n                            .push((entry.path.clone(), SkipReason::SevenZ(e))),",
        "                        Some(Err(_)) => {}",
        [SEVEN_DAMAGED],
    ),
    (
        "a block that cannot be decoded skips its files silently",
        "            Err(e) => {\n                for (entry, ..) in wanted {\n                    report\n                        .skipped\n                        .push((entry.path.clone(), SkipReason::SevenZ(e)));\n                }\n            }",
        "            Err(_) => {}",
        [SEVEN_LOCKED],
    ),
    (
        "a 7z file needing a password is called corrupt",
        "        sevenz::Error::PasswordRequired => TestResult::DecryptionFailed,\n",
        "",
        [SEVEN_LOCKED],
    ),
    (
        "damage after a block's last file is not reported",
        "                if result.error_after_files.is_some() {",
        "                if false {",
        [SEVEN_DAMAGED],
    ),
    (
        "data after a block's end is not reported",
        "                } else if result.data_after_end {",
        "                } else if false {",
        [SEVEN_AFTER],
    ),
    (
        "a 7z is rewritten",
        "    if let Members::SevenZ(_) = &source.members {\n        return Err(SaveError::Unwritable {",
        "    if false {\n        return Err(SaveError::Unwritable {",
        [SEVEN_SAVE],
    ),
    (
        "a lone surrogate loses its middle bits",
        "                    0x80 | ((hi & 0x0F) << 2) | (lo >> 6),",
        "                    0x80 | (lo >> 6),",
        [SEVEN_NAMES],
    ),
    (
        "a time before 1970 wraps around",
        "    (ticks / 10_000_000).saturating_sub(EPOCH_GAP)",
        "    (ticks / 10_000_000).wrapping_sub(EPOCH_GAP)",
        [SEVEN_TIME],
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
        'a new compressed TAR cannot be created',
        '        Some(format @ (ArchiveFormat::TarGz | ArchiveFormat::TarBz2 | ArchiveFormat::TarXz)) => {\n            compress_tar(format, &empty_tar)?',
        '        Some(format @ (ArchiveFormat::TarGz | ArchiveFormat::TarBz2 | ArchiveFormat::TarXz)) => {\n            return Err(SaveError::Unwritable { format });',
        ['a_new_archive_is_written_in_the_format_its_name_says'],
    ),
    (
        'a tar.xz is saved gzipped',
        '        ArchiveFormat::TarXz => Ok(xz::compress(tar, xz::Preset::DEFAULT)),',
        '        ArchiveFormat::TarXz => Ok(deflate::gzip(tar)),',
        ['a_tar_and_a_compressed_tar_are_rewritten_in_their_own_format', TXZ_READ],
    ),
    (
        'a tar.xz is saved at another level',
        '        ArchiveFormat::TarXz => Ok(xz::compress(tar, xz::Preset::DEFAULT)),',
        '        ArchiveFormat::TarXz => Ok(xz::compress(tar, xz::Preset::new(9).unwrap_or(xz::Preset::DEFAULT))),',
        [TXZ_READ],
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
