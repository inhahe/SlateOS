"""Mutation test for the Device Manager's real inventory.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-26, when the window stopped saying
it could not see the hardware and began listing what the kernel publishes:
how each source becomes rows (`inventory.rs`), and the scan, the banner, the
status bar, the export and the "Not reported" wording (`main.rs`).

Deliberately absent: the call to `scan_hardware` in `main()`, and the
`cfg(not(test))` half of `machine()`.  Neither runs under `cargo test` -- the
one is the program's entry point, the other is compiled out of the test build
-- so no test can notice them broken; they are one line each, read by eye.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

# inventory.rs
PCI_ROW = "a_pci_function_becomes_a_row_in_its_branch"
NAMELESS = "a_pci_function_with_no_name_is_named_by_its_ids"
CLASSES = "a_pci_class_decides_the_branch"
REGISTERED = "what_the_kernel_registered_is_listed_as_working"
CPU = "the_processor_is_one_row_named_by_its_brand"
LINK = "an_interface_row_says_its_link_and_address"
# main.rs
EMPTY = "an_empty_tree_says_nothing_was_read_and_why"
SCAN = "a_scan_lists_the_devices_the_kernel_publishes"
RESCAN = "a_rescan_replaces_the_list_and_the_selection"
STALE = "test_state_scan_hardware"
UNKNOWN = "the_status_bar_counts_the_devices_with_no_status_reported"
NOT_REPORTED = "what_the_kernel_does_not_say_reads_as_not_reported"
IRQ = "test_device_format_irq"
EXPORT = "export_asks_where_and_writes_the_report"
PICKER = "the_open_picker_is_drawn_and_takes_the_keys"

INVENTORY = [
    (
        "rows are not numbered from one",
        "        next_id = next_id.saturating_add(1);",
        "        next_id = next_id.saturating_add(2);",
        [PCI_ROW, RESCAN],
    ),
    (
        "an unreadable PCI tree is not named",
        '        Err(why) => unreadable.push(format!("PCI devices ({why})")),',
        "        Err(_) => {}",
        [EMPTY, SCAN],
    ),
    (
        "unreadable disks are not named",
        '        Err(why) => unreadable.push(format!("disks ({why})")),',
        "        Err(_) => {}",
        [EMPTY],
    ),
    (
        "unreadable interfaces are not named",
        '        Err(why) => unreadable.push(format!("network interfaces ({why})")),',
        "        Err(_) => {}",
        [EMPTY],
    ),
    (
        "unreadable outputs are not named",
        '        Err(why) => unreadable.push(format!("display outputs ({why})")),',
        "        Err(_) => {}",
        [EMPTY],
    ),
    (
        "an unreadable processor is not named",
        '        Err(why) => unreadable.push(format!("the processor ({why})")),',
        "        Err(_) => {}",
        [PCI_ROW, REGISTERED, EMPTY, SCAN],
    ),
    (
        "loopback is listed as a device",
        '            for n in interfaces.iter().filter(|n| n.name != "lo") {',
        "            for n in interfaces.iter() {",
        [REGISTERED, SCAN],
    ),
    (
        "a storage controller is filed under Other",
        "        (Some(0x01), _) => DeviceCategory::Storage,",
        "        (Some(0x01), _) => DeviceCategory::Other,",
        [PCI_ROW],
    ),
    (
        "wireless is not networking",
        "        (Some(0x02 | 0x0D), _) => DeviceCategory::Network,",
        "        (Some(0x02), _) => DeviceCategory::Network,",
        [CLASSES],
    ),
    (
        "video capture is filed as audio",
        "        (Some(0x04), Some(0x01 | 0x03)) => DeviceCategory::Audio,",
        "        (Some(0x04), _) => DeviceCategory::Audio,",
        [CLASSES],
    ),
    (
        "a USB controller is filed under System",
        "        (Some(0x0C), Some(0x03)) => DeviceCategory::Usb,",
        "        (Some(0x0C), Some(0x03)) => DeviceCategory::System,",
        [CLASSES],
    ),
    (
        "a PCI function's status is invented",
        "        pci_category(f.class_code, f.subclass_code),\n        DeviceStatus::Unknown,",
        "        pci_category(f.class_code, f.subclass_code),\n        DeviceStatus::Working,",
        [PCI_ROW],
    ),
    (
        "a nameless PCI function has no name",
        '        _ => format!("PCI device {:04x}:{:04x}", f.vendor_id, f.device_id),',
        "        _ => String::new(),",
        [NAMELESS],
    ),
    (
        "a function with no description is not named by its class",
        "    let what = if f.description.is_empty() {\n        f.class.clone()",
        "    let what = if f.description.is_empty() {\n        String::new()",
        [NAMELESS],
    ),
    (
        "an unknown vendor is left blank",
        '        format!("{:04x}", f.vendor_id)',
        "        String::new()",
        [PCI_ROW],
    ),
    (
        "a PCI location loses its function number",
        '    row.location = format!("PCI {:02x}:{:02x}.{}", f.bus, f.device, f.function);',
        '    row.location = format!("PCI {:02x}:{:02x}", f.bus, f.device);',
        [PCI_ROW],
    ),
    (
        "a registered disk is not called working",
        "    let mut row = blank(DeviceCategory::Storage, DeviceStatus::Working);",
        "    let mut row = blank(DeviceCategory::Storage, DeviceStatus::Unknown);",
        [REGISTERED],
    ),
    (
        "a disabled output is called working",
        "        } else {\n            DeviceStatus::Disabled\n        },",
        "        } else {\n            DeviceStatus::Working\n        },",
        [REGISTERED],
    ),
    (
        "a disabled output is called enabled",
        "    row.enabled = enabled;",
        "    row.enabled = true;",
        [REGISTERED],
    ),
    (
        "a blank brand is used as the processor's name",
        "        .filter(|b| !b.trim().is_empty())\n",
        "",
        [CPU],
    ),
    (
        "the processor's brand keeps its padding",
        '        .map_or_else(|| String::from("Processor"), |b| b.trim().to_owned());',
        '        .map_or_else(|| String::from("Processor"), |b| b);',
        [CPU],
    ),
    (
        "a link that is down is not a warning",
        "    let status = if n.up == Some(false) {",
        "    let status = if n.up == Some(true) {",
        [LINK],
    ),
    (
        "a link that is down is not said",
        '        Some(false) => detail.push_str("; link down"),',
        "        Some(false) => {}",
        [LINK],
    ),
    (
        "an interface with no address does not say so",
        '        Address::Unassigned => detail.push_str("; no address assigned"),',
        "        Address::Unassigned => {}",
        [LINK],
    ),
    (
        "an interface's address is not said",
        '        Address::Is(ip) => detail.push_str(&format!("; address {ip}")),',
        "        Address::Is(_) => {}",
        [LINK],
    ),
]

MAIN = [
    (
        "a scan forgets what it could not read",
        "        self.unreadable = found.unreadable;",
        "        self.unreadable = found.unreadable.into_iter().take(0).collect();",
        [EMPTY, SCAN],
    ),
    (
        "a scan keeps the old tree",
        "        self.resource_view = ResourceView::from_devices(&self.devices);\n"
        "        self.tree_nodes = build_tree_nodes(&self.devices);\n"
        "        self.update_checks = self",
        "        self.resource_view = ResourceView::from_devices(&self.devices);\n"
        "        self.update_checks = self",
        [SCAN, STALE],
    ),
    (
        "a scan keeps the old update checks",
        "        self.update_checks = self\n"
        "            .devices\n"
        "            .iter()\n"
        "            .map(|d| (d.id, DriverUpdateCheck::new(d.id)))\n"
        "            .collect();\n",
        "",
        [STALE],
    ),
    (
        "a scan keeps a selection that now names another device",
        "        self.selected_tree_index = None;\n        self.apply_search_filter();\n        let found",
        "        self.apply_search_filter();\n        let found",
        [RESCAN],
    ),
    (
        "the scan's notice leaves out what could not be read",
        "            (n, false) => format!(\n"
        '                "Found {n} device(s); could not read {}",\n'
        '                self.unreadable.join("; ")\n'
        "            ),",
        '            (n, false) => format!("Found {n} device(s)"),',
        [SCAN],
    ),
    (
        "the status bar leaves out the notice",
        '        (Some(notice), false) => format!("{counts} | {notice}"),',
        "        (Some(_), false) => counts,",
        [SCAN, EXPORT],
    ),
    (
        "the status bar counts working devices as unreported",
        "        .filter(|d| d.status == DeviceStatus::Unknown)",
        "        .filter(|d| d.status == DeviceStatus::Working)",
        [UNKNOWN],
    ),
    (
        "the banner stays over a full tree",
        "    if !state.devices.is_empty() {\n        return;\n    }",
        "",
        [EMPTY, SCAN],
    ),
    (
        "the banner does not say what could not be read",
        '        format!("Could not read {}.", state.unreadable.join("; "))',
        '        String::from("Press F5 to look at the machine.")',
        [EMPTY],
    ),
    (
        "Export asks for no destination",
        '            ToolbarAction::Export => self.picker.open_to_write("hardware-report.txt"),',
        "            ToolbarAction::Export => {}",
        [EXPORT, PICKER],
    ),
    (
        "a chosen destination is not written",
        "        Picked::Chose(path) => {\n            state.notice = Some(state.write_report(&path));",
        "        Picked::Chose(_path) => {\n            state.notice = None;",
        [EXPORT],
    ),
    (
        "the report written is not the report composed",
        "        match safeio::write_str_atomically(path, &report) {",
        '        match safeio::write_str_atomically(path, "") {',
        [EXPORT],
    ),
    (
        "keys go through the picker to the window behind",
        "        Picked::Handled | Picked::Cancelled => return EventResult::Consumed,",
        "        Picked::Handled | Picked::Cancelled => {}",
        [PICKER],
    ),
    (
        "the open picker is not drawn",
        "    cmds.extend(\n"
        "        state\n"
        "            .picker\n"
        "            .render(&state.palette, state.width, state.height),\n"
        "    );\n",
        "",
        [PICKER],
    ),
    (
        "the driver tab claims no driver is installed",
        "                text: DRIVER_NOT_REPORTED.to_string(),",
        '                text: "No driver installed".to_string(),',
        [NOT_REPORTED],
    ),
    (
        "an unreported interrupt line reads as none",
        '            Some(irq) => format!("IRQ {irq}"),\n            None => NOT_REPORTED.to_string(),',
        '            Some(irq) => format!("IRQ {irq}"),\n            None => "N/A".to_string(),',
        [IRQ, NOT_REPORTED],
    ),
    (
        'a chord raises the keys',
        '    if key.key == Key::F1 && plain {',
        '    if key.key == Key::F1 {',
        ['a_chord_is_neither_a_key_of_the_window_nor_typing'],
    ),
    # No row for "a command's letter is typed into the search": since
    # 2026-10-04 the search box's typing is textline::apply_key's, which tells
    # a command from AltGr itself, in its own crate and with its own tests;
    # a_chord_is_neither_a_key_of_the_window_nor_typing still holds the box
    # to it.
    (
        'a chord works the search box',
        '        if plain && matches!(key.key, Key::Escape | Key::Enter) {\n',
        '        if matches!(key.key, Key::Escape | Key::Enter) {\n',
        ['a_chord_is_neither_a_key_of_the_window_nor_typing'],
    ),
    (
        'AltGr is taken for Ctrl',
        '    if textline::is_ctrl_chord(key.modifiers) {',
        '    if key.modifiers.ctrl {',
        ['a_chord_is_neither_a_key_of_the_window_nor_typing'],
    ),
    (
        'a chord works the tree and the tabs',
        '    if !plain {\n        return EventResult::Ignored;\n    }\n',
        '',
        ['a_chord_is_neither_a_key_of_the_window_nor_typing'],
    ),
    # -- the shortcut card's hold on the pointer
    (
        'a press goes through the shortcut card',
        '            MouseEventKind::Press(_) => {\n'
        '                state.show_help = false;\n'
        '                return EventResult::Consumed;\n'
        '            }\n',
        '',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    (
        'only the left button puts the card away',
        '            MouseEventKind::Press(_) => {\n'
        '                state.show_help = false;\n',
        '            MouseEventKind::Press(MouseButton::Left) => {\n'
        '                state.show_help = false;\n',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    (
        'the wheel scrolls what the card covers',
        '            MouseEventKind::Scroll { .. } => return EventResult::Ignored,\n',
        '',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
]

SEARCH = 'the_search_box_is_the_toolkits_field'

MAIN += [
    # The search box is the toolkit's field, in the user's colours
    # (2026-10-04; lane C, c-e-a-theme-can-shape-the-controls).
    (
        'the search box never lights',
        '        hovered: open && state.hovered_search,\n',
        '        hovered: false,\n',
        [SEARCH],
    ),
    (
        'the search box is never marked',
        '        focused: open && state.search_focused,\n',
        '        focused: false,\n',
        [SEARCH],
    ),
    (
        'the search box shows through the list of keys',
        '    let open = !state.show_help && !state.picker.is_open();\n',
        '    let open = !state.picker.is_open();\n',
        [SEARCH],
    ),
    (
        'the pointer over the search box is not followed',
        '            state.hovered_search = search_box().contains(mx, my);\n',
        '',
        [SEARCH],
    ),
    (
        'the light stays after the pointer leaves',
        '            state.hovered_search = false;\n',
        '',
        [SEARCH],
    ),
    (
        "the window keeps the default colours whatever the theme",
        '        self.palette = *palette;\n',
        '        let _ = palette;\n',
        [SEARCH],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        '        self.focus_ring_width = settings.focus_ring_width();\n',
        '        let _ = settings;\n',
        [SEARCH],
    ),
]

# The search box edits at a caret (2026-10-04,
# known-issues/E-twenty-nine-applications-type-only-at-the-end-of-a-box): it
# took typing at its end and Backspace from it, and nothing else, and
# swallowed every chord.
EDITS = "the_search_box_edits_at_a_caret"
TREE_KEYS = "the_trees_keys_stay_out_of_the_search_box"
WHERE = "a_press_takes_the_search_box_where_it_is_drawn"
SHOWN = "the_box_edits_the_search_it_shows"
CTRL_F = "ctrl_f_selects_what_the_search_box_holds"

MAIN += [
    (
        "a plain key the box does not answer reaches the tree",
        "        if !textline::is_ctrl_chord(key.modifiers) {\n            return EventResult::Consumed;\n        }\n    }\n",
        "    }\n",
        [TREE_KEYS],
    ),
    (
        "a window chord is lost to the search box",
        "        if !textline::is_ctrl_chord(key.modifiers) {\n            return EventResult::Consumed;\n        }\n    }\n",
        "        return EventResult::Consumed;\n    }\n",
        [TREE_KEYS],
    ),
    (
        "Ctrl+F does not select what the box holds",
        "        self.search_editor.select_all();\n",
        "",
        [CTRL_F],
    ),
    (
        "a cut or a copy takes nothing to the clipboard",
        "            self.search_clipboard = copied;\n",
        "            let _ = copied;\n",
        [EDITS],
    ),
    (
        "an edit does not filter the tree",
        "            self.search_query = self.search_editor.text().to_owned();\n            self.apply_search_filter();\n",
        "            self.search_query = self.search_editor.text().to_owned();\n",
        [EDITS],
    ),
    (
        "a key finds the editor holding another search",
        "    fn search_key(&mut self, key: &KeyEvent) -> bool {\n        if self.search_editor.text() != self.search_query {\n",
        "    fn search_key(&mut self, key: &KeyEvent) -> bool {\n        if false {\n",
        [SHOWN],
    ),
    (
        "a press finds the editor holding another search",
        "        self.search_focused = true;\n        if self.search_editor.text() != self.search_query {\n",
        "        self.search_focused = true;\n        if false {\n",
        [SHOWN],
    ),
    (
        "a press puts the caret at the start",
        "            x - rect.x - SEARCH_TEXT_INSET,\n",
        "            0.0,\n",
        [EDITS],
    ),
    (
        "a press in the box does not give it the keyboard",
        "                state.press_search(mx);\n",
        "",
        [EDITS, WHERE],
    ),
    (
        "the band round the box takes the press",
        "            if search_box().contains(mx, my) {\n",
        "            if mx < SIDEBAR_WIDTH\n                && my >= TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT\n                && my < TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT + SEARCH_BAR_HEIGHT\n            {\n",
        [WHERE],
    ),
    (
        "the caret is drawn at the start",
        "                cursor: state.search_cursor(),\n",
        "                cursor: TextCursor::default(),\n",
        [EDITS],
    ),
    (
        "the selection is not drawn",
        "                selection_anchor: if editing {\n",
        "                selection_anchor: if false {\n",
        [EDITS],
    ),
    (
        "an empty box with the keyboard has no caret",
        "        if focused {\n            textedit::push_caret(",
        "        if false {\n            textedit::push_caret(",
        [EDITS],
    ),
]

TABLES = {
    "inventory.rs": INVENTORY,
    "main.rs": MAIN,
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
        worst = max(worst, sweep(SRC / file, rows, "devicemanager", timeout=900, only=mine))
    raise SystemExit(worst)
