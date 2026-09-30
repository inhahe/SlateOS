// A test module's job is to fail loudly the instant the code under test is
// wrong, so the defensive lints that forbid exactly that in production code
// are off here -- as `CLAUDE.md` prescribes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    clippy::cast_precision_loss
)]

use super::*;
use crate::event::Modifiers;

// ---- a ribbon to test ----

const PASTE: CommandId = 1;
const PASTE_SPECIAL: MenuItemId = 101;
const PASTE_TEXT: MenuItemId = 102;
const CUT: CommandId = 2;
const COPY: CommandId = 3;
const FONT: CommandId = 10;
const BOLD: CommandId = 11;
const ITALIC: CommandId = 12;
const UNDERLINE: CommandId = 13;
const STYLES: CommandId = 20;
const ZOOM: CommandId = 30;
const CROP: CommandId = 40;

fn row(id: MenuItemId, label: &str) -> MenuItem {
    MenuItem::Action {
        id,
        label: label.to_owned(),
        shortcut: None,
        icon: None,
        enabled: true,
        checked: None,
    }
}

/// Home (clipboard, font, styles), View (zoom), and a Picture tab for the
/// "picture" context.
fn tabs() -> Vec<RibbonTab> {
    vec![
        RibbonTab::new("home", "Home")
            .with(
                Group::new("clipboard", "Clipboard")
                    .with_icon("edit-paste")
                    .with_priority(3)
                    .with(Control::split(
                        Command::new(PASTE, "Paste").with_icon("edit-paste"),
                        ButtonSize::Large,
                        vec![
                            row(PASTE_SPECIAL, "Paste special"),
                            row(PASTE_TEXT, "Paste as text"),
                        ],
                    ))
                    .with(Control::button(
                        Command::new(CUT, "Cut").with_icon("edit-cut"),
                        ButtonSize::Medium,
                    ))
                    .with(Control::button(
                        Command::new(COPY, "Copy").with_icon("edit-copy"),
                        ButtonSize::Medium,
                    )),
            )
            .with(
                Group::new("font", "Font")
                    .with_priority(2)
                    .with(Control::dropdown(
                        Command::new(FONT, "Font"),
                        vec![
                            Choice::new("Sans"),
                            Choice::new("Serif"),
                            Choice::new("Mono"),
                        ],
                        110.0,
                    ))
                    .with(Control::toggle(
                        Command::new(BOLD, "Bold").with_icon("format-text-bold"),
                        ButtonSize::Small,
                    ))
                    .with(Control::toggle(
                        Command::new(ITALIC, "Italic").with_icon("format-text-italic"),
                        ButtonSize::Small,
                    ))
                    .with(Control::button(
                        Command::new(UNDERLINE, "Underline")
                            .with_icon("format-text-underline")
                            .disabled_because("Select some text first"),
                        ButtonSize::Medium,
                    )),
            )
            .with(
                Group::new("styles", "Styles")
                    .with_priority(1)
                    .with(Control::gallery(
                        Command::new(STYLES, "Styles"),
                        vec![
                            Choice::new("Normal").with_icon("style-normal"),
                            Choice::new("Title"),
                            Choice::new("Heading 1"),
                            Choice::new("Heading 2"),
                            Choice::new("Quote"),
                        ],
                        3,
                    )),
            ),
        RibbonTab::new("view", "View").with(Group::new("zoom", "Zoom").with(Control::button(
            Command::new(ZOOM, "Zoom"),
            ButtonSize::Large,
        ))),
        RibbonTab::new("picture", "Picture")
            .contextual("picture")
            .with(Group::new("adjust", "Adjust").with(Control::button(
                Command::new(CROP, "Crop"),
                ButtonSize::Large,
            ))),
    ]
}

const VIEWPORT: (f32, f32) = (2400.0, 1000.0);

fn ribbon() -> Ribbon {
    Ribbon::new(tabs())
}

fn layout_at(r: &Ribbon, width: f32) -> RibbonLayout {
    r.layout(0.0, 0.0, width, VIEWPORT)
}

fn wide(r: &Ribbon) -> RibbonLayout {
    layout_at(r, 2000.0)
}

fn mouse(x: f32, y: f32, kind: MouseEventKind) -> MouseEvent {
    MouseEvent { x, y, kind }
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn press(r: &mut Ribbon, l: &RibbonLayout, (x, y): (f32, f32)) -> RibbonEvent {
    r.handle_mouse(l, &mouse(x, y, MouseEventKind::Press(MouseButton::Left)))
}

fn release(r: &mut Ribbon, l: &RibbonLayout, (x, y): (f32, f32)) -> RibbonEvent {
    r.handle_mouse(l, &mouse(x, y, MouseEventKind::Release(MouseButton::Left)))
}

/// Press and release at one point, answering the release's event -- or the
/// press's, when the press already said something.
fn click(r: &mut Ribbon, l: &RibbonLayout, at: (f32, f32)) -> RibbonEvent {
    let pressed = press(r, l, at);
    let released = release(r, l, at);
    if released == RibbonEvent::Handled {
        pressed
    } else {
        released
    }
}

fn key(k: Key, ctrl: bool) -> KeyEvent {
    KeyEvent {
        key: k,
        pressed: true,
        modifiers: Modifiers {
            ctrl,
            ..Modifiers::NONE
        },
        text: String::new(),
    }
}

/// Where the control of command `id` is in `l`: the panel's first, then the
/// body's.
fn slot<'a>(r: &Ribbon, l: &'a RibbonLayout, id: CommandId) -> &'a ControlSlot {
    l.panel
        .iter()
        .chain(l.body.iter())
        .flat_map(|b| &b.groups)
        .flat_map(|g| g.controls.iter().map(move |c| (g.group, c)))
        .find(|(g, c)| {
            r.control_at(*g, c.control)
                .is_some_and(|c| c.command().id == id)
        })
        .map(|(_, c)| c)
        .unwrap_or_else(|| panic!("command {id} is not laid out"))
}

fn tab_slot(l: &RibbonLayout, r: &Ribbon, id: &str) -> Rect {
    l.tabs
        .iter()
        .find(|t| r.tabs()[t.tab].id == id)
        .map(|t| t.rect)
        .unwrap_or_else(|| panic!("tab {id} is not shown"))
}

/// The id of the row reached through the menu rows labelled `path`.
fn row_id(items: &[MenuItem], path: &[&str]) -> MenuItemId {
    let (first, rest) = path.split_first().unwrap();
    for item in items {
        match item {
            MenuItem::Action { id, label, .. } if label == first && rest.is_empty() => return *id,
            MenuItem::Submenu {
                label, children, ..
            } if label == first && !rest.is_empty() => return row_id(children, rest),
            _ => {}
        }
    }
    panic!("no row {path:?}")
}

/// Choose the row `path` of the open menu, as a click on it or Enter over it
/// does.
fn choose(r: &mut Ribbon, path: &[&str]) -> RibbonEvent {
    let open = r.menu.as_ref().expect("no menu is open");
    let id = row_id(open.menu.items(), path);
    let act = open.acts[usize::try_from(id).unwrap()];
    r.menu = None;
    r.do_act(act)
}

/// The labels of the open menu's top rows.
fn menu_labels(r: &Ribbon) -> Vec<String> {
    r.menu
        .as_ref()
        .expect("no menu is open")
        .menu
        .items()
        .iter()
        .filter_map(|item| match item {
            MenuItem::Action { label, .. } | MenuItem::Submenu { label, .. } => Some(label.clone()),
            MenuItem::Separator => None,
        })
        .collect()
}

fn group_slot(l: &RibbonLayout, group: usize) -> &GroupSlot {
    l.body
        .as_ref()
        .unwrap()
        .groups
        .iter()
        .find(|g| g.group == group)
        .unwrap_or_else(|| panic!("group {group} is not in the body"))
}

// ---- tabs and contexts ----

/// **The first ordinary tab is in front, and a contextual tab is not shown
/// while its context is not active.**
#[test]
fn the_first_ordinary_tab_is_in_front() {
    let r = ribbon();
    assert_eq!(r.front_id(), Some("home"));
    assert_eq!(r.visible_tabs(), [0, 1]);
    let l = wide(&r);
    assert_eq!(l.tabs.len(), 2);
    assert_eq!(l.rect.h, STRIP_HEIGHT + BODY_HEIGHT);
    assert_eq!(l.body.as_ref().unwrap().groups.len(), 3);
}

/// **A context shows its tab after the ordinary ones, set apart; its ending
/// takes the tab away, and from the front to the first tab.**
#[test]
fn a_context_shows_its_tab_after_the_others_and_takes_it_away() {
    let mut r = ribbon();
    assert!(
        !r.select_tab("picture"),
        "a tab not shown came to the front"
    );
    r.set_context("picture", true);
    assert!(r.context_active("picture"));
    assert_eq!(r.visible_tabs(), [0, 1, 2]);
    let l = wide(&r);
    let view = tab_slot(&l, &r, "view");
    let picture = tab_slot(&l, &r, "picture");
    assert!(
        picture.x >= view.right() + CONTEXT_GAP,
        "the contextual tab is not set apart: {view:?} {picture:?}"
    );
    assert!(r.select_tab("picture"));
    assert_eq!(r.front_id(), Some("picture"));

    r.set_context("picture", false);
    assert_eq!(r.visible_tabs(), [0, 1]);
    assert_eq!(
        r.front_id(),
        Some("home"),
        "the ended context's tab stayed in front"
    );
}

/// **A press on a tab brings it to the front, and says so; a press on the
/// tab in front says nothing more.**
#[test]
fn a_press_on_a_tab_brings_it_to_the_front() {
    let mut r = ribbon();
    let l = wide(&r);
    let view = centre(tab_slot(&l, &r, "view"));
    assert_eq!(
        press(&mut r, &l, view),
        RibbonEvent::TabSelected("view".into())
    );
    assert_eq!(r.front_id(), Some("view"));
    let l = wide(&r);
    assert_eq!(l.body.as_ref().unwrap().groups.len(), 1);
    assert_eq!(press(&mut r, &l, view), RibbonEvent::Handled);
}

/// **New tabs keep the tab in front where it is still there, and close what
/// was open.**
#[test]
fn new_tabs_keep_the_front_tab_and_close_what_was_open() {
    let mut r = ribbon();
    r.select_tab("view");
    let l = wide(&r);
    let zoom = centre(slot(&r, &l, ZOOM).rect);
    press(&mut r, &l, zoom);
    r.set_tabs(tabs());
    assert_eq!(r.front_id(), Some("view"));
    assert!(r.pressed.is_none());

    let mut r = ribbon();
    r.select_tab("view");
    r.set_tabs(tabs().into_iter().filter(|t| t.id != "view").collect());
    assert_eq!(
        r.front_id(),
        Some("home"),
        "a tab that went stayed in front"
    );
}

// ---- controls ----

/// **A button does its command when released over the face it was pressed
/// on -- and not when the pointer was dragged off it.**
#[test]
fn a_button_does_its_command_on_release_over_it() {
    let mut r = ribbon();
    let l = wide(&r);
    let cut = centre(slot(&r, &l, CUT).rect);
    let copy = centre(slot(&r, &l, COPY).rect);
    assert_eq!(click(&mut r, &l, cut), RibbonEvent::Command(CUT));

    assert_eq!(press(&mut r, &l, cut), RibbonEvent::Handled);
    assert_eq!(
        release(&mut r, &l, copy),
        RibbonEvent::Handled,
        "dragged onto Copy, Copy ran"
    );
    assert_eq!(press(&mut r, &l, cut), RibbonEvent::Handled);
    assert_eq!(release(&mut r, &l, (5000.0, 5000.0)), RibbonEvent::Ignored);
    assert_eq!(click(&mut r, &l, copy), RibbonEvent::Command(COPY));
}

/// **A toggle flips when pressed and says which way; the application can set
/// it too.**
#[test]
fn a_toggle_flips_and_says_which_way() {
    let mut r = ribbon();
    let l = wide(&r);
    let bold = centre(slot(&r, &l, BOLD).rect);
    assert_eq!(
        click(&mut r, &l, bold),
        RibbonEvent::Toggled { id: BOLD, on: true }
    );
    assert_eq!(
        click(&mut r, &l, bold),
        RibbonEvent::Toggled {
            id: BOLD,
            on: false
        }
    );
    assert!(r.set_on(BOLD, true));
    assert!(matches!(
        r.control(BOLD),
        Some(Control::Toggle { on: true, .. })
    ));
    assert!(!r.set_on(CUT, true), "a button was treated as a toggle");
}

/// **A control that cannot be used does nothing, and keeps why.**
#[test]
fn a_disabled_control_does_nothing_and_keeps_why() {
    let mut r = ribbon();
    let l = wide(&r);
    let underline = centre(slot(&r, &l, UNDERLINE).rect);
    assert_eq!(click(&mut r, &l, underline), RibbonEvent::Handled);
    assert_eq!(
        r.control(UNDERLINE)
            .unwrap()
            .command()
            .disabled_reason
            .as_deref(),
        Some("Select some text first")
    );
    assert!(r.set_enabled(UNDERLINE, true, None));
    assert_eq!(
        r.control(UNDERLINE).unwrap().command().disabled_reason,
        None
    );
    assert_eq!(
        click(&mut r, &l, underline),
        RibbonEvent::Command(UNDERLINE)
    );
    assert!(r.set_enabled(UNDERLINE, false, Some("Not here")));
    assert_eq!(click(&mut r, &l, underline), RibbonEvent::Handled);
}

/// **A split button's face does its command; its arrow opens its menu, whose
/// rows report the split button and their own ids.**
#[test]
fn a_split_buttons_face_does_its_command_and_its_arrow_opens_its_menu() {
    let mut r = ribbon();
    let l = wide(&r);
    let paste = slot(&r, &l, PASTE).clone();
    assert_eq!(
        click(&mut r, &l, centre(paste.face)),
        RibbonEvent::Command(PASTE)
    );

    let arrow = paste.arrow.expect("a split button has an arrow");
    assert!(
        arrow.y >= paste.face.bottom(),
        "a large split button's arrow is under its face"
    );
    assert_eq!(press(&mut r, &l, centre(arrow)), RibbonEvent::Handled);
    assert!(r.menu_open());
    assert_eq!(menu_labels(&r), ["Paste special", "Paste as text"]);
    // Through the menu itself: a press on its second row.
    let row = r.menu.as_ref().unwrap().menu.item_rect(1).unwrap();
    assert_eq!(
        press(&mut r, &l, centre(row)),
        RibbonEvent::MenuItem {
            id: PASTE,
            item: PASTE_TEXT
        }
    );
    assert!(!r.menu_open());
}

/// **A press outside an open menu closes it and does nothing else.**
#[test]
fn a_press_outside_a_menu_closes_it_and_does_nothing_else() {
    let mut r = ribbon();
    let l = wide(&r);
    let arrow = centre(slot(&r, &l, PASTE).arrow.unwrap());
    press(&mut r, &l, arrow);
    assert!(r.menu_open());
    let cut = centre(slot(&r, &l, CUT).rect);
    assert_eq!(press(&mut r, &l, cut), RibbonEvent::Handled);
    assert!(!r.menu_open());
    assert_eq!(
        release(&mut r, &l, cut),
        RibbonEvent::Handled,
        "the press that closed the menu ran Cut"
    );
}

/// **A dropdown lists its choices, the chosen one checked, and reports the
/// one chosen.**
#[test]
fn a_dropdown_lists_its_choices_and_reports_the_one_chosen() {
    let mut r = ribbon();
    let l = wide(&r);
    let font = slot(&r, &l, FONT).clone();
    assert_eq!(font.rect.w, 110.0);
    assert_eq!(press(&mut r, &l, centre(font.rect)), RibbonEvent::Handled);
    assert_eq!(menu_labels(&r), ["Sans", "Serif", "Mono"]);
    assert_eq!(
        choose(&mut r, &["Serif"]),
        RibbonEvent::Chose { id: FONT, index: 1 }
    );
    assert!(matches!(
        r.control(FONT),
        Some(Control::Dropdown {
            selected: Some(1),
            ..
        })
    ));

    press(&mut r, &l, centre(font.rect));
    let items = r.menu.as_ref().unwrap().menu.items().to_vec();
    assert!(
        matches!(
            items[1],
            MenuItem::Action {
                checked: Some(true),
                ..
            }
        ),
        "the chosen one is not checked"
    );
    assert!(matches!(
        items[0],
        MenuItem::Action {
            checked: Some(false),
            ..
        }
    ));

    assert!(r.set_selected(FONT, Some(9)), "the dropdown was not found");
    assert!(
        matches!(
            r.control(FONT),
            Some(Control::Dropdown { selected: None, .. })
        ),
        "a choice past the list was kept"
    );
}

/// **A gallery chooses on a click, lists every choice behind its arrow, and
/// shows the run of choices that ends with the one chosen.**
#[test]
fn a_gallery_chooses_on_a_click_and_lists_every_choice() {
    let mut r = ribbon();
    let l = wide(&r);
    let gallery = slot(&r, &l, STYLES).clone();
    assert_eq!(
        gallery.choices.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(
        click(&mut r, &l, centre(gallery.choices[1].1)),
        RibbonEvent::Chose {
            id: STYLES,
            index: 1
        }
    );

    press(&mut r, &l, centre(gallery.arrow.unwrap()));
    assert_eq!(
        menu_labels(&r),
        ["Normal", "Title", "Heading 1", "Heading 2", "Quote"]
    );
    assert_eq!(
        choose(&mut r, &["Quote"]),
        RibbonEvent::Chose {
            id: STYLES,
            index: 4
        }
    );
    let l = wide(&r);
    assert_eq!(
        slot(&r, &l, STYLES)
            .choices
            .iter()
            .map(|(i, _)| *i)
            .collect::<Vec<_>>(),
        [2, 3, 4],
        "the chosen one is not shown"
    );
}

/// **An open menu has the keyboard: Escape closes it, and the arrows and
/// Enter choose from it.**
#[test]
fn an_open_menu_has_the_keyboard() {
    let mut r = ribbon();
    let l = wide(&r);
    let font = centre(slot(&r, &l, FONT).rect);
    press(&mut r, &l, font);
    assert_eq!(r.handle_key(&key(Key::Escape, false)), RibbonEvent::Handled);
    assert!(!r.menu_open());

    press(&mut r, &l, font);
    assert_eq!(r.handle_key(&key(Key::Down, false)), RibbonEvent::Handled);
    let chosen = r.handle_key(&key(Key::Enter, false));
    assert!(
        matches!(chosen, RibbonEvent::Chose { id: FONT, .. }),
        "Enter chose nothing: {chosen:?}"
    );
    assert_eq!(
        r.handle_key(&key(Key::Escape, false)),
        RibbonEvent::Ignored,
        "Escape with nothing open was taken"
    );
}

// ---- folding ----

/// The whole widths of the front tab's groups, and their folded widths.
fn widths(r: &Ribbon) -> (Vec<f32>, Vec<f32>) {
    let groups = &r.tabs()[r.front().unwrap()].groups;
    (
        groups.iter().map(group_width).collect(),
        groups.iter().map(folded_width).collect(),
    )
}

fn folded(l: &RibbonLayout) -> Vec<usize> {
    l.body
        .as_ref()
        .unwrap()
        .groups
        .iter()
        .filter(|g| g.button.is_some())
        .map(|g| g.group)
        .collect()
}

/// **Groups fold when they do not fit -- the lowest priority first, one at a
/// time -- and what does not fit folded waits behind `»`.**
#[test]
fn groups_fold_lowest_priority_first() {
    let r = ribbon();
    let (whole, small) = widths(&r);
    let all: f32 = whole.iter().sum();

    assert!(
        folded(&layout_at(&r, all)).is_empty(),
        "a ribbon that fits folded something"
    );
    // Styles (priority 1) first, then Font (2), then Clipboard (3).
    assert_eq!(folded(&layout_at(&r, all - 1.0)), [2]);
    let styles_folded = whole[0] + whole[1] + small[2];
    assert_eq!(folded(&layout_at(&r, styles_folded)), [2]);
    assert_eq!(folded(&layout_at(&r, styles_folded - 1.0)), [1, 2]);
    let all_folded: f32 = small.iter().sum();
    assert_eq!(folded(&layout_at(&r, all_folded)), [0, 1, 2]);
    assert!(layout_at(&r, all_folded).body.unwrap().overflow.is_none());

    let l = layout_at(&r, all_folded - 1.0);
    let body = l.body.as_ref().unwrap();
    let overflow = body.overflow.as_ref().expect("nothing waits behind »");
    let shown: Vec<usize> = body.groups.iter().map(|g| g.group).collect();
    assert_eq!(
        shown
            .iter()
            .chain(&overflow.groups)
            .copied()
            .collect::<Vec<_>>(),
        [0, 1, 2],
        "a group was lost, or the order changed"
    );
    assert!(
        overflow.rect.right() <= all_folded - 1.0,
        "» is past the edge"
    );
}

/// **Among groups of one priority, the one furthest right folds first.**
#[test]
fn among_equals_the_rightmost_folds_first() {
    let mut tabs = tabs();
    for g in &mut tabs[0].groups {
        g.priority = 5;
    }
    let r = Ribbon::new(tabs);
    let (whole, _) = widths(&r);
    let all: f32 = whole.iter().sum();
    assert_eq!(folded(&layout_at(&r, all - 1.0)), [2]);
}

/// **A folded group opens whole in a panel under it; a command given there
/// closes it, and so do a second press on the button and a press
/// elsewhere.**
#[test]
fn a_folded_group_opens_whole_in_a_panel_under_it() {
    let mut r = ribbon();
    let (whole, small) = widths(&r);
    let width = whole[0] + whole[1] + small[2];
    let l = layout_at(&r, width);
    let styles = group_slot(&l, 2).clone();
    let button = styles.button.expect("styles is not folded");
    assert_eq!(press(&mut r, &l, centre(button)), RibbonEvent::Handled);
    assert!(r.panel_open());
    let l = layout_at(&r, width);
    let panel = l.panel.as_ref().expect("no panel is laid out");
    assert_eq!(
        panel.rect.y,
        styles.rect.bottom(),
        "the panel is not under the group"
    );
    assert!(
        panel.rect.right() <= width,
        "the panel runs past the ribbon"
    );
    assert_eq!(panel.groups[0].controls.len(), 1);

    let cell = slot(&r, &l, STYLES).choices[0].1;
    assert_eq!(
        click(&mut r, &l, centre(cell)),
        RibbonEvent::Chose {
            id: STYLES,
            index: 0
        }
    );
    assert!(!r.panel_open(), "the panel stayed open after its command");

    let l = layout_at(&r, width);
    press(&mut r, &l, centre(button));
    let l = layout_at(&r, width);
    assert_eq!(press(&mut r, &l, centre(button)), RibbonEvent::Handled);
    assert!(!r.panel_open(), "a second press on the button left it open");

    let l = layout_at(&r, width);
    press(&mut r, &l, centre(button));
    let l = layout_at(&r, width);
    let cut = centre(slot(&r, &l, CUT).rect);
    assert_eq!(press(&mut r, &l, cut), RibbonEvent::Handled);
    assert!(!r.panel_open());
    assert_eq!(
        release(&mut r, &l, cut),
        RibbonEvent::Handled,
        "the press that closed the panel ran Cut"
    );
    assert_eq!(click(&mut r, &l, cut), RibbonEvent::Command(CUT));
}

/// **Escape closes an open panel.**
#[test]
fn escape_closes_an_open_panel() {
    let mut r = ribbon();
    let (whole, small) = widths(&r);
    let width = whole[0] + whole[1] + small[2];
    let l = layout_at(&r, width);
    press(&mut r, &l, centre(group_slot(&l, 2).button.unwrap()));
    assert_eq!(r.handle_key(&key(Key::Escape, false)), RibbonEvent::Handled);
    assert!(!r.panel_open());
}

/// **`»` holds the groups there is no room for, each a submenu of its
/// controls: a button a row, a toggle a checked row, a dropdown a submenu of
/// its choices, a split button its face and its menu.**
#[test]
fn the_overflow_menu_holds_the_groups_there_is_no_room_for() {
    let mut r = ribbon();
    let (_, small) = widths(&r);
    let l = layout_at(&r, small[0] + OVERFLOW_WIDTH + 1.0);
    let overflow = l
        .body
        .as_ref()
        .unwrap()
        .overflow
        .clone()
        .expect("nothing overflowed");
    assert_eq!(overflow.groups, [1, 2]);
    assert_eq!(
        press(&mut r, &l, centre(overflow.rect)),
        RibbonEvent::Handled
    );
    assert_eq!(menu_labels(&r), ["Font", "Styles"]);
    assert_eq!(
        choose(&mut r, &["Font", "Bold"]),
        RibbonEvent::Toggled { id: BOLD, on: true }
    );

    press(&mut r, &l, centre(overflow.rect));
    assert_eq!(
        choose(&mut r, &["Font", "Font", "Mono"]),
        RibbonEvent::Chose { id: FONT, index: 2 }
    );
    press(&mut r, &l, centre(overflow.rect));
    assert_eq!(
        choose(&mut r, &["Styles", "Styles", "Title"]),
        RibbonEvent::Chose {
            id: STYLES,
            index: 1
        }
    );

    // As narrow as a ribbon can usefully be: room for » and nothing else.
    let l = layout_at(&r, OVERFLOW_WIDTH);
    assert!(l.body.as_ref().unwrap().groups.is_empty());
    let overflow = l.body.as_ref().unwrap().overflow.clone().unwrap();
    assert_eq!(overflow.groups, [0, 1, 2]);
    press(&mut r, &l, centre(overflow.rect));
    assert_eq!(
        choose(&mut r, &["Clipboard", "Paste", "Paste"]),
        RibbonEvent::Command(PASTE)
    );
    press(&mut r, &l, centre(overflow.rect));
    assert_eq!(
        choose(&mut r, &["Clipboard", "Paste", "Paste special"]),
        RibbonEvent::MenuItem {
            id: PASTE,
            item: PASTE_SPECIAL
        }
    );
    press(&mut r, &l, centre(overflow.rect));
    assert_eq!(
        choose(&mut r, &["Font", "Underline"]),
        RibbonEvent::Handled,
        "a disabled command ran from the menu"
    );
}

/// **A ribbon narrower than `»` keeps it inside its own width**: a button
/// past the ribbon's edge is one the window may not show.
#[test]
fn nothing_is_laid_out_past_the_ribbons_edge() {
    for width in [0.0, 10.0, OVERFLOW_WIDTH - 1.0] {
        let l = layout_at(&ribbon(), width);
        let body = l.body.unwrap();
        let overflow = body.overflow.unwrap();
        assert!(body.groups.is_empty());
        assert!(
            overflow.rect.right() <= width,
            "» past the edge at {width}: {:?}",
            overflow.rect
        );
    }
}

// ---- minimized ----

/// **A double click on a tab minimizes the ribbon -- the tabs alone -- and a
/// tab pressed then shows its commands over the page until one is used, the
/// tab is pressed again, or a press lands elsewhere; Ctrl+F1 brings the body
/// back.**
#[test]
fn a_minimized_ribbon_shows_a_tab_over_the_page() {
    let mut r = ribbon();
    let l = wide(&r);
    let home = centre(tab_slot(&l, &r, "home"));
    let double = mouse(
        home.0,
        home.1,
        MouseEventKind::DoubleClick(MouseButton::Left),
    );
    assert_eq!(r.handle_mouse(&l, &double), RibbonEvent::Customized);
    assert!(r.minimized());
    let l = wide(&r);
    assert_eq!(l.rect.h, STRIP_HEIGHT);
    assert!(l.body.is_none());
    assert!(l.panel.is_none());

    assert_eq!(press(&mut r, &l, home), RibbonEvent::Handled);
    let l = wide(&r);
    let panel = l
        .panel
        .as_ref()
        .expect("the tab did not open over the page");
    assert_eq!(panel.rect.y, STRIP_HEIGHT);
    let cut = centre(slot(&r, &l, CUT).rect);
    assert_eq!(click(&mut r, &l, cut), RibbonEvent::Command(CUT));
    assert!(
        !r.panel_open(),
        "the tab stayed over the page after a command"
    );

    press(&mut r, &l, home);
    let l = wide(&r);
    assert_eq!(press(&mut r, &l, home), RibbonEvent::Handled);
    assert!(!r.panel_open(), "pressing the tab again left it open");

    // Another tab: in front, and over the page.
    let view = centre(tab_slot(&l, &r, "view"));
    assert_eq!(
        press(&mut r, &l, view),
        RibbonEvent::TabSelected("view".into())
    );
    assert!(r.panel_open());
    let l = wide(&r);
    assert_eq!(press(&mut r, &l, (10.0, 600.0)), RibbonEvent::Handled);
    assert!(!r.panel_open(), "a press on the page left the tab open");

    assert_eq!(r.handle_key(&key(Key::F1, true)), RibbonEvent::Customized);
    assert!(!r.minimized());
    assert_eq!(
        r.handle_key(&key(Key::F1, false)),
        RibbonEvent::Ignored,
        "F1 alone minimized"
    );
}

/// **A press past the ribbon, with nothing open, is not the ribbon's.**
#[test]
fn a_press_past_the_ribbon_is_not_its() {
    let mut r = ribbon();
    let l = wide(&r);
    assert_eq!(press(&mut r, &l, (10.0, 600.0)), RibbonEvent::Ignored);
    assert_eq!(
        r.handle_mouse(&l, &mouse(10.0, 600.0, MouseEventKind::Move)),
        RibbonEvent::Ignored
    );
    assert_eq!(r.hover(), None);
}

// ---- geometry ----

/// **A large control or a gallery takes a column to itself; the others stack
/// three to a column, in order, a short column centred.**
#[test]
fn columns_stack_three_rows_and_a_large_control_takes_one() {
    let r = ribbon();
    let l = wide(&r);
    let paste = slot(&r, &l, PASTE).rect;
    let cut = slot(&r, &l, CUT).rect;
    let copy = slot(&r, &l, COPY).rect;
    assert_eq!(paste.h, CONTENT_HEIGHT);
    assert!(cut.x > paste.right(), "Cut is not in the next column");
    assert_eq!(cut.x, copy.x);
    assert_eq!(copy.y, cut.bottom());
    assert_eq!(
        cut.y,
        BODY_TOP + CONTENT_TOP + ROW_HEIGHT / 2.0,
        "two rows are not centred"
    );

    let font = slot(&r, &l, FONT).rect;
    let bold = slot(&r, &l, BOLD).rect;
    let italic = slot(&r, &l, ITALIC).rect;
    let underline = slot(&r, &l, UNDERLINE).rect;
    assert_eq!((font.x, bold.x, italic.x), (font.x, font.x, font.x));
    assert_eq!(font.y, BODY_TOP + CONTENT_TOP);
    assert!(
        underline.x > font.right(),
        "a fourth row did not start a column"
    );
    assert_eq!(underline.y, BODY_TOP + CONTENT_TOP + ROW_HEIGHT);
}

/// Where the body starts in a ribbon laid out at the top of the window.
const BODY_TOP: f32 = STRIP_HEIGHT;

/// **A large button's long name goes onto two lines, broken where the longer
/// line is shortest; a short one, or one with no space, stays on one.**
#[test]
fn large_names_wrap_where_the_longer_line_is_shortest() {
    assert_eq!(large_lines("Crop"), ["Crop"]);
    assert_eq!(
        large_lines("Supercalifragilistic"),
        ["Supercalifragilistic"]
    );
    assert_eq!(
        large_lines("Insert a table here"),
        ["Insert a", "table here"]
    );
}

/// **A gallery shows its first choices, or the run that ends with the one
/// chosen.**
#[test]
fn a_gallery_shows_the_run_that_ends_with_the_one_chosen() {
    assert_eq!(gallery_window(5, 3, None), 0..3);
    assert_eq!(gallery_window(5, 3, Some(1)), 0..3);
    assert_eq!(gallery_window(5, 3, Some(4)), 2..5);
    assert_eq!(gallery_window(2, 3, None), 0..2);
    assert_eq!(gallery_window(5, 0, None), 0..1);
    assert_eq!(gallery_window(0, 3, None), 0..0);
    assert_eq!(
        gallery_window(5, 3, Some(9)),
        0..3,
        "a choice past the list moved the window"
    );
}

// ---- drawing ----

fn drawn(r: &Ribbon, l: &RibbonLayout, p: &Palette) -> Vec<RenderCommand> {
    let mut out = Vec::new();
    draw(&mut out, p, r, l, &|_, _| Some(7));
    out
}

fn texts(cmds: &[RenderCommand]) -> Vec<(String, Color)> {
    cmds.iter()
        .filter_map(|c| match c {
            RenderCommand::Text { text, color, .. } => Some((text.clone(), *color)),
            _ => None,
        })
        .collect()
}

fn fills(cmds: &[RenderCommand]) -> Vec<(Rect, Color)> {
    cmds.iter()
        .filter_map(|c| match c {
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                color,
                ..
            } => Some((Rect::new(*x, *y, *width, *height), *color)),
            _ => None,
        })
        .collect()
}

/// **The strip is drawn in the title bar's colour, and the tab in front in
/// the body's, joining it.**
#[test]
fn the_strip_is_the_title_bars_colour_and_the_front_tab_the_bodys() {
    let r = ribbon();
    let l = wide(&r);
    let p = Palette::for_mode(false);
    let fills = fills(&drawn(&r, &l, &p));
    assert_eq!(fills[0], (l.strip, p.surface0));
    let home = tab_slot(&l, &r, "home");
    assert!(
        fills.contains(&(home, p.base)),
        "the front tab is not the body's colour"
    );
    let view = tab_slot(&l, &r, "view");
    assert!(
        !fills.iter().any(|(rect, _)| *rect == view),
        "a tab behind was filled"
    );
}

/// **A contextual tab carries a band of the accent along its top edge, and
/// an ordinary one does not.**
#[test]
fn a_contextual_tab_carries_the_accent_band() {
    let mut r = ribbon();
    r.set_context("picture", true);
    let l = wide(&r);
    let p = Palette::for_mode(true);
    let fills = fills(&drawn(&r, &l, &p));
    let picture = tab_slot(&l, &r, "picture");
    let band = Rect::new(picture.x, picture.y, picture.w, CONTEXT_BAND);
    assert!(
        fills.contains(&(band, p.accent)),
        "no band on the contextual tab"
    );
    let home = tab_slot(&l, &r, "home");
    assert!(!fills.contains(&(Rect::new(home.x, home.y, home.w, CONTEXT_BAND), p.accent)));
}

/// **Every control shown is named where its size says -- a small one not --
/// and the groups too; a disabled one is named faintly; a dropdown with
/// nothing chosen shows its own name as a placeholder.**
#[test]
fn controls_and_groups_are_named() {
    let r = ribbon();
    let l = wide(&r);
    let p = Palette::for_mode(false);
    let texts = texts(&drawn(&r, &l, &p));
    let colour_of = |name: &str| {
        texts
            .iter()
            .find(|(t, _)| t == name)
            .map(|(_, c)| *c)
            .unwrap_or_else(|| panic!("{name:?} is not drawn: {texts:?}"))
    };
    assert_eq!(colour_of("Paste"), p.text);
    assert_eq!(colour_of("Cut"), p.text);
    assert_eq!(
        colour_of("Underline"),
        p.overlay0,
        "a disabled control is not faint"
    );
    assert_eq!(colour_of("Clipboard"), p.subtext0);
    assert_eq!(colour_of("Styles"), p.subtext0);
    assert_eq!(colour_of("Heading 1"), p.text);
    assert!(
        !texts.iter().any(|(t, _)| t == "Bold"),
        "a small button was named"
    );
    let fonts: Vec<&Color> = texts
        .iter()
        .filter(|(t, _)| t == "Font")
        .map(|(_, c)| c)
        .collect();
    assert!(
        fonts.contains(&&p.subtext0),
        "the dropdown's placeholder is not faint: {fonts:?}"
    );
}

/// **Pictures are asked for at their sizes -- large 32, medium and small 16,
/// a gallery's 32 -- and a disabled control's is washed out.**
#[test]
fn pictures_are_asked_for_at_their_sizes() {
    let r = ribbon();
    let l = wide(&r);
    let p = Palette::for_mode(false);
    let asked = std::cell::RefCell::new(Vec::new());
    let mut out = Vec::new();
    draw(&mut out, &p, &r, &l, &|name, size| {
        asked.borrow_mut().push((name.to_owned(), size));
        Some(u64::from(size))
    });
    let asked = asked.into_inner();
    for (name, size) in [
        ("edit-paste", LARGE_ICON),
        ("edit-cut", SMALL_ICON),
        ("format-text-bold", SMALL_ICON),
        ("style-normal", LARGE_ICON),
    ] {
        assert!(
            asked.contains(&(name.to_owned(), size)),
            "{name} was not asked for at {size}: {asked:?}"
        );
    }
    let underline = slot(&r, &l, UNDERLINE).face;
    let wash = Color::rgba(p.base.r, p.base.g, p.base.b, 160);
    assert!(
        fills(&out)
            .iter()
            .any(|(rect, c)| *c == wash && underline.contains(rect.x + 1.0, rect.y + 1.0)),
        "the disabled picture is not washed out"
    );
}

/// **A toggle that is on is drawn chosen.**
#[test]
fn a_toggle_that_is_on_is_drawn_chosen() {
    let mut r = ribbon();
    let p = Palette::for_mode(false);
    let l = wide(&r);
    let bold = slot(&r, &l, BOLD).face;
    assert!(!fills(&drawn(&r, &l, &p)).contains(&(bold, p.selection_fill())));
    r.set_on(BOLD, true);
    assert!(fills(&drawn(&r, &l, &p)).contains(&(bold, p.selection_fill())));
}

/// **An open menu is drawn over everything, last.**
#[test]
fn an_open_menu_is_drawn_last() {
    let mut r = ribbon();
    let l = wide(&r);
    let font = centre(slot(&r, &l, FONT).rect);
    press(&mut r, &l, font);
    let p = Palette::for_mode(false);
    let texts = texts(&drawn(&r, &l, &p));
    let last_ribbon = texts.iter().rposition(|(t, _)| t == "Styles").unwrap();
    let first_row = texts.iter().position(|(t, _)| t == "Serif").unwrap();
    assert!(
        first_row > last_ribbon,
        "the menu is drawn under the ribbon"
    );
}

/// **The pointer over a control lights it; over nothing of the ribbon's, the
/// light goes.**
#[test]
fn the_pointer_lights_what_it_is_over() {
    let mut r = ribbon();
    let l = wide(&r);
    let p = Palette::for_mode(false);
    let cut = slot(&r, &l, CUT).face;
    let (x, y) = centre(cut);
    assert_eq!(
        r.handle_mouse(&l, &mouse(x, y, MouseEventKind::Move)),
        RibbonEvent::Handled
    );
    assert!(fills(&drawn(&r, &l, &p)).contains(&(cut, p.surface1)));
    r.handle_mouse(&l, &mouse(10.0, 600.0, MouseEventKind::Move));
    assert!(!fills(&drawn(&r, &l, &p)).contains(&(cut, p.surface1)));

    // A disabled control is not lit.
    let underline = slot(&r, &l, UNDERLINE).face;
    let (x, y) = centre(underline);
    r.handle_mouse(&l, &mouse(x, y, MouseEventKind::Move));
    assert!(!fills(&drawn(&r, &l, &p)).contains(&(underline, p.surface1)));
}
