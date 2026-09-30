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
    let act = open.acts[usize::try_from(id).unwrap()].clone();
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
    assert!(
        r.menu.as_ref().unwrap().menu.width() >= font.rect.w,
        "the list is narrower than the box it drops from"
    );
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
    assert_eq!(
        r.handle_key(&l, &key(Key::Escape, false)),
        RibbonEvent::Handled
    );
    assert!(!r.menu_open());

    press(&mut r, &l, font);
    assert_eq!(
        r.handle_key(&l, &key(Key::Down, false)),
        RibbonEvent::Handled
    );
    let chosen = r.handle_key(&l, &key(Key::Enter, false));
    assert!(
        matches!(chosen, RibbonEvent::Chose { id: FONT, .. }),
        "Enter chose nothing: {chosen:?}"
    );
    assert_eq!(
        r.handle_key(&l, &key(Key::Escape, false)),
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
    assert_eq!(
        r.handle_key(&l, &key(Key::Escape, false)),
        RibbonEvent::Handled
    );
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

/// **Behind `»` goes everything from the first group that does not fit --
/// not a narrow group further on that happens to fit in the gap**, which
/// would put the groups out of their order.
#[test]
fn nothing_after_a_group_that_does_not_fit_is_shown() {
    let group = |id: &str, label: &str, n: u64| {
        Group::new(id, label).with(Control::button(Command::new(n, "x"), ButtonSize::Small))
    };
    let r = Ribbon::new(vec![
        RibbonTab::new("t", "T")
            .with(group("a", "A", 1))
            .with(group("wide", "Supercalifragilistic", 2))
            .with(group("b", "B", 3)),
    ]);
    let (_, small) = widths(&r);
    assert!(
        small[1] > small[0] + 1.0,
        "the fixture's middle group is not the widest"
    );
    // Room for » and the first group, and for the last beside it -- not the
    // middle one.
    let width = OVERFLOW_WIDTH + small[0] + small[2];
    let l = layout_at(&r, width);
    let body = l.body.as_ref().unwrap();
    assert_eq!(body.groups.iter().map(|g| g.group).collect::<Vec<_>>(), [0]);
    assert_eq!(body.overflow.as_ref().unwrap().groups, [1, 2]);
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

    assert_eq!(
        r.handle_key(&l, &key(Key::F1, true)),
        RibbonEvent::Customized
    );
    assert!(!r.minimized());
    assert_eq!(
        r.handle_key(&l, &key(Key::F1, false)),
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

    // A gallery takes a column to itself, the body's full height.
    let styles = slot(&r, &l, STYLES).rect;
    assert_eq!(styles.h, CONTENT_HEIGHT, "the gallery was put in a row");
    assert_eq!(styles.y, BODY_TOP + CONTENT_TOP);
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
    // The first choice past the ones shown at the start: the window moves.
    assert_eq!(gallery_window(5, 3, Some(3)), 1..4);
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

/// **With accented title bars the strip is the accent, joined to the bar
/// above it: the tabs behind are written in what reads on it, and a
/// contextual tab's band, which would vanish into it, in that ink too.**
#[test]
fn the_strip_follows_an_accented_title_bar() {
    let mut r = ribbon();
    r.set_context("picture", true);
    r.pin(CUT);
    let l = wide(&r);
    let mut p = Palette::for_mode(false);
    p.accent_titlebars = true;
    assert_eq!(p.title_bar(), p.accent);
    let cmds = drawn(&r, &l, &p);
    let fills = fills(&cmds);
    assert!(
        fills.contains(&(l.strip, p.accent)),
        "the strip is not the accent"
    );
    assert!(
        fills.contains(&(l.qat.as_ref().unwrap().rect, p.accent)),
        "the toolbar over the strip is not the accent"
    );
    let texts = texts(&cmds);
    assert!(
        texts.contains(&("View".to_owned(), p.on_accent())),
        "a tab behind is not written in what reads on the accent: {texts:?}"
    );
    assert!(
        texts.contains(&("Home".to_owned(), p.text)),
        "the front tab left the body's ink"
    );
    let picture = tab_slot(&l, &r, "picture");
    let band = Rect::new(picture.x, picture.y, picture.w, CONTEXT_BAND);
    assert!(
        fills.contains(&(band, p.on_accent())),
        "the band vanished into the strip"
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

// ---- key tips ----

/// A letter or digit typed.
fn typed(c: char) -> KeyEvent {
    KeyEvent {
        key: Key::A,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: c.to_string(),
    }
}

/// The key tips of `l`, keys to what they reach.
fn tips_of(r: &Ribbon, l: &RibbonLayout) -> Vec<(String, TipTarget)> {
    r.key_tips(l)
        .into_iter()
        .map(|t| (t.keys, t.target))
        .collect()
}

fn control_tip(r: &Ribbon, l: &RibbonLayout, id: CommandId) -> String {
    let at = slot(r, l, id);
    r.key_tips(l)
        .into_iter()
        .find(|t| {
            matches!(t.target, TipTarget::Control { control, .. } if control == at.control)
                && t.rect == at.rect
        })
        .map(|t| t.keys)
        .unwrap_or_else(|| panic!("command {id} has no tip"))
}

/// **F10 shows a digit on each tab and a letter on each command of the tab
/// in front -- a word's first letter where it is free.**
#[test]
fn f10_shows_a_digit_on_each_tab_and_a_letter_on_each_command() {
    let mut r = ribbon();
    let l = wide(&r);
    assert!(!r.tips_shown());
    assert_eq!(
        r.handle_key(&l, &key(Key::F10, false)),
        RibbonEvent::Handled
    );
    assert!(r.tips_shown());
    let tips = tips_of(&r, &l);
    assert!(tips.contains(&("1".into(), TipTarget::Tab(0))));
    assert!(tips.contains(&("2".into(), TipTarget::Tab(1))));
    for (id, keys) in [
        (PASTE, "p"),
        (CUT, "c"),
        (COPY, "o"),
        (FONT, "f"),
        (BOLD, "b"),
        (ITALIC, "i"),
        (UNDERLINE, "u"),
        (STYLES, "s"),
    ] {
        assert_eq!(control_tip(&r, &l, id), keys, "command {id}");
    }
    let mut r = ribbon();
    assert_eq!(
        r.handle_key(&l, &key(Key::F10, true)),
        RibbonEvent::Ignored,
        "Ctrl+F10 was taken for the tips"
    );
}

/// **A command's letter does it, and ends the tips.**
#[test]
fn a_commands_letter_does_it_and_ends_the_tips() {
    let mut r = ribbon();
    let l = wide(&r);
    r.handle_key(&l, &key(Key::F10, false));
    assert_eq!(r.handle_key(&l, &typed('c')), RibbonEvent::Command(CUT));
    assert!(!r.tips_shown());
    r.handle_key(&l, &key(Key::F10, false));
    assert_eq!(
        r.handle_key(&l, &typed('B')),
        RibbonEvent::Toggled { id: BOLD, on: true },
        "a capital was not taken"
    );
}

/// **A tab's digit brings it to the front, and the letters follow it.**
#[test]
fn a_tabs_digit_brings_it_forward_and_the_letters_follow() {
    let mut r = ribbon();
    let l = wide(&r);
    r.handle_key(&l, &key(Key::F10, false));
    assert_eq!(
        r.handle_key(&l, &typed('2')),
        RibbonEvent::TabSelected("view".into())
    );
    assert!(r.tips_shown(), "the tips went with the tab");
    let l = wide(&r);
    assert_eq!(control_tip(&r, &l, ZOOM), "z");
    assert_eq!(r.handle_key(&l, &typed('z')), RibbonEvent::Command(ZOOM));
}

/// **A dropdown's letter opens its list with the chosen row lit, for the
/// arrows and Enter.**
#[test]
fn a_dropdowns_letter_opens_its_list_for_the_arrows() {
    let mut r = ribbon();
    let l = wide(&r);
    r.handle_key(&l, &key(Key::F10, false));
    assert_eq!(r.handle_key(&l, &typed('f')), RibbonEvent::Handled);
    assert!(r.menu_open());
    assert!(!r.tips_shown());
    assert_eq!(
        r.handle_key(&l, &key(Key::Enter, false)),
        RibbonEvent::Chose { id: FONT, index: 0 }
    );

    r.set_selected(FONT, Some(2));
    r.handle_key(&l, &key(Key::F10, false));
    r.handle_key(&l, &typed('f'));
    assert_eq!(
        r.handle_key(&l, &key(Key::Enter, false)),
        RibbonEvent::Chose { id: FONT, index: 2 },
        "the chosen row was not the lit one"
    );
}

/// **A split button's letter offers its face first, then its own rows.**
#[test]
fn a_split_buttons_letter_offers_its_face_and_its_rows() {
    let mut r = ribbon();
    let l = wide(&r);
    r.handle_key(&l, &key(Key::F10, false));
    r.handle_key(&l, &typed('p'));
    assert_eq!(menu_labels(&r), ["Paste", "Paste special", "Paste as text"]);
    assert_eq!(
        r.handle_key(&l, &key(Key::Enter, false)),
        RibbonEvent::Command(PASTE)
    );

    r.handle_key(&l, &key(Key::F10, false));
    r.handle_key(&l, &typed('p'));
    assert_eq!(
        choose(&mut r, &["Paste as text"]),
        RibbonEvent::MenuItem {
            id: PASTE,
            item: PASTE_TEXT
        }
    );
}

/// **Escape steps back: out of a folded group's panel, then out of the tips.
/// A folded group's letter opens it and the letters move onto it.**
#[test]
fn escape_steps_back_out_of_a_panel_then_the_tips() {
    let mut r = ribbon();
    let (whole, small) = widths(&r);
    let width = whole[0] + whole[1] + small[2];
    let l = layout_at(&r, width);
    r.handle_key(&l, &key(Key::F10, false));
    assert!(tips_of(&r, &l).contains(&("s".into(), TipTarget::Folded(2))));
    assert_eq!(r.handle_key(&l, &typed('s')), RibbonEvent::Handled);
    assert!(r.panel_open());
    assert!(r.tips_shown());
    let l = layout_at(&r, width);
    // The panel's gallery is lettered now, and nothing under the panel is.
    let tips = tips_of(&r, &l);
    assert!(
        tips.iter()
            .any(|(k, t)| k == "s" && matches!(t, TipTarget::Control { group: 2, .. })),
        "{tips:?}"
    );
    assert!(
        !tips.iter().any(|(k, _)| k == "c"),
        "Cut, under the panel, kept a tip"
    );

    assert_eq!(
        r.handle_key(&l, &key(Key::Escape, false)),
        RibbonEvent::Handled
    );
    assert!(!r.panel_open());
    assert!(r.tips_shown(), "Escape out of the panel left the tips too");
    let l = layout_at(&r, width);
    assert_eq!(
        r.handle_key(&l, &key(Key::Escape, false)),
        RibbonEvent::Handled
    );
    assert!(!r.tips_shown());
}

/// **Any key that is not a tip's leaves the tips and is the application's;
/// so does a press of the pointer.**
#[test]
fn another_key_or_a_press_leaves_the_tips() {
    let mut r = ribbon();
    let l = wide(&r);
    r.handle_key(&l, &key(Key::F10, false));
    assert_eq!(
        r.handle_key(&l, &key(Key::Left, false)),
        RibbonEvent::Ignored
    );
    assert!(!r.tips_shown());

    r.handle_key(&l, &key(Key::F10, false));
    press(&mut r, &l, (10.0, 600.0));
    assert!(!r.tips_shown(), "a press left the tips up");
    r.handle_key(&l, &key(Key::F10, false));
    assert_eq!(
        r.handle_key(&l, &key(Key::F10, false)),
        RibbonEvent::Handled
    );
    assert!(!r.tips_shown(), "F10 again did not leave them");
}

/// **A letter no tip starts with starts the typing again, rather than leave
/// it half done.**
#[test]
fn a_letter_no_tip_starts_with_starts_again() {
    let mut r = ribbon();
    let l = wide(&r);
    r.handle_key(&l, &key(Key::F10, false));
    assert_eq!(r.handle_key(&l, &typed('q')), RibbonEvent::Handled);
    assert!(r.tips_shown());
    assert_eq!(r.handle_key(&l, &typed('c')), RibbonEvent::Command(CUT));
}

/// **A second letter that finishes no tip starts the typing over**: what
/// comes next is a tip's first letter, not the rest of one already dropped.
#[test]
fn a_wrong_second_letter_starts_the_typing_over() {
    let mut r = crowded(30);
    let l = layout_at(&r, 4000.0);
    r.handle_key(&l, &key(Key::F10, false));
    // Items 27 on start with `t` (`ta`..`td`), and no tip is `tz`.
    assert_eq!(r.handle_key(&l, &typed('t')), RibbonEvent::Handled);
    assert_eq!(r.handle_key(&l, &typed('z')), RibbonEvent::Handled);
    assert!(r.tips_shown());
    r.handle_key(&l, &typed('i'));
    assert_eq!(
        r.handle_key(&l, &typed('a')),
        RibbonEvent::Command(100),
        "the dropped `t` was kept"
    );
}

/// A tab of `n` small buttons, named `Item 1` onwards, numbered from 100.
fn crowded(n: u64) -> Ribbon {
    let mut group = Group::new("many", "Many");
    for i in 0..n {
        group = group.with(Control::button(
            Command::new(100 + i, format!("Item {}", i + 1)),
            ButtonSize::Small,
        ));
    }
    Ribbon::new(vec![RibbonTab::new("crowd", "Crowd").with(group)])
}

/// **Past twenty-six things every tip is two letters, all different, and a
/// first letter waits for the second.**
#[test]
fn past_twenty_six_every_tip_is_two_letters() {
    let mut r = crowded(30);
    let l = layout_at(&r, 4000.0);
    r.handle_key(&l, &key(Key::F10, false));
    let letters: Vec<String> = r
        .key_tips(&l)
        .into_iter()
        .filter(|t| matches!(t.target, TipTarget::Control { .. }))
        .map(|t| t.keys)
        .collect();
    assert_eq!(letters.len(), 30);
    assert!(letters.iter().all(|k| k.len() == 2), "{letters:?}");
    let unique: BTreeSet<&String> = letters.iter().collect();
    assert_eq!(unique.len(), 30, "two things share a tip: {letters:?}");
    let last = letters[29].clone();
    let mut chars = last.chars();
    assert_eq!(
        r.handle_key(&l, &typed(chars.next().unwrap())),
        RibbonEvent::Handled
    );
    assert!(r.tips_shown());
    assert_eq!(
        r.handle_key(&l, &typed(chars.next().unwrap())),
        RibbonEvent::Command(129)
    );

    // Twenty-six exactly still fit one letter each.
    let r = crowded(26);
    let l = layout_at(&r, 4000.0);
    assert!(r.key_tips(&l).iter().all(|t| t.keys.len() == 1));
}

/// **Tips take a word's first letter where it is free, then another of its
/// letters, then any.**
#[test]
fn tips_take_a_first_letter_where_it_is_free() {
    assert_eq!(tip_letters(&["Cut", "Copy", "Cat"]), ["c", "o", "a"]);
    assert_eq!(
        tip_letters(&["Page setup", "Paste", "Print"]),
        ["p", "a", "r"]
    );
    assert_eq!(
        tip_letters(&["", "123"]),
        ["a", "b"],
        "a name with no letters got none"
    );
    assert_eq!(tip_letters(&[]), Vec::<String>::new());
}

/// **Minimized, the digits open a tab over the page, and its commands take
/// letters there.**
#[test]
fn minimized_the_digits_open_a_tab_over_the_page() {
    let mut r = ribbon();
    r.set_minimized(true);
    let l = wide(&r);
    r.handle_key(&l, &key(Key::F10, false));
    assert!(
        tips_of(&r, &l)
            .iter()
            .all(|(_, t)| matches!(t, TipTarget::Tab(_))),
        "a hidden command had a tip"
    );
    r.handle_key(&l, &typed('1'));
    assert!(r.panel_open());
    let l = wide(&r);
    assert_eq!(r.handle_key(&l, &typed('c')), RibbonEvent::Command(CUT));
    assert!(!r.panel_open());
}

/// **The tips are drawn in capitals on the accent; once a first letter is
/// typed, only the tips it leads to.**
#[test]
fn the_tips_are_drawn_in_capitals_on_the_accent() {
    let mut r = ribbon();
    let l = wide(&r);
    let p = Palette::for_mode(false);
    let before = texts(&drawn(&r, &l, &p));
    assert!(!before.iter().any(|(t, _)| t == "C"));
    r.handle_key(&l, &key(Key::F10, false));
    let cmds = drawn(&r, &l, &p);
    let texts_now = texts(&cmds);
    for keys in ["1", "2", "P", "C", "O", "F", "S"] {
        assert!(
            texts_now.contains(&(keys.to_owned(), p.on_accent())),
            "no {keys} tip: {texts_now:?}"
        );
    }
    let cut = slot(&r, &l, CUT).rect;
    assert!(
        fills(&cmds)
            .iter()
            .any(|(rect, c)| *c == p.accent && rect.bottom() == cut.bottom()),
        "no badge at the foot of Cut"
    );

    let mut r = crowded(30);
    let l = layout_at(&r, 4000.0);
    r.handle_key(&l, &key(Key::F10, false));
    let tips = r.key_tips(&l);
    let first = tips
        .iter()
        .find(|t| t.keys.len() == 2)
        .unwrap()
        .keys
        .clone();
    let lead = first.chars().next().unwrap();
    r.handle_key(&l, &typed(lead));
    let shown = texts(&drawn(&r, &l, &p));
    let upper = lead.to_ascii_uppercase();
    assert!(
        shown
            .iter()
            .filter(|(_, c)| *c == p.on_accent())
            .all(|(t, _)| t.starts_with(upper)),
        "a tip the typing no longer leads to was drawn: {shown:?}"
    );
}

// ---- tooltips ----

/// **A control rested on shows its name after a while, and a disabled one
/// says why it cannot be used.**
#[test]
fn a_control_rested_on_says_what_it_is_and_why_it_cannot_be_used() {
    let mut r = ribbon();
    let l = wide(&r);
    let p = Palette::for_mode(false);
    let delay = u64::from(TOOLTIP_DELAY_MS);
    let (x, y) = centre(slot(&r, &l, CUT).face);
    r.handle_mouse(&l, &mouse(x, y, MouseEventKind::Move));
    assert_eq!(
        r.tooltip_due_in(1_000),
        Some(0),
        "a rest not yet timed is not due at once"
    );
    assert!(!r.tick(1_000));
    assert_eq!(r.tooltip_due_in(1_000), Some(delay));
    assert!(!r.tick(1_000 + delay - 1));
    assert!(r.tick(1_000 + delay), "the tooltip did not come up");
    assert!(r.tooltip().is_some());
    assert!(texts(&drawn(&r, &l, &p)).iter().any(|(t, _)| t == "Cut"));
    assert!(!r.tick(5_000), "a tooltip already up asked for a redraw");

    let (x, y) = centre(slot(&r, &l, UNDERLINE).face);
    r.handle_mouse(&l, &mouse(x, y, MouseEventKind::Move));
    assert!(
        r.tooltip().is_none(),
        "the old tooltip stayed over the new control"
    );
    r.tick(6_000);
    assert!(r.tick(6_000 + delay));
    let shown = texts(&drawn(&r, &l, &p));
    assert!(
        shown.iter().any(|(t, _)| t == "Select some text first"),
        "{shown:?}"
    );

    press(&mut r, &l, (x, y));
    assert!(r.tooltip().is_none(), "a press left the tooltip up");
}

/// **A tab, or nothing of the ribbon's, has no tooltip.**
#[test]
fn a_tab_or_nothing_has_no_tooltip() {
    let mut r = ribbon();
    let l = wide(&r);
    let home = centre(tab_slot(&l, &r, "home"));
    r.handle_mouse(&l, &mouse(home.0, home.1, MouseEventKind::Move));
    assert_eq!(r.tooltip_due_in(0), None);
    r.tick(0);
    assert!(!r.tick(10_000));
    r.handle_mouse(&l, &mouse(10.0, 600.0, MouseEventKind::Move));
    assert_eq!(r.tooltip_due_in(0), None);
}

// ---- one command, one state ----

/// **A toggle's state is its command's: pressed on one tab, it is on on
/// every tab that shows it.**
#[test]
fn a_toggles_state_is_its_commands_wherever_it_appears() {
    let mut defined = tabs();
    defined[1].groups[0].controls.push(Control::toggle(
        Command::new(BOLD, "Bold"),
        ButtonSize::Small,
    ));
    let mut r = Ribbon::new(defined);
    let l = wide(&r);
    let bold = centre(slot(&r, &l, BOLD).rect);
    assert_eq!(
        click(&mut r, &l, bold),
        RibbonEvent::Toggled { id: BOLD, on: true }
    );
    let everywhere: Vec<bool> = r
        .tabs()
        .iter()
        .flat_map(|t| &t.groups)
        .flat_map(|g| &g.controls)
        .filter_map(|c| match c {
            Control::Toggle { command, on, .. } if command.id == BOLD => Some(*on),
            _ => None,
        })
        .collect();
    assert_eq!(
        everywhere,
        [true, true],
        "the other tab's Bold was left off"
    );
    r.select_tab("view");
    let l = wide(&r);
    let bold = centre(slot(&r, &l, BOLD).rect);
    assert_eq!(
        click(&mut r, &l, bold),
        RibbonEvent::Toggled {
            id: BOLD,
            on: false
        }
    );
}

// ---- the Quick Access Toolbar ----

/// **The Quick Access Toolbar holds the commands put on it, over the strip,
/// and each does what it does on its tab -- from whichever tab is in
/// front.**
#[test]
fn the_quick_access_toolbar_does_what_its_commands_do() {
    let mut r = ribbon();
    assert!(wide(&r).qat.is_none(), "an empty toolbar took a row");
    assert!(r.pin(CUT));
    assert!(r.pin(BOLD));
    assert!(r.pin(FONT));
    assert!(!r.pin(CUT), "a command went on twice");
    assert!(!r.pin(999), "a command the ribbon has not went on");
    assert_eq!(r.qat(), [CUT, BOLD, FONT]);

    r.select_tab("view");
    let l = wide(&r);
    let qat = l.qat.clone().expect("no toolbar");
    assert_eq!(qat.rect.y, 0.0);
    assert_eq!(l.strip.y, QAT_HEIGHT);
    assert_eq!(l.rect.h, QAT_HEIGHT + STRIP_HEIGHT + BODY_HEIGHT);
    assert_eq!(qat.buttons.len(), 3);
    assert_eq!(
        click(&mut r, &l, centre(qat.buttons[0].1)),
        RibbonEvent::Command(CUT)
    );
    assert_eq!(
        click(&mut r, &l, centre(qat.buttons[1].1)),
        RibbonEvent::Toggled { id: BOLD, on: true }
    );
    assert!(matches!(
        r.control(BOLD),
        Some(Control::Toggle { on: true, .. })
    ));
    assert_eq!(
        press(&mut r, &l, centre(qat.buttons[2].1)),
        RibbonEvent::Handled
    );
    assert_eq!(
        choose(&mut r, &["Mono"]),
        RibbonEvent::Chose { id: FONT, index: 2 }
    );
    assert!(matches!(
        r.control(FONT),
        Some(Control::Dropdown {
            selected: Some(2),
            ..
        })
    ));

    assert!(r.unpin(BOLD));
    assert!(!r.unpin(BOLD));
    assert_eq!(r.qat(), [CUT, FONT]);
}

/// **The toolbar can go under the ribbon -- under the tabs, when it is
/// minimized.**
#[test]
fn the_toolbar_can_go_under_the_ribbon() {
    let mut r = ribbon();
    r.pin(CUT);
    r.set_qat_below(true);
    assert!(r.qat_below());
    let l = wide(&r);
    assert_eq!(l.strip.y, 0.0);
    assert_eq!(
        l.qat.as_ref().unwrap().rect.y,
        l.body.as_ref().unwrap().rect.bottom()
    );
    assert_eq!(l.rect.h, STRIP_HEIGHT + BODY_HEIGHT + QAT_HEIGHT);
    r.set_minimized(true);
    let l = wide(&r);
    assert_eq!(l.qat.as_ref().unwrap().rect.y, l.strip.bottom());
    assert_eq!(l.rect.h, STRIP_HEIGHT + QAT_HEIGHT);
}

/// **The toolbar is drawn in the strip's colour over the ribbon, each
/// button its picture at the small size -- or its name's first letter.**
#[test]
fn the_toolbar_is_drawn_with_pictures_or_initials() {
    let mut r = ribbon();
    r.pin(CUT);
    r.pin(FONT);
    let l = wide(&r);
    let p = Palette::for_mode(false);
    let asked = std::cell::RefCell::new(Vec::new());
    let mut out = Vec::new();
    draw(&mut out, &p, &r, &l, &|name, size| {
        asked.borrow_mut().push((name.to_owned(), size));
        Some(1)
    });
    let qat = l.qat.as_ref().unwrap();
    assert!(fills(&out).contains(&(qat.rect, p.surface0)));
    assert!(
        asked
            .borrow()
            .contains(&("edit-cut".to_owned(), SMALL_ICON))
    );
    assert!(
        texts(&out).iter().any(|(t, _)| t == "F"),
        "the picture-less Font has no initial"
    );
}

// ---- the right-click menu ----

fn right_press(r: &mut Ribbon, l: &RibbonLayout, (x, y): (f32, f32)) -> RibbonEvent {
    r.handle_mouse(l, &mouse(x, y, MouseEventKind::Press(MouseButton::Right)))
}

/// **A right-click on a command offers it for the toolbar and takes it out
/// of its group; the group's menu offers every command it has not.**
#[test]
fn a_right_click_on_a_command_pins_it_or_takes_it_out() {
    let mut r = ribbon();
    let l = wide(&r);
    let cut = centre(slot(&r, &l, CUT).rect);
    assert_eq!(right_press(&mut r, &l, cut), RibbonEvent::Handled);
    assert_eq!(
        menu_labels(&r),
        [
            "Add to Quick Access Toolbar",
            "Remove from this group",
            "Add a command to this group",
            "Show the Quick Access Toolbar under the ribbon",
            "Collapse the ribbon",
        ]
    );
    assert_eq!(
        choose(&mut r, &["Add to Quick Access Toolbar"]),
        RibbonEvent::Customized
    );
    assert_eq!(r.qat(), [CUT]);

    let l = wide(&r);
    let cut = centre(slot(&r, &l, CUT).rect);
    right_press(&mut r, &l, cut);
    assert!(menu_labels(&r).contains(&"Remove from Quick Access Toolbar".to_owned()));
    assert!(
        menu_labels(&r).contains(&"Undo all changes to the ribbon".to_owned()),
        "a changed ribbon offers no way back"
    );
    assert_eq!(
        choose(&mut r, &["Remove from this group"]),
        RibbonEvent::Customized
    );
    let l = wide(&r);
    let clipboard = &r.tabs()[0].groups[0];
    assert!(!clipboard.controls.iter().any(|c| c.command().id == CUT));
    assert_eq!(
        r.qat(),
        [CUT],
        "taking it out of its group took it off the toolbar"
    );

    // Back from the group's own menu, where it was.
    let paste = centre(slot(&r, &l, PASTE).face);
    right_press(&mut r, &l, paste);
    assert_eq!(
        choose(&mut r, &["Add a command to this group", "Cut"]),
        RibbonEvent::Customized
    );
    let ids: Vec<CommandId> = r.tabs()[0].groups[0]
        .controls
        .iter()
        .map(|c| c.command().id)
        .collect();
    assert_eq!(ids, [PASTE, CUT, COPY], "Cut did not go back where it was");
}

/// **A right-click on a toolbar button takes it off; on a tab, hides or
/// moves it; past the ribbon, is not the ribbon's.**
#[test]
fn a_right_click_on_the_toolbar_or_a_tab() {
    let mut r = ribbon();
    r.pin(COPY);
    let l = wide(&r);
    let button = centre(l.qat.as_ref().unwrap().buttons[0].1);
    right_press(&mut r, &l, button);
    assert_eq!(menu_labels(&r)[0], "Remove from Quick Access Toolbar");
    assert_eq!(
        choose(&mut r, &["Remove from Quick Access Toolbar"]),
        RibbonEvent::Customized
    );
    assert!(r.qat().is_empty());

    let l = wide(&r);
    let home = centre(tab_slot(&l, &r, "home"));
    right_press(&mut r, &l, home);
    let items = r.menu.as_ref().unwrap().menu.items().to_vec();
    let enabled = |label: &str| {
        items.iter().find_map(|i| match i {
            MenuItem::Action {
                label: l, enabled, ..
            } if l == label => Some(*enabled),
            _ => None,
        })
    };
    assert_eq!(enabled("Hide this tab"), Some(true));
    assert_eq!(
        enabled("Move left"),
        Some(false),
        "the first tab can move left"
    );
    assert_eq!(enabled("Move right"), Some(true));
    assert_eq!(choose(&mut r, &["Move right"]), RibbonEvent::Customized);
    let order: Vec<&str> = r
        .visible_tabs()
        .iter()
        .map(|&i| r.tabs()[i].id.as_str())
        .collect();
    assert_eq!(order, ["view", "home"]);
    assert_eq!(
        r.front_id(),
        Some("home"),
        "moving a tab changed the tab in front"
    );

    assert_eq!(right_press(&mut r, &l, (10.0, 600.0)), RibbonEvent::Ignored);
    assert!(!r.menu_open());
}

/// **Tabs hide and show again -- the last ordinary one does not hide --
/// and the menu anywhere offers the hidden ones back.**
#[test]
fn tabs_hide_and_show_again() {
    let mut r = ribbon();
    assert!(r.hide_tab("home", true));
    assert_eq!(r.front_id(), Some("view"), "a hidden tab stayed in front");
    assert!(!r.hide_tab("view", true), "the last tab was hidden");
    assert!(!r.hide_tab("nope", true));
    assert_eq!(r.hidden_tabs(), ["home"]);
    let l = wide(&r);
    right_press(&mut r, &l, centre(l.strip));
    assert_eq!(
        choose(&mut r, &["Show a hidden tab", "Home"]),
        RibbonEvent::Customized
    );
    assert!(r.hidden_tabs().is_empty());
    assert_eq!(r.visible_tabs(), [0, 1]);

    // A hidden tab keeps its place in a moved order.
    assert!(r.move_tab("view", true));
    assert!(r.hide_tab("home", true));
    assert!(!r.move_tab("view", false), "moved past a hidden tab");
    assert!(r.hide_tab("home", false));
    let order: Vec<&str> = r
        .visible_tabs()
        .iter()
        .map(|&i| r.tabs()[i].id.as_str())
        .collect();
    assert_eq!(order, ["view", "home"]);
}

/// **A command put in a group comes at its end, a row high -- an offered
/// one as a button -- and taking it out again leaves no trace.**
#[test]
fn a_command_put_in_a_group_comes_at_its_end() {
    let mut r = ribbon();
    assert!(r.add_to_group("home", "font", ZOOM));
    assert!(!r.add_to_group("home", "font", ZOOM), "put in twice");
    assert!(!r.add_to_group("home", "nope", ZOOM));
    assert!(!r.add_to_group("home", "font", 999));
    let font = &r.tabs()[0].groups[1];
    assert!(matches!(
        font.controls.last(),
        Some(Control::Button { command, size: ButtonSize::Medium }) if command.id == ZOOM
    ));
    assert!(r.remove_from_group("home", "font", ZOOM));
    assert!(!r.customized(), "taking an added command out left a trace");

    r.offer(Command::new(77, "Word count"));
    assert!(r.catalog().iter().any(|c| c.id == 77));
    assert!(r.add_to_group("home", "clipboard", 77));
    assert!(r.set_enabled(77, false, Some("No document")));
    let added = r.tabs()[0].groups[0].controls.last().unwrap();
    assert_eq!(added.command().id, 77);
    assert!(!added.command().enabled);
    let l = wide(&r);
    let at = centre(slot(&r, &l, 77).rect);
    assert_eq!(
        click(&mut r, &l, at),
        RibbonEvent::Handled,
        "a disabled offered command ran"
    );
}

/// **Undoing every change keeps how the ribbon is looked at: minimized
/// stays minimized.**
#[test]
fn undoing_every_change_keeps_it_minimized() {
    let mut r = ribbon();
    r.pin(CUT);
    r.hide_tab("view", true);
    r.remove_from_group("home", "clipboard", COPY);
    r.set_minimized(true);
    assert!(r.customized());
    r.reset_customization();
    assert!(!r.customized());
    assert!(r.qat().is_empty());
    assert_eq!(r.visible_tabs(), [0, 1]);
    assert!(
        r.tabs()[0].groups[0]
            .controls
            .iter()
            .any(|c| c.command().id == COPY)
    );
    assert!(r.minimized());
}

/// **New tabs from the application keep the user's changes.**
#[test]
fn new_tabs_keep_the_users_changes() {
    let mut r = ribbon();
    r.remove_from_group("home", "clipboard", CUT);
    r.set_tabs(tabs());
    assert!(
        !r.tabs()[0].groups[0]
            .controls
            .iter()
            .any(|c| c.command().id == CUT)
    );
}

// ---- saved as a line of text ----

/// **The user's changes go into one line of text and come back from it
/// whole.**
#[test]
fn the_changes_survive_as_a_line_of_text() {
    let mut r = ribbon();
    assert_eq!(
        r.customization_text(),
        "ribbon1",
        "an unchanged ribbon wrote something"
    );
    r.set_minimized(true);
    r.pin(CUT);
    r.pin(FONT);
    r.set_qat_below(true);
    r.move_tab("view", true);
    r.hide_tab("home", true);
    r.remove_from_group("home", "clipboard", COPY);
    r.add_to_group("home", "font", ZOOM);
    let text = r.customization_text();
    assert_eq!(
        text,
        "ribbon1;min=1;qat=2,10;below=1;order=view,home;hidden=home;\
         removed=home/clipboard/3;added=home/font/30"
    );

    let mut back = ribbon();
    back.apply_customization(&text);
    assert_eq!(back.customization_text(), text);
    assert!(back.minimized());
    assert_eq!(back.qat(), [CUT, FONT]);
    assert!(
        back.tabs()[0].groups[1]
            .controls
            .iter()
            .any(|c| c.command().id == ZOOM)
    );
}

/// **What names a tab, group or command the ribbon no longer has is
/// dropped; text of another shape is ignored whole.**
#[test]
fn text_naming_what_is_gone_is_dropped() {
    let mut r = ribbon();
    r.apply_customization(
        "ribbon1;qat=1,999,1;hidden=nope,view;order=home,picture,nope;\
         removed=home/nope/2,home/clipboard/2,home/clipboard/999,bad;\
         added=home/font/404,home/font/30;mystery=1",
    );
    assert_eq!(
        r.customization_text(),
        "ribbon1;qat=1;order=home;hidden=view;removed=home/clipboard/2;added=home/font/30"
    );

    let mut r = ribbon();
    r.apply_customization("ribbon2;min=1");
    assert!(!r.minimized(), "text of another shape was read");
    r.apply_customization("");
    assert!(!r.customized());
}

/// **A tab whose id is not plain is left out of the text, rather than
/// written in a way that reads back as something else.**
#[test]
fn an_id_that_is_not_plain_is_left_out() {
    let mut defined = tabs();
    defined[1].id = "my view;x=1".to_owned();
    let mut r = Ribbon::new(defined);
    assert!(r.hide_tab("my view;x=1", true));
    assert_eq!(r.customization_text(), "ribbon1");
    // A command taken out of, or put into, one of its groups likewise.
    assert!(r.remove_from_group("my view;x=1", "zoom", ZOOM));
    assert!(r.add_to_group("my view;x=1", "zoom", CUT));
    assert_eq!(r.customization_text(), "ribbon1");
    assert!(plain("a-b_c.1"));
    assert!(!plain(""));
    assert!(!plain("a/b"));
    assert!(!plain("a,b"));
}

// ---- the panel and the strip ----

/// **A press on the strip where there is no tab closes an open panel, and
/// does nothing else.**
#[test]
fn a_press_on_the_empty_strip_closes_a_panel() {
    let mut r = ribbon();
    let (whole, small) = widths(&r);
    let width = whole[0] + whole[1] + small[2];
    let l = layout_at(&r, width);
    press(&mut r, &l, centre(group_slot(&l, 2).button.unwrap()));
    assert!(r.panel_open());
    let l = layout_at(&r, width);
    let empty = (l.strip.right() - 5.0, l.strip.y + l.strip.h / 2.0);
    assert_eq!(press(&mut r, &l, empty), RibbonEvent::Handled);
    assert!(
        !r.panel_open(),
        "a press on the empty strip left the panel open"
    );
}
