"""Three settings surfaces that present controls for acts nothing performs."""

import pathlib


def put_const_above_first_item(s: str, const: str, marker: str = "pub struct ") -> str:
    i = s.index(marker)
    while True:
        prev = s.rindex(chr(10), 0, i - 1) + 1
        if s[prev:i].lstrip().startswith(("///", "//", "#[")):
            i = prev
            continue
        break
    return s[:i] + const + "\n\n" + s[i:]


# ---------------------------------------------------------------- videoplayer
p = pathlib.Path("apps/videoplayer/src/main.rs")
s = p.read_text(encoding="utf-8")
s = put_const_above_first_item(s, '''/// What the Screenshots block says under the options it offers.
///
/// **The shortcut for this was already removed as impossible.** `Shortcuts
/// ::list` used to advertise `Ctrl+S` (take a screenshot) and no longer does,
/// with the reason recorded there: "this tree has neither a file chooser nor a
/// way to read back the framebuffer, and a help panel that promises what the
/// program cannot do is the same defect one level up".
///
/// The settings block was left behind, still naming a format, a quality and a
/// subtitle option. `CANNOT_PLAY_LINES` does cover the prerequisites -- no
/// frame is decoding and there is no filesystem -- but it is drawn at the top
/// left of the window, and relying on a reader having seen it before reaching
/// a settings panel is what `apps/mediaconvert` got wrong: three lines about
/// the queue did not reach the panel that configured it.
const NO_SCREENSHOTS: &str = "Not applied: this player cannot take a \\
screenshot -- no frame is decoded and there is nowhere to write one.";''')

old = s[s.index('            text: "Screenshots".to_string(),'):]
anchor = '''                    "Yes"
                } else {
                    "No"
                }
            ),'''
assert s.count(anchor) == 1
end = s.index(anchor) + len(anchor)
rest = s.index("});", end) + len("});")
s = s[:rest] + '''

        cmds.push(RenderCommand::Text {
            x: label_x + 8.0,
            y: extra_y + 144.0,
            text: NO_SCREENSHOTS.to_owned(),
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(500.0),
            overflow: TextOverflow::Ellipsis,
        });''' + s[rest:]
p.write_text(s, encoding="utf-8", newline="\n")
print("videoplayer: NO_SCREENSHOTS added")

# ------------------------------------------------------------- fontmanager
p = pathlib.Path("apps/fontmanager/src/main.rs")
s = p.read_text(encoding="utf-8")
s = put_const_above_first_item(s, '''/// What the render-settings panel says under the values it offers.
///
/// `default_size_pt` is read in exactly one place -- the `format!` that draws
/// "11.0 pt" beside the label "Default Size". The preview below renders at
/// `PREVIEW_SIZE_LABELS`, a fixed list, and never at this number.
///
/// **It is a system setting with no route out of this window.** A default UI
/// font size belongs to the toolkit, and `gui/toolkit` picks its faces from
/// hardcoded candidate lists; the kernel's `/proc/fontsettings` publishes a
/// `default_size_dp` that nothing here reads and nothing here could write.
/// Both ends of that loop are open, which is why the notice says "nothing
/// carries" rather than "not implemented" -- the work is not in this file.
const SETTINGS_NOT_CARRIED: &str = "Not applied: nothing carries these to the \\
toolkit, and the preview below renders at its own fixed sizes.";''')

old_row = '''        let size_str = format!("{:.1} pt", self.render_settings.default_size_pt);
        tree.text(value_x, y, &size_str, self.palette.blue, 13.0);
        y += 32.0;'''
assert s.count(old_row) == 1
s = s.replace(old_row, '''        let size_str = format!("{:.1} pt", self.render_settings.default_size_pt);
        tree.text(value_x, y, &size_str, self.palette.blue, 13.0);
        y += 18.0;
        // Directly under the first value rather than at the foot of the panel:
        // the numbers above are what read as confirmation.
        tree.text(label_x, y, SETTINGS_NOT_CARRIED, self.palette.subtext0, 11.0);
        y += 22.0;''', 1)
p.write_text(s, encoding="utf-8", newline="\n")
print("fontmanager: SETTINGS_NOT_CARRIED added")

# ---------------------------------------------------------- settings/remote
p = pathlib.Path("apps/settings/src/remote.rs")
s = p.read_text(encoding="utf-8")
const = '''/// What the Remote Desktop and Dynamic DNS pages say about themselves.
///
/// **Neither `RemoteDesktopConfig` nor `DynDnsConfig` is named by any file
/// outside this one.** Nothing serves a remote desktop, nothing updates a DNS
/// record, and `idle_timeout_minutes` and `update_interval_minutes` are read
/// in exactly one place each: the `format!` that draws them as "N min".
///
/// This page mattered more than the others of its kind, which is why it has a
/// notice rather than a mention in a tracking file. The other eight were app
/// windows; this is **Settings**, the surface an operator goes to precisely
/// when they want to know what the machine is configured to do. A page here
/// that answers confidently is the most convincing wrong answer the system can
/// give.
const NOTHING_APPLIES: &str = "Not applied: nothing on this system serves a \\
remote desktop or updates a DNS record. These values are stored and read back \\
here, and nowhere else.";

'''
i = s.index("fn render_section_header")
while True:
    prev = s.rindex(chr(10), 0, i - 1) + 1
    if s[prev:i].lstrip().startswith(("///", "//", "#[")):
        i = prev
        continue
    break
s = s[:i] + const + s[i:]

for old_r, label in (
    ('''    let interval_str = format!("{} min", cfg.update_interval_minutes);
    y = render_dropdown_row(pal, tree, x, y, "Update Interval", &interval_str);''',
     "dyndns"),
    ('''    let timeout_str = format!("{} min", config.idle_timeout_minutes);
    y = render_dropdown_row(pal, tree, x, y, "Idle Timeout", &timeout_str);''',
     "remote desktop"),
):
    assert s.count(old_r) == 1, label
    s = s.replace(old_r, old_r + '''
    text_bold(tree, x, y, NOTHING_APPLIES, pal.subtext0, 11.0);
    y += 18.0;''', 1)
p.write_text(s, encoding="utf-8", newline="\n")
print("settings/remote: NOTHING_APPLIES added twice")
