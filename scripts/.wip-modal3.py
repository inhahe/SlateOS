"""The modal half of the door, in three more apps."""

import pathlib

DOC = """    /// An open picker takes the keyboard, and the window behind it does not.
    ///
    /// The other door test pins that the key *opens* the picker -- but the key
    /// handler is what opens it, so cutting the picker's event routing
    /// entirely leaves that assertion true. This is the half routing actually
    /// decides: with a dialog up, a keystroke belongs to the dialog, and a
    /// window that scrolls underneath one is a modal that is not modal.
    ///
    /// Found by `scripts/find-unpinned-picker-routing.py`, which cuts the
    /// routing and reports whose tests notice. Sixteen of twenty did not.
"""

EDITS = [
    (
        "apps/filediff/src/main.rs",
        "    /// Two real files are read and compared.",
        DOC
        + """    #[test]
    fn an_open_picker_takes_the_keyboard_from_the_panes() {
        let mut app = FileDiffApp::new();
        app.left_content = String::from("a\\nb\\nc\\nd\\ne\\nf\\ng\\nh\\ni\\nj\\n");
        app.right_content = app.left_content.clone();
        app.recompute_diff();
        let before = app.scroll_left;

        app.handle_event(&ctrl_key(Key::O, false));
        assert!(app.picker.is_open(), "control: the picker must be up");

        app.handle_event(&Event::Key(KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }));
        assert_eq!(
            app.scroll_left, before,
            "Down at the open dialog scrolled the pane behind it"
        );
    }

""",
    ),
    (
        "apps/renamer/src/main.rs",
        "    /// The report reaches the disk.",
        DOC
        + """    #[test]
    fn an_open_picker_takes_the_keyboard_from_the_file_list() {
        let mut app = RenamerApp::new();
        app.add_file("one.txt", 10, 0);
        app.add_file("two.txt", 10, 0);
        app.selected_file = 0;

        app.handle_event(&ctrl(Key::O));
        assert!(app.picker.is_open(), "control: the picker must be up");

        app.handle_event(&press(Key::Down));
        assert_eq!(
            app.selected_file, 0,
            "Down at the open dialog moved the selection behind it"
        );
    }

""",
    ),
    (
        "apps/musicplayer/src/main.rs",
        "    /// A playlist survives a save and an open.",
        DOC
        + """    #[test]
    fn an_open_picker_takes_the_keyboard_from_the_playlist() {
        let mut state = PlayerState::new();
        state.add_track(Track::from_path(PathBuf::from("/music/one.mp3")));
        state.add_track(Track::from_path(PathBuf::from("/music/two.mp3")));
        state.current_track_index = Some(0);

        handle_event(&mut state, &ctrl_key(Key::O));
        assert!(state.picker.is_open(), "control: the picker must be up");

        // `n` is the next-track shortcut, and a letter somebody might type
        // into a filename.
        handle_event(
            &mut state,
            &Event::Key(KeyEvent {
                key: Key::N,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: String::from("n"),
            }),
        );
        assert_eq!(
            state.current_track_index,
            Some(0),
            "a letter typed at the open dialog changed the track behind it"
        );
    }

""",
    ),
]

for path, anchor, block in EDITS:
    p = pathlib.Path(path)
    s = p.read_text(encoding="utf-8")
    assert s.count(anchor) == 1, f"{s.count(anchor)} anchors in {path}"
    p.write_text(s.replace(anchor, block + anchor), encoding="utf-8", newline="\n")
    print(f"{path}: modal test added")
