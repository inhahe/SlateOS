import pathlib

p = pathlib.Path("apps/mindmap/src/main.rs")
s = p.read_text(encoding="utf-8")

old = """        // Down selects the first CHILD, so a map with no children cannot move
        // whether or not the dialog took the key -- the first version of this
        // test asserted that and passed for the wrong reason. The scanner
        // caught it: cutting the routing left it green.
        let child = app.add_child_to_selected(String::from("a child"));
        assert!(child.is_some(), "control: the fixture needs somewhere to move to");
        let before = app.selected_node;"""
assert s.count(old) == 1, s.count(old)

new = """        // Down selects the first CHILD, and needs both a selection to start
        // from and a child to move to. A new map has neither: `selected_node`
        // is `None`, so `select_first_child` returns immediately. Two earlier
        // versions of this test asserted `None == None` and passed with the
        // dialog doing nothing -- the scanner caught both, by cutting the
        // routing and staying green.
        app.selected_node = Some(app.active_map_ref().root_id);
        let child = app.add_child_to_selected(String::from("a child"));
        assert!(child.is_some(), "control: the fixture needs somewhere to move to");
        let before = app.selected_node;
        assert!(before.is_some(), "control: something must be selected to move from");"""

p.write_text(s.replace(old, new), encoding="utf-8", newline="\n")
print("mindmap fixture selects a root")
