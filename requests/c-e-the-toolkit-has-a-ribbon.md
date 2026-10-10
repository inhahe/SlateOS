# C -> E: the toolkit has a ribbon

**From:** Lane C. **To:** Lane E. **Filed:** 2026-09-30.
**Status:** OPEN -- for any application with more commands than a menu
bar and a toolbar show well. Nothing is broken until then.

**In short:** `guitk::ribbon` is a bar of commands in tabs (Home, View,
...), each tab showing its commands in named groups: big buttons, small
ones, on/off switches, buttons with a menu, dropdown lists and rows of
picture choices. When the window is narrow, whole groups fold into single
buttons. It has keyboard access (F10, then a letter), tooltips that say why
a disabled command cannot be used, a Quick Access Toolbar the user fills,
and the user's own changes -- tabs hidden or moved, commands added to or
taken out of groups -- saved as one line of your settings file. It is a
widget, not a requirement: an application that is happy with a menu bar
keeps it.

## How an application uses it

```rust
use guitk::ribbon::{ButtonSize, Choice, Command, Control, Group, Ribbon, RibbonEvent, RibbonTab};

// Your command numbers: stable from one run to the next, since the user's
// toolbar and changes are saved by them.
const PASTE: u64 = 1;
const BOLD: u64 = 2;
const FONT: u64 = 3;

let mut ribbon = Ribbon::new(vec![
    RibbonTab::new("home", "Home")
        .with(Group::new("clipboard", "Clipboard").with_icon("edit-paste").with_priority(9)
            .with(Control::button(Command::new(PASTE, "Paste").with_icon("edit-paste"), ButtonSize::Large)))
        .with(Group::new("font", "Font").with_priority(5)
            .with(Control::dropdown(Command::new(FONT, "Font"), vec![Choice::new("Sans"), Choice::new("Serif")], 120.0))
            .with(Control::toggle(Command::new(BOLD, "Bold").with_icon("format-text-bold"), ButtonSize::Small))),
    RibbonTab::new("picture", "Picture").contextual("picture") /* shown while set_context("picture", true) */,
]);
ribbon.apply_customization(&settings.get_str(&["ribbon"]).unwrap_or_default());

// Each frame: lay it out at the top, draw your page under it, draw it last
// (a folded group's panel and a menu lie over your page).
let layout = ribbon.layout(0.0, 0.0, width, (width, height));
draw_page(layout.rect.bottom());
guitk::ribbon::draw(&mut sink, &palette, &ribbon, &layout, &|name, size| icons.find(name, size));

// Input: pointer and keys to the ribbon first; `Ignored` means it is yours.
match ribbon.handle_mouse(&layout, &event) {
    RibbonEvent::Command(PASTE) => paste(),
    RibbonEvent::Toggled { id: BOLD, on } => set_bold(on),
    RibbonEvent::Chose { id: FONT, index } => set_font(index),
    RibbonEvent::Customized => settings.set_str(&["ribbon"], &ribbon.customization_text()),
    RibbonEvent::Ignored => { /* the page's */ }
    _ => {}
}
if ribbon.tick(now_ms) { redraw(); }            // tooltips: after every event, and
let wake = ribbon.tooltip_due_in(now_ms);      // when this says
```

- **State is the command's.** `set_enabled(id, false, Some("Select some
  text first"))`, `set_on(id, on)` and `set_selected(id, index)` change every
  place the command appears; the reason shows in its tooltip.
- **`Ribbon::offer(command)`** lists a command the user may add to a group
  that no tab shows by default.
- **`Group::priority`** decides which groups fold first when the window is
  narrow: the lowest. Give the groups your users reach for most the highest.
- **Keys:** F10 shows the key tips, Ctrl+F1 minimizes. Give the ribbon keys
  before your own shortcuts, and take them back when it answers `Ignored`.

`gui/toolkit/src/ribbon.rs` documents the rest, and `design-decisions.md`
§1453 why it is deliberately not Office's in three places (no staged
shrinking of groups, no gallery preview, key tips in one layer). Its tests
(`ribbon_tests.rs`) are worked examples.

## Where it would help first

Command-dense applications with a toolbar of their own today:
`apps/explorer`, `apps/imageviewer`, `apps/photomanager` and
`apps/pdfviewer` each draw a toolbar by hand, and `apps/editor` has a menu
bar. `apps/paint` and `apps/spreadsheet` have neither and many commands.
Which to move, and when, is yours: a ribbon suits an application whose
commands a user browses; one whose commands a user already knows by heart
may be better with a menu bar.
