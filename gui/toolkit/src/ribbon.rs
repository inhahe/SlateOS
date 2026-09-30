//! Ribbon: a bar of commands in tabs, each tab in named groups, for an
//! application with more commands than a menu bar shows well.
//!
//! `roadmap-detailed.md` → *Ribbon Widget*: "a tabbed command surface
//! (Office-style) for command-dense applications: file explorer, text
//! editor, image editor ... a widget, not a mandatory chrome". This module
//! is that widget, in the toolkit's usual shape -- state and functions over
//! it, every case testable headless, [`crate::dock`] being the model:
//!
//! - **the ribbon** ([`Ribbon`]): tabs ([`RibbonTab`]) of named groups
//!   ([`Group`]) of controls ([`Control`]) -- buttons in three sizes,
//!   toggles, split buttons, dropdowns and galleries;
//! - **contextual tabs**: a tab that belongs to a context -- "a picture is
//!   selected" -- is shown only while the application says that context is
//!   active ([`Ribbon::set_context`]);
//! - **geometry** ([`Ribbon::layout`]): where the tabs, groups and controls
//!   are in the width the window gives it. When the groups do not fit, they
//!   fold, one at a time, each into a single button that shows the whole
//!   group in a panel when pressed -- lowest [`Group::priority`] first -- and
//!   what does not fit even folded waits behind a `»` button at the end;
//! - **minimizing** ([`Ribbon::set_minimized`]): the tabs alone, a tab's
//!   commands shown over the page while it is pressed -- a double click on a
//!   tab, or Ctrl+F1;
//! - **input** ([`Ribbon::handle_mouse`], [`Ribbon::handle_key`]): presses
//!   and keys turned into a [`RibbonEvent`] -- a command, a toggle, a row of
//!   a split button's menu, a choice;
//! - **the keyboard** ([`Ribbon::key_tips`]): F10 puts a digit on each tab
//!   and a letter on each command, and typing one reaches it;
//! - **tooltips** ([`Ribbon::tick`]): a control's name after the pointer
//!   rests on it, and why it cannot be used when it cannot;
//! - **the user's changes**: commands put on a Quick Access Toolbar over or
//!   under the ribbon ([`Ribbon::pin`]), tabs hidden and moved, commands
//!   taken out of groups and put into them -- all from a right-click -- and
//!   kept as one line of text ([`Ribbon::customization_text`],
//!   [`Ribbon::apply_customization`]);
//! - **drawing** ([`draw`]), the strip in the title bar's colour, so that a
//!   ribbon under a title bar reads as one piece of window.
//!
//! The application owns the [`Ribbon`], lays it out each frame at the top of
//! its window, draws its own page in what is left under
//! [`RibbonLayout::rect`], and draws the ribbon last: a folded group's panel
//! and a minimized ribbon's tab lie over the page.
//!
//! # What it deliberately does not copy
//!
//! Microsoft licenses specific arrangements of the ribbon -- the "Office
//! Fluent UI" -- and the roadmap entry is explicit: implement the general
//! tabbed pattern, which is not anyone's, and stop short of those. So, on
//! purpose, and not for want of time:
//!
//! | Office | Here |
//! |---|---|
//! | contextual tabs gathered under a coloured header naming their set, drawn up in the title bar | each contextual tab carries a band of the user's accent along its own top edge; nothing is drawn above the strip |
//! | groups shrink by stages -- large buttons to medium to small -- before they collapse | a group is shown whole, or folded into one button: nothing in between |
//! | a gallery previews a choice on the document while the pointer rests on it | a gallery chooses on a click and never previews |
//! | key tips come in layers: letters on the tabs, then a tab's own letters | F10 shows one layer at once: a digit on each tab and a letter on each command of the tab in front ([`Ribbon::key_tips`]) |
//!
//! # Commands are the application's numbers
//!
//! Every control names a [`CommandId`], which is what an event reports. The
//! same command may appear in more than one place -- Paste on two tabs -- and
//! the setters ([`Ribbon::set_enabled`], [`Ribbon::set_on`],
//! [`Ribbon::set_selected`]) change every place it appears.

use std::collections::BTreeSet;

use crate::color::Color;
use crate::event::{Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use crate::frame::Rect;
use crate::menu::{ContextMenu, MenuAction, MenuItem, MenuItemId, Tooltip};
use crate::palette::Palette;
use crate::render::{FontWeightHint, RenderCommand, TextOverflow};
use crate::style::CornerRadii;
use crate::surface::CommandSink;
use crate::text;

// ============================================================================
// Measures
// ============================================================================

/// The strip the tabs sit in.
pub const STRIP_HEIGHT: f32 = 28.0;

/// The body: three rows of controls, and the groups' names under them.
pub const BODY_HEIGHT: f32 = 96.0;

/// One row of the body: a medium or small button, a dropdown.
pub const ROW_HEIGHT: f32 = 24.0;

/// How many rows stack in the body.
const ROWS: usize = 3;

/// Above the controls, inside the body.
const CONTENT_TOP: f32 = 4.0;

/// The rows together.
const CONTENT_HEIGHT: f32 = ROW_HEIGHT * 3.0;

/// A group's name, under its controls.
const GROUP_LABEL_HEIGHT: f32 = 18.0;

/// Inside a group, on each side of its controls.
const GROUP_PAD: f32 = 6.0;

/// Between two columns of one group.
const COLUMN_GAP: f32 = 2.0;

/// A large button's picture, and a folded group's.
pub const LARGE_ICON: u32 = 32;

/// A medium or small button's picture.
pub const SMALL_ICON: u32 = 16;

/// Inside a medium or small button, on each side.
const BUTTON_PAD: f32 = 4.0;

/// Between a medium button's picture and its name.
const ICON_GAP: f32 = 4.0;

/// Inside a large button, on each side.
const LARGE_PAD: f32 = 5.0;

/// Above a large button's picture.
const LARGE_TOP: f32 = 3.0;

/// A large split button's face: its picture. The rest -- its name and the
/// arrow -- opens its menu.
const LARGE_FACE: f32 = LARGE_TOP + 32.0 + 3.0;

/// The part of a medium or small split button that opens its menu, and a
/// dropdown's arrow.
const ARROW_WIDTH: f32 = 14.0;

/// The narrowest a dropdown is drawn, whatever the application asked.
const MIN_DROPDOWN: f32 = 40.0;

/// A control's name.
const LABEL_SIZE: f32 = 12.0;

/// A group's name.
const GROUP_LABEL_SIZE: f32 = 11.0;

/// A gallery choice's name.
const CHOICE_SIZE: f32 = 10.0;

/// A tab's name.
const TAB_SIZE: f32 = 12.5;

/// On each side of a tab's name.
const TAB_PAD: f32 = 12.0;

/// Between two tabs.
const TAB_GAP: f32 = 2.0;

/// Above the tabs, inside the strip.
const TAB_TOP: f32 = 3.0;

/// Before the first tab.
const STRIP_INDENT: f32 = 6.0;

/// Between the ordinary tabs and the contextual ones.
const CONTEXT_GAP: f32 = 10.0;

/// The accent band along a contextual tab's top edge.
pub const CONTEXT_BAND: f32 = 3.0;

/// A large button whose name is wider than this, and has a space in it,
/// puts its name on two lines.
const LARGE_WRAP: f32 = 56.0;

/// The button holding the groups there is no room for even folded.
const OVERFLOW_WIDTH: f32 = 24.0;

/// A gallery choice's width.
const GALLERY_CELL: f32 = 56.0;

/// The column beside a gallery that lists all its choices.
const GALLERY_MORE: f32 = 16.0;

/// Round a control's corners by this much.
const RADIUS: f32 = 3.0;

/// How long the pointer rests on a control before its tooltip shows.
pub const TOOLTIP_DELAY_MS: u32 = 600;

/// The Quick Access Toolbar's row.
pub const QAT_HEIGHT: f32 = 26.0;

/// Around and between the Quick Access Toolbar's buttons.
const QAT_PAD: f32 = 2.0;

/// A key tip's letters.
const TIP_SIZE: f32 = 11.0;

/// A key tip's height, and the room on each side of its letters.
const TIP_HEIGHT: f32 = 16.0;
const TIP_PAD: f32 = 4.0;

/// The id a submenu row carries in a menu the ribbon builds. Never reported:
/// a submenu row opens its submenu, and only a leaf row is chosen.
const SUBMENU_ROW: MenuItemId = MenuItemId::MAX;

// ============================================================================
// What a ribbon holds
// ============================================================================

/// A command's number: the application's own, the same from one run to the
/// next, and what every event names.
pub type CommandId = u64;

/// How big a button is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonSize {
    /// A column to itself: a 32-pixel picture over its name, on up to two
    /// lines. For the commands the tab is for.
    Large,
    /// A row: a 16-pixel picture with its name beside it.
    Medium,
    /// A row: the picture alone, its name in its tooltip.
    Small,
}

/// What a control is called, what picture it has, and whether it can be used
/// now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// What an event names when the control is used.
    pub id: CommandId,
    /// Its name, drawn beside or under its picture.
    pub label: String,
    /// Its picture: an icon-theme name or an absolute path, found through the
    /// resolver [`draw`] is given, as a menu row's is.
    pub icon: Option<String>,
    /// Whether it can be used now. A control that cannot is drawn faint and
    /// reports nothing.
    pub enabled: bool,
    /// Why it cannot be used, when it cannot: `design.txt` asks that a
    /// disabled control say why, and how to make it usable.
    pub disabled_reason: Option<String>,
    /// More than its name says, for the pointer resting on it.
    pub tooltip: Option<String>,
}

impl Command {
    /// A command numbered `id`, named `label`, usable, with no picture.
    #[must_use]
    pub fn new(id: CommandId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            icon: None,
            enabled: true,
            disabled_reason: None,
            tooltip: None,
        }
    }

    /// With the picture `icon`.
    #[must_use]
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// With `tooltip` for the pointer resting on it.
    #[must_use]
    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Not usable now, for `reason`.
    #[must_use]
    pub fn disabled_because(mut self, reason: impl Into<String>) -> Self {
        self.enabled = false;
        self.disabled_reason = Some(reason.into());
        self
    }
}

/// One of a dropdown's or a gallery's choices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// Its name.
    pub label: String,
    /// Its picture, where it has one.
    pub icon: Option<String>,
}

impl Choice {
    /// A choice named `label`.
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            icon: None,
        }
    }

    /// With the picture `icon`.
    #[must_use]
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }
}

/// A thing in a group that the user presses.
#[derive(Clone, Debug)]
pub enum Control {
    /// Does its command when pressed.
    Button {
        /// What it is.
        command: Command,
        /// How big it is drawn.
        size: ButtonSize,
    },
    /// On or off, flipped when pressed: Bold, Show hidden files.
    Toggle {
        /// What it is.
        command: Command,
        /// How big it is drawn.
        size: ButtonSize,
        /// Whether it is on.
        on: bool,
    },
    /// A button with a menu of variations: its face does its command, its
    /// arrow opens the menu -- Paste, and Paste Special under it.
    Split {
        /// What its face does.
        command: Command,
        /// How big it is drawn.
        size: ButtonSize,
        /// Its menu. A row chosen is reported with its own id, beside the
        /// split button's ([`RibbonEvent::MenuItem`]).
        menu: Vec<MenuItem>,
    },
    /// One of a list, shown in a box a row tall: a font, a zoom level.
    Dropdown {
        /// What it is: its name is shown in the box while nothing is chosen.
        command: Command,
        /// What can be chosen.
        choices: Vec<Choice>,
        /// What is chosen now.
        selected: Option<usize>,
        /// How wide the box is.
        width: f32,
    },
    /// Choices shown as pictures, a few in the group and all of them in a
    /// list: styles, shapes.
    Gallery {
        /// What it is.
        command: Command,
        /// What can be chosen.
        choices: Vec<Choice>,
        /// What is chosen now.
        selected: Option<usize>,
        /// How many choices the group shows at once.
        shown: usize,
    },
}

impl Control {
    /// A button.
    #[must_use]
    pub fn button(command: Command, size: ButtonSize) -> Self {
        Self::Button { command, size }
    }

    /// A toggle, off.
    #[must_use]
    pub fn toggle(command: Command, size: ButtonSize) -> Self {
        Self::Toggle {
            command,
            size,
            on: false,
        }
    }

    /// A split button with `menu` under its arrow.
    #[must_use]
    pub fn split(command: Command, size: ButtonSize, menu: Vec<MenuItem>) -> Self {
        Self::Split {
            command,
            size,
            menu,
        }
    }

    /// A dropdown `width` wide, nothing chosen.
    #[must_use]
    pub fn dropdown(command: Command, choices: Vec<Choice>, width: f32) -> Self {
        Self::Dropdown {
            command,
            choices,
            selected: None,
            width,
        }
    }

    /// A gallery showing `shown` choices at once, nothing chosen.
    #[must_use]
    pub fn gallery(command: Command, choices: Vec<Choice>, shown: usize) -> Self {
        Self::Gallery {
            command,
            choices,
            selected: None,
            shown,
        }
    }

    /// What it is.
    #[must_use]
    pub fn command(&self) -> &Command {
        match self {
            Self::Button { command, .. }
            | Self::Toggle { command, .. }
            | Self::Split { command, .. }
            | Self::Dropdown { command, .. }
            | Self::Gallery { command, .. } => command,
        }
    }

    fn command_mut(&mut self) -> &mut Command {
        match self {
            Self::Button { command, .. }
            | Self::Toggle { command, .. }
            | Self::Split { command, .. }
            | Self::Dropdown { command, .. }
            | Self::Gallery { command, .. } => command,
        }
    }

    /// Its button size, for the three kinds that have one.
    fn size(&self) -> Option<ButtonSize> {
        match self {
            Self::Button { size, .. } | Self::Toggle { size, .. } | Self::Split { size, .. } => {
                Some(*size)
            }
            Self::Dropdown { .. } | Self::Gallery { .. } => None,
        }
    }

    /// Whether it takes a column of the group to itself rather than a row.
    fn takes_column(&self) -> bool {
        matches!(self, Self::Gallery { .. }) || self.size() == Some(ButtonSize::Large)
    }
}

/// A named cluster of controls within a tab: Clipboard, Font, Paragraph.
#[derive(Clone, Debug)]
pub struct Group {
    /// The application's name for it, the same from one run to the next.
    pub id: String,
    /// Its name, under its controls.
    pub label: String,
    /// Its picture, on the button it folds into.
    pub icon: Option<String>,
    /// Which groups fold first when the window is too narrow for them all:
    /// the lowest priority first, and among equals the one furthest right.
    /// The application's call, since only it knows which commands its users
    /// reach for.
    pub priority: u8,
    /// Its controls, in order: a large one or a gallery takes a column to
    /// itself, and the others stack in columns of three rows.
    pub controls: Vec<Control>,
}

impl Group {
    /// A group with no controls yet, priority 0.
    #[must_use]
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            priority: 0,
            controls: Vec::new(),
        }
    }

    /// With the picture `icon`, for when it is folded.
    #[must_use]
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// With priority `priority`: see [`Group::priority`].
    #[must_use]
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    /// With `control` after the ones it has.
    #[must_use]
    pub fn with(mut self, control: Control) -> Self {
        self.controls.push(control);
        self
    }
}

/// A category of commands: Home, View, Tools.
#[derive(Clone, Debug)]
pub struct RibbonTab {
    /// The application's name for it, the same from one run to the next.
    pub id: String,
    /// Its name, on the tab.
    pub label: String,
    /// The context it belongs to, for a tab shown only while that context is
    /// active; `None` for a tab that is always there.
    pub context: Option<String>,
    /// Its groups, left to right.
    pub groups: Vec<Group>,
}

impl RibbonTab {
    /// An ordinary tab with no groups yet.
    #[must_use]
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            context: None,
            groups: Vec::new(),
        }
    }

    /// Shown only while `context` is active.
    #[must_use]
    pub fn contextual(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// With `group` after the ones it has.
    #[must_use]
    pub fn with(mut self, group: Group) -> Self {
        self.groups.push(group);
        self
    }
}

// ============================================================================
// What happened
// ============================================================================

/// What an event on the ribbon came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RibbonEvent {
    /// Not the ribbon's: the pointer is not over it or anything it opened,
    /// or the key means nothing to it. The application's to handle.
    Ignored,
    /// The ribbon's, and it changed at most how the ribbon looks: draw it
    /// again.
    Handled,
    /// A button, or a split button's face, was pressed.
    Command(CommandId),
    /// A toggle was pressed, and is now `on`.
    Toggled {
        /// The toggle's command.
        id: CommandId,
        /// Whether it is on now.
        on: bool,
    },
    /// A row of a split button's menu was chosen.
    MenuItem {
        /// The split button's command.
        id: CommandId,
        /// The row's own id, as the application gave it.
        item: MenuItemId,
    },
    /// A dropdown's or a gallery's choice was made.
    Chose {
        /// The dropdown's or gallery's command.
        id: CommandId,
        /// Which of its choices.
        index: usize,
    },
    /// Another tab was brought to the front.
    TabSelected(String),
    /// The user changed the ribbon in a way worth keeping from one run to the
    /// next -- minimized it, or brought it back.
    Customized,
}

/// Where the pointer is, in the ribbon's terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// A tab, by its place in the tabs the ribbon was given.
    Tab(usize),
    /// A control of the front tab: its group, its place in the group, and
    /// which part of it.
    Control {
        /// The group, by its place in the front tab.
        group: usize,
        /// The control, by its place in the group.
        control: usize,
        /// Which part.
        part: Part,
    },
    /// A group folded into one button.
    Folded(usize),
    /// The button holding the groups there is no room for.
    Overflow,
    /// A Quick Access Toolbar button, by its place in [`Ribbon::qat`].
    Qat(usize),
    /// The ribbon, or a panel it opened, where nothing is.
    Blank,
}

/// What a key tip leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipTarget {
    /// A tab, by its place in the tabs the ribbon was given: brought to the
    /// front, the letters following it.
    Tab(usize),
    /// A control of the front tab, by its group's place and its own: a
    /// button or toggle pressed; a split button's, dropdown's or gallery's
    /// menu opened, ready for the arrow keys.
    Control {
        /// The group, by its place in the front tab.
        group: usize,
        /// The control, by its place in the group.
        control: usize,
    },
    /// A folded group: its panel opened, the letters moving onto it.
    Folded(usize),
    /// The `»` button: its menu opened.
    Overflow,
}

/// A key tip: the keys that reach something, what, and where it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyTip {
    /// The keys to type: a digit for a tab, one or two letters otherwise.
    pub keys: String,
    /// What typing them reaches.
    pub target: TipTarget,
    /// Where the thing reached is.
    pub rect: Rect,
}

/// Which part of a control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The face: what a press does the command of.
    Face,
    /// What opens its menu: a split button's arrow, a dropdown, a gallery's
    /// column of arrows.
    Arrow,
    /// A gallery's choice, by its place in the choices.
    Choice(usize),
}

/// A panel the ribbon has opened over the page.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Panel {
    /// A folded group, whole, under the button it folded into.
    Group {
        /// The group, by its place in the front tab.
        group: usize,
        /// Where the folded group is: the panel hangs under it.
        anchor: Rect,
    },
    /// A minimized ribbon's front tab, where the body would be.
    Tab,
}

/// What a row of a menu the ribbon built does. By command rather than by
/// place, so that a row does the same whether its command is on the front
/// tab, behind `»` or on the Quick Access Toolbar.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Act {
    /// Press a command's face: a button's, a toggle's, a split button's.
    Press(CommandId),
    /// A split button's own menu row.
    SplitRow { id: CommandId, row: MenuItemId },
    /// Choose a dropdown's or a gallery's choice.
    Choose { id: CommandId, index: usize },
    /// Put a command on the Quick Access Toolbar (`true`), or take it off.
    Pin(CommandId, bool),
    /// Put the Quick Access Toolbar under the ribbon (`true`), or over it.
    QatBelow(bool),
    /// Minimize the ribbon (`true`), or bring it back.
    Minimize(bool),
    /// Hide a tab (`true`), or show it again.
    HideTab(String, bool),
    /// Move a tab one place left (`true`), or right.
    MoveTab(String, bool),
    /// Take a command out of a group: tab, group, command.
    Remove(String, String, CommandId),
    /// Put a command into a group, at its end: tab, group, command.
    Add(String, String, CommandId),
    /// Undo every change the user made but minimizing.
    Reset,
}

/// What a press on a part of a control does to its command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Press {
    /// Its command: a button's, a split button's face.
    Face,
    /// A toggle turned to this.
    Toggle(bool),
    /// A choice made.
    Choose(usize),
    /// Nothing: a part that opens a menu, or a choice not there.
    Nothing,
}

/// What pressing `part` of `control` does.
fn press_of(control: &Control, part: Part) -> Press {
    match (control, part) {
        (Control::Button { .. } | Control::Split { .. }, Part::Face) => Press::Face,
        (Control::Toggle { on, .. }, Part::Face) => Press::Toggle(!*on),
        (Control::Gallery { choices, .. }, Part::Choice(i)) if i < choices.len() => {
            Press::Choose(i)
        }
        _ => Press::Nothing,
    }
}

/// A menu the ribbon is building: its rows, and what each does, numbered
/// together so that row `n` does `acts[n]`.
#[derive(Default)]
struct MenuBuilder {
    items: Vec<MenuItem>,
    acts: Vec<Act>,
}

impl MenuBuilder {
    /// A row that does `act`, returned rather than added: a submenu's.
    fn child(
        &mut self,
        label: impl Into<String>,
        icon: Option<String>,
        act: Act,
        enabled: bool,
    ) -> MenuItem {
        let id = self.acts.len() as MenuItemId;
        self.acts.push(act);
        MenuItem::Action {
            id,
            label: label.into(),
            shortcut: None,
            icon,
            enabled,
            checked: None,
        }
    }

    /// A row that does `act`.
    fn row(&mut self, label: &str, act: Act, enabled: bool) {
        let item = self.child(label, None, act, enabled);
        self.items.push(item);
    }

    /// A line between rows -- never first, and never two together.
    fn separator(&mut self) {
        if self
            .items
            .last()
            .is_some_and(|item| !matches!(item, MenuItem::Separator))
        {
            self.items.push(MenuItem::Separator);
        }
    }

    /// A submenu of `children`, which can be opened only when it has some.
    fn submenu(&mut self, label: &str, children: Vec<MenuItem>) {
        self.items.push(MenuItem::Submenu {
            id: SUBMENU_ROW,
            label: label.to_owned(),
            icon: None,
            enabled: !children.is_empty(),
            children,
        });
    }
}

/// A menu the ribbon has open, and what each of its rows does: row `n` is
/// `acts[n]`, whatever the rows are named for the user.
struct OpenMenu {
    menu: ContextMenu,
    acts: Vec<Act>,
}

// ============================================================================
// The ribbon
// ============================================================================

/// What the user has changed about a ribbon: kept from one run to the next
/// as a line of text ([`Ribbon::customization_text`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Customization {
    /// Whether it shows its tabs alone.
    minimized: bool,
    /// The Quick Access Toolbar's commands, left to right.
    qat: Vec<CommandId>,
    /// Whether the Quick Access Toolbar is under the ribbon rather than
    /// over it.
    qat_below: bool,
    /// The ordinary tabs' ids in the user's order; a tab not named here --
    /// one the application added since -- follows in its own place.
    order: Vec<String>,
    /// The ids of the tabs the user hid.
    hidden: BTreeSet<String>,
    /// Commands taken out of a group: tab id, group id, command.
    removed: BTreeSet<(String, String, CommandId)>,
    /// Commands put into a group, at its end: tab id, group id, command.
    added: Vec<(String, String, CommandId)>,
}

/// A ribbon: the application's tabs, the user's changes to them, which tab
/// is in front, which contexts are active, and what the pointer and the
/// keyboard are doing to it.
pub struct Ribbon {
    /// The tabs as the application gave them.
    base: Vec<RibbonTab>,
    /// Commands the application offers for the user to put in a group,
    /// beyond those already on a tab.
    offered: Vec<Command>,
    /// The user's changes.
    custom: Customization,
    /// The tabs shown: `base` with the user's changes to groups made. Built
    /// again whenever either changes ([`rebuild`](Self::rebuild)); what the
    /// layout, input and drawing all read.
    tabs: Vec<RibbonTab>,
    /// The front tab's id. Kept by name rather than place, so that a tab
    /// appearing or going before it leaves the same tab in front.
    front: String,
    contexts: BTreeSet<String>,
    hover: Option<Hit>,
    pressed: Option<Hit>,
    panel: Option<Panel>,
    menu: Option<OpenMenu>,
    /// What has been typed of a key tip, while key tips are shown.
    tips: Option<String>,
    /// The tooltip of what the pointer rests on, once it is being timed.
    tooltip: Option<Tooltip>,
    /// Where the pointer came to rest, and in what window, until the next
    /// [`tick`](Self::tick) starts timing it: a pointer event carries no time.
    resting: Option<(f32, f32, (f32, f32))>,
}

impl Ribbon {
    /// A ribbon of `tabs`, the first ordinary one in front.
    #[must_use]
    pub fn new(tabs: Vec<RibbonTab>) -> Self {
        let mut ribbon = Self {
            base: tabs,
            offered: Vec::new(),
            custom: Customization::default(),
            tabs: Vec::new(),
            front: String::new(),
            contexts: BTreeSet::new(),
            hover: None,
            pressed: None,
            panel: None,
            menu: None,
            tips: None,
            tooltip: None,
            resting: None,
        };
        ribbon.rebuild();
        ribbon
    }

    /// The tabs shown -- the application's, with the user's changes to their
    /// groups made -- which every place in a [`RibbonLayout`] and every
    /// [`Hit`] refers to.
    #[must_use]
    pub fn tabs(&self) -> &[RibbonTab] {
        &self.tabs
    }

    /// Put `tabs` in place of the application's. The user's changes are made
    /// to the new tabs as they were to the old, where they still apply; the
    /// front tab stays in front if it is still there; anything open is
    /// closed, since it named a place in the tabs that were.
    pub fn set_tabs(&mut self, tabs: Vec<RibbonTab>) {
        self.base = tabs;
        self.rebuild();
    }

    /// Offer `command` for the user to put in a group, beyond the commands
    /// already on the tabs: something a user may want to hand and the
    /// application does not show by default.
    pub fn offer(&mut self, command: Command) {
        self.offered.retain(|c| c.id != command.id);
        self.offered.push(command);
    }

    /// Build the tabs shown from the application's and the user's changes.
    fn rebuild(&mut self) {
        let mut tabs = self.base.clone();
        for tab in &mut tabs {
            let tab_id = tab.id.clone();
            for group in &mut tab.groups {
                let group_id = group.id.clone();
                group.controls.retain(|c| {
                    !self.custom.removed.contains(&(
                        tab_id.clone(),
                        group_id.clone(),
                        c.command().id,
                    ))
                });
                for (t, g, id) in &self.custom.added {
                    if *t == tab_id
                        && *g == group_id
                        && !group.controls.iter().any(|c| c.command().id == *id)
                        && let Some(control) = self.catalog_control(*id)
                    {
                        group.controls.push(control);
                    }
                }
            }
        }
        self.tabs = tabs;
        self.close();
        self.hover = None;
        self.keep_front_visible();
    }

    /// The control a command put into a group appears as: its first control
    /// on the application's tabs -- a button a row high, so that it takes a
    /// row and not a column -- or, for an offered command, a button.
    fn catalog_control(&self, id: CommandId) -> Option<Control> {
        let found = self
            .base
            .iter()
            .flat_map(|t| &t.groups)
            .flat_map(|g| &g.controls)
            .find(|c| c.command().id == id);
        match found {
            Some(control) => {
                let mut control = control.clone();
                if let Control::Button { size, .. }
                | Control::Toggle { size, .. }
                | Control::Split { size, .. } = &mut control
                    && *size == ButtonSize::Large
                {
                    *size = ButtonSize::Medium;
                }
                Some(control)
            }
            None => self
                .offered
                .iter()
                .find(|c| c.id == id)
                .map(|command| Control::button(command.clone(), ButtonSize::Medium)),
        }
    }

    /// Every command the user can put in a group: those on the
    /// application's tabs, each once, then those offered.
    #[must_use]
    pub fn catalog(&self) -> Vec<Command> {
        let mut seen = BTreeSet::new();
        self.base
            .iter()
            .flat_map(|t| &t.groups)
            .flat_map(|g| &g.controls)
            .map(|c| c.command().clone())
            .chain(self.offered.iter().cloned())
            .filter(|c| seen.insert(c.id))
            .collect()
    }

    /// The indices of the tabs shown, in the order they are shown: the
    /// ordinary ones in the user's order, then those of the active contexts;
    /// a tab the user hid in neither.
    #[must_use]
    pub fn visible_tabs(&self) -> Vec<usize> {
        let shown = |t: &RibbonTab| !self.custom.hidden.contains(&t.id);
        let mut ordinary: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| t.context.is_none() && shown(t))
            .map(|(i, _)| i)
            .collect();
        ordinary.sort_by_key(|&i| self.order_of(i));
        let contextual = self.tabs.iter().enumerate().filter(|(_, t)| {
            shown(t)
                && t.context
                    .as_ref()
                    .is_some_and(|context| self.contexts.contains(context))
        });
        ordinary
            .into_iter()
            .chain(contextual.map(|(i, _)| i))
            .collect()
    }

    /// Where the tab `i` falls in the user's order: its place there, or --
    /// for a tab the user never moved -- after every one they did. The sort
    /// that reads this is stable, so tabs never moved keep the application's
    /// order among themselves.
    fn order_of(&self, i: usize) -> usize {
        self.tabs
            .get(i)
            .and_then(|t| self.custom.order.iter().position(|id| *id == t.id))
            .unwrap_or(usize::MAX)
    }

    /// The front tab, by its place in [`tabs`](Self::tabs); `None` when no
    /// tab is shown.
    #[must_use]
    pub fn front(&self) -> Option<usize> {
        let visible = self.visible_tabs();
        visible
            .iter()
            .copied()
            .find(|&i| self.tabs.get(i).is_some_and(|t| t.id == self.front))
            .or_else(|| visible.first().copied())
    }

    /// The front tab's id.
    #[must_use]
    pub fn front_id(&self) -> Option<&str> {
        self.front()
            .and_then(|i| self.tabs.get(i))
            .map(|t| t.id.as_str())
    }

    /// Bring the tab `id` to the front, if it is shown. Answers whether it
    /// is in front now.
    pub fn select_tab(&mut self, id: &str) -> bool {
        let shown = self
            .visible_tabs()
            .into_iter()
            .any(|i| self.tabs.get(i).is_some_and(|t| t.id == id));
        if shown && self.front != id {
            self.front = id.to_owned();
            self.close();
        }
        shown
    }

    /// Say whether the context `name` is active: its tabs are shown while it
    /// is. A context ending takes its tab from the front, to the first tab.
    pub fn set_context(&mut self, name: &str, active: bool) {
        let changed = if active {
            self.contexts.insert(name.to_owned())
        } else {
            self.contexts.remove(name)
        };
        if changed {
            self.close();
            self.keep_front_visible();
        }
    }

    /// Whether the context `name` is active.
    #[must_use]
    pub fn context_active(&self, name: &str) -> bool {
        self.contexts.contains(name)
    }

    /// Whether the ribbon shows its tabs alone.
    #[must_use]
    pub fn minimized(&self) -> bool {
        self.custom.minimized
    }

    /// Show the tabs alone, or the tabs and the front tab's commands.
    pub fn set_minimized(&mut self, minimized: bool) {
        self.custom.minimized = minimized;
        self.close();
    }

    /// Enable or disable every control of `id`; a reason says why it cannot
    /// be used. Answers whether any control has that command.
    pub fn set_enabled(&mut self, id: CommandId, enabled: bool, reason: Option<&str>) -> bool {
        let apply = |command: &mut Command| {
            command.enabled = enabled;
            command.disabled_reason = if enabled {
                None
            } else {
                reason.map(str::to_owned)
            };
        };
        let mut any = self.each_control(id, |control| apply(control.command_mut()));
        // An offered command too, so that one put in a group later arrives
        // as it should be.
        for command in self.offered.iter_mut().filter(|c| c.id == id) {
            apply(command);
            any = true;
        }
        any
    }

    /// Turn every toggle of `id` on or off. Answers whether any toggle has
    /// that command.
    pub fn set_on(&mut self, id: CommandId, on: bool) -> bool {
        let mut any = false;
        self.each_control(id, |control| {
            if let Control::Toggle { on: now, .. } = control {
                *now = on;
                any = true;
            }
        });
        any
    }

    /// Choose `index` -- or nothing -- in every dropdown and gallery of `id`.
    /// An index past its choices chooses nothing. Answers whether any
    /// dropdown or gallery has that command.
    pub fn set_selected(&mut self, id: CommandId, index: Option<usize>) -> bool {
        let mut any = false;
        self.each_control(id, |control| {
            if let Control::Dropdown {
                choices, selected, ..
            }
            | Control::Gallery {
                choices, selected, ..
            } = control
            {
                *selected = index.filter(|&i| i < choices.len());
                any = true;
            }
        });
        any
    }

    // ---- the user's changes ----

    /// The Quick Access Toolbar's commands, left to right.
    #[must_use]
    pub fn qat(&self) -> &[CommandId] {
        &self.custom.qat
    }

    /// Put the command `id` at the end of the Quick Access Toolbar. Answers
    /// whether it was put there -- not when it is there already, or is no
    /// command the ribbon has.
    pub fn pin(&mut self, id: CommandId) -> bool {
        if self.custom.qat.contains(&id) || self.command_control(id).is_none() {
            return false;
        }
        self.custom.qat.push(id);
        self.close();
        true
    }

    /// Take the command `id` off the Quick Access Toolbar. Answers whether
    /// it was on it.
    pub fn unpin(&mut self, id: CommandId) -> bool {
        let before = self.custom.qat.len();
        self.custom.qat.retain(|&pinned| pinned != id);
        self.close();
        self.custom.qat.len() != before
    }

    /// Whether the Quick Access Toolbar is under the ribbon rather than over
    /// it.
    #[must_use]
    pub fn qat_below(&self) -> bool {
        self.custom.qat_below
    }

    /// Put the Quick Access Toolbar under the ribbon, or over it.
    pub fn set_qat_below(&mut self, below: bool) {
        self.custom.qat_below = below;
        self.close();
    }

    /// The ids of the tabs the user hid.
    #[must_use]
    pub fn hidden_tabs(&self) -> Vec<&str> {
        self.custom.hidden.iter().map(String::as_str).collect()
    }

    /// Hide the tab `id`, or show it again. Answers whether anything changed.
    /// The last ordinary tab shown is not hidden: a ribbon with none would
    /// have nowhere to show a command.
    pub fn hide_tab(&mut self, id: &str, hide: bool) -> bool {
        if !self.tabs.iter().any(|t| t.id == id) {
            return false;
        }
        let changed = if hide {
            self.can_hide(id) && self.custom.hidden.insert(id.to_owned())
        } else {
            self.custom.hidden.remove(id)
        };
        if changed {
            self.close();
            self.keep_front_visible();
        }
        changed
    }

    /// Move the tab `id` one place left (`true`) or right among the ordinary
    /// tabs shown. Answers whether it moved.
    pub fn move_tab(&mut self, id: &str, left: bool) -> bool {
        let Some((mut ids, at, other)) = self.move_target(id, left) else {
            return false;
        };
        ids.swap(at, other);
        self.custom.order = ids;
        self.close();
        true
    }

    /// Take the command `id` out of the group `group` of the tab `tab`: one
    /// the user put there is simply taken back out. Answers whether anything
    /// changed.
    pub fn remove_from_group(&mut self, tab: &str, group: &str, id: CommandId) -> bool {
        let entry = (tab.to_owned(), group.to_owned(), id);
        let added = self.custom.added.len();
        self.custom.added.retain(|e| *e != entry);
        let changed = if self.custom.added.len() == added {
            self.base_group(tab, group)
                .is_some_and(|g| g.controls.iter().any(|c| c.command().id == id))
                && self.custom.removed.insert(entry)
        } else {
            true
        };
        if changed {
            self.rebuild();
        }
        changed
    }

    /// Put the command `id` at the end of the group `group` of the tab `tab`
    /// -- or, one the user took out of it, back where it was. Answers whether
    /// anything changed.
    pub fn add_to_group(&mut self, tab: &str, group: &str, id: CommandId) -> bool {
        let entry = (tab.to_owned(), group.to_owned(), id);
        let changed = if self.custom.removed.remove(&entry) {
            true
        } else if self.base_group(tab, group).is_none()
            || self.catalog_control(id).is_none()
            || self
                .shown_group(tab, group)
                .is_some_and(|g| g.controls.iter().any(|c| c.command().id == id))
        {
            false
        } else {
            self.custom.added.push(entry);
            true
        };
        if changed {
            self.rebuild();
        }
        changed
    }

    /// Undo every change the user made to the ribbon, but minimizing it,
    /// which is how they are looking at it rather than a change to it.
    pub fn reset_customization(&mut self) {
        self.custom = Customization {
            minimized: self.custom.minimized,
            ..Customization::default()
        };
        self.rebuild();
    }

    /// Whether the user has changed anything that
    /// [`reset_customization`](Self::reset_customization) would undo.
    #[must_use]
    pub fn customized(&self) -> bool {
        self.custom
            != Customization {
                minimized: self.custom.minimized,
                ..Customization::default()
            }
    }

    /// The group `group` of the application's tab `tab`.
    fn base_group(&self, tab: &str, group: &str) -> Option<&Group> {
        self.base
            .iter()
            .find(|t| t.id == tab)?
            .groups
            .iter()
            .find(|g| g.id == group)
    }

    /// The group `group` of the shown tab `tab`.
    fn shown_group(&self, tab: &str, group: &str) -> Option<&Group> {
        self.tabs
            .iter()
            .find(|t| t.id == tab)?
            .groups
            .iter()
            .find(|g| g.id == group)
    }

    /// The user's changes as one line of text, for the application to keep
    /// in its settings file and give back to
    /// [`apply_customization`](Self::apply_customization) when it next
    /// starts: `ribbon1;min=1;qat=3,10;below=1;order=home,view;hidden=view;
    /// removed=home/clipboard/2;added=home/clipboard/40`, each part there only
    /// when it says something.
    ///
    /// Tab and group ids go in as they are, so only plain ones -- letters,
    /// digits, `_`, `-`, `.` -- can be written: a change naming another is
    /// left out, rather than written in a way that reads back as something
    /// else.
    #[must_use]
    pub fn customization_text(&self) -> String {
        let c = &self.custom;
        let entries = |set: &mut dyn Iterator<Item = &(String, String, CommandId)>| {
            set.filter(|(t, g, _)| plain(t) && plain(g))
                .map(|(t, g, id)| format!("{t}/{g}/{id}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        let mut fields = vec![TEXT_VERSION.to_owned()];
        if c.minimized {
            fields.push("min=1".to_owned());
        }
        if !c.qat.is_empty() {
            let ids: Vec<String> = c.qat.iter().map(u64::to_string).collect();
            fields.push(format!("qat={}", ids.join(",")));
        }
        if c.qat_below {
            fields.push("below=1".to_owned());
        }
        let order: Vec<&str> = c
            .order
            .iter()
            .map(String::as_str)
            .filter(|t| plain(t))
            .collect();
        if !order.is_empty() {
            fields.push(format!("order={}", order.join(",")));
        }
        let hidden: Vec<&str> = c
            .hidden
            .iter()
            .map(String::as_str)
            .filter(|t| plain(t))
            .collect();
        if !hidden.is_empty() {
            fields.push(format!("hidden={}", hidden.join(",")));
        }
        let removed = entries(&mut c.removed.iter());
        if !removed.is_empty() {
            fields.push(format!("removed={removed}"));
        }
        let added = entries(&mut c.added.iter());
        if !added.is_empty() {
            fields.push(format!("added={added}"));
        }
        fields.join(";")
    }

    /// Make the changes [`customization_text`](Self::customization_text)
    /// wrote, in place of any made so far. A change naming a tab, group or
    /// command the ribbon no longer has is dropped, so that the next save is
    /// clean; text of another shape is ignored whole, leaving the ribbon as
    /// the application made it.
    pub fn apply_customization(&mut self, text: &str) {
        let mut fields = text.trim().split(';');
        if fields.next() != Some(TEXT_VERSION) {
            return;
        }
        let mut c = Customization::default();
        for field in fields {
            let Some((key, value)) = field.split_once('=') else {
                continue;
            };
            let list = || value.split(',').filter(|v| !v.is_empty());
            match key {
                "min" => c.minimized = value == "1",
                "below" => c.qat_below = value == "1",
                "qat" => {
                    for id in list().filter_map(|v| v.parse::<CommandId>().ok()) {
                        if !c.qat.contains(&id) && self.catalog_control(id).is_some() {
                            c.qat.push(id);
                        }
                    }
                }
                "order" => {
                    c.order = list()
                        .filter(|t| self.base.iter().any(|b| b.id == *t && b.context.is_none()))
                        .map(str::to_owned)
                        .collect();
                }
                "hidden" => {
                    c.hidden = list()
                        .filter(|t| self.base.iter().any(|b| b.id == *t))
                        .map(str::to_owned)
                        .collect();
                }
                "removed" => {
                    c.removed = list()
                        .filter_map(entry_of)
                        .filter(|(t, g, id)| {
                            self.base_group(t, g)
                                .is_some_and(|g| g.controls.iter().any(|c| c.command().id == *id))
                        })
                        .collect();
                }
                "added" => {
                    c.added = list()
                        .filter_map(entry_of)
                        .filter(|(t, g, id)| {
                            self.base_group(t, g).is_some() && self.catalog_control(*id).is_some()
                        })
                        .collect();
                }
                _ => {}
            }
        }
        self.custom = c;
        self.rebuild();
    }

    /// The control numbered `id` -- the first, where it appears more than
    /// once.
    #[must_use]
    pub fn control(&self, id: CommandId) -> Option<&Control> {
        self.tabs
            .iter()
            .flat_map(|t| &t.groups)
            .flat_map(|g| &g.controls)
            .find(|c| c.command().id == id)
    }

    /// What the pointer is over, as last seen.
    #[must_use]
    pub fn hover(&self) -> Option<Hit> {
        self.hover
    }

    /// Whether the ribbon has a menu open.
    #[must_use]
    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// Whether the ribbon has a panel open over the page: a folded group, or
    /// a minimized ribbon's tab.
    #[must_use]
    pub fn panel_open(&self) -> bool {
        self.panel.is_some()
    }

    /// Close whatever is open.
    pub fn close(&mut self) {
        self.panel = None;
        self.menu = None;
        self.pressed = None;
        self.tooltip = None;
        self.resting = None;
    }

    /// Keep time: start timing a tooltip for where the pointer came to rest,
    /// and show it once it has rested long enough. Answers whether the ribbon
    /// needs drawing again -- a tooltip came up.
    ///
    /// Call it after every event given to the ribbon, and when
    /// [`tooltip_due_in`](Self::tooltip_due_in) says: a pointer event carries
    /// no time, and a ribbon that asked for a clock of its own would be a
    /// second clock to keep in step with the application's.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        if let Some((x, y, viewport)) = self.resting.take()
            && let Some(text) = self.tooltip_text()
        {
            let mut tip = Tooltip::new(&text).with_delay(TOOLTIP_DELAY_MS);
            tip.start_hover(x, y, now_ms, viewport);
            self.tooltip = Some(tip);
        }
        match self.tooltip.as_mut() {
            Some(tip) if !tip.is_visible() => {
                tip.tick(now_ms);
                tip.is_visible()
            }
            _ => false,
        }
    }

    /// How long until a tooltip comes up, for an application that sleeps
    /// until something is due: none when nothing is waiting to show, and
    /// nothing at all when the pointer has come to rest and not yet been
    /// timed.
    #[must_use]
    pub fn tooltip_due_in(&self, now_ms: u64) -> Option<u64> {
        if self.resting.is_some() {
            return Some(0);
        }
        self.tooltip.as_ref().and_then(|tip| tip.due_in(now_ms))
    }

    /// The tooltip showing now, if one is.
    #[must_use]
    pub fn tooltip(&self) -> Option<&Tooltip> {
        self.tooltip.as_ref().filter(|tip| tip.is_visible())
    }

    /// What the tooltip for what the pointer is over says: a control's name,
    /// then why it cannot be used -- `design.txt` asks every disabled control
    /// to say -- or what more it does; a folded group's name; `»`'s purpose.
    fn tooltip_text(&self) -> Option<String> {
        match self.hover? {
            Hit::Control { group, control, .. } => {
                Some(command_tip(self.control_at(group, control)?.command()))
            }
            Hit::Qat(i) => {
                let id = *self.custom.qat.get(i)?;
                Some(command_tip(self.command_control(id)?.command()))
            }
            Hit::Folded(group) => self.group(group).map(|g| g.label.clone()),
            Hit::Overflow => Some("More commands".to_owned()),
            Hit::Tab(_) | Hit::Blank => None,
        }
    }

    /// Whether key tips are shown.
    #[must_use]
    pub fn tips_shown(&self) -> bool {
        self.tips.is_some()
    }

    /// The key tips for `layout`: a digit on each tab, one to ten, and a
    /// letter -- or two, where there are more than twenty-six -- on each thing
    /// the front tab shows, or an open panel shows: its controls, its folded
    /// groups, `»`.
    ///
    /// One layer, all at once: a digit brings its tab to the front and the
    /// letters follow it. Deliberately not Office's sequence of layers --
    /// letters on the tabs, then a tab's own -- per the roadmap's caution.
    #[must_use]
    pub fn key_tips(&self, layout: &RibbonLayout) -> Vec<KeyTip> {
        let mut tips: Vec<KeyTip> = layout
            .tabs
            .iter()
            .zip("1234567890".chars())
            .map(|(slot, digit)| KeyTip {
                keys: digit.to_string(),
                target: TipTarget::Tab(slot.tab),
                rect: slot.rect,
            })
            .collect();
        let Some(body) = layout.panel.as_ref().or(layout.body.as_ref()) else {
            return tips;
        };
        let mut targets: Vec<(TipTarget, Rect, &str)> = Vec::new();
        for g in &body.groups {
            if let Some(button) = g.button {
                if let Some(group) = self.group(g.group) {
                    targets.push((TipTarget::Folded(g.group), button, &group.label));
                }
                continue;
            }
            for c in &g.controls {
                if let Some(control) = self.control_at(g.group, c.control) {
                    targets.push((
                        TipTarget::Control {
                            group: g.group,
                            control: c.control,
                        },
                        c.rect,
                        &control.command().label,
                    ));
                }
            }
        }
        if let Some(overflow) = &body.overflow {
            targets.push((TipTarget::Overflow, overflow.rect, "More"));
        }
        let labels: Vec<&str> = targets.iter().map(|(_, _, label)| *label).collect();
        for ((target, rect, _), keys) in targets.into_iter().zip(tip_letters(&labels)) {
            if !keys.is_empty() {
                tips.push(KeyTip { keys, target, rect });
            }
        }
        tips
    }

    /// Apply `f` to every control of `id`: the application's and the shown
    /// alike, so that the state of a command -- a toggle on, a choice made --
    /// is one state wherever it appears, and survives the shown tabs being
    /// built again. Answers whether any control has that command.
    fn each_control(&mut self, id: CommandId, mut f: impl FnMut(&mut Control)) -> bool {
        let mut any = false;
        for control in self
            .base
            .iter_mut()
            .chain(self.tabs.iter_mut())
            .flat_map(|t| &mut t.groups)
            .flat_map(|g| &mut g.controls)
            .filter(|c| c.command().id == id)
        {
            f(control);
            any = true;
        }
        any
    }

    fn keep_front_visible(&mut self) {
        let front = self
            .front()
            .and_then(|i| self.tabs.get(i))
            .map_or_else(String::new, |t| t.id.clone());
        self.front = front;
    }

    /// The group `group` of the front tab.
    fn group(&self, group: usize) -> Option<&Group> {
        self.front()
            .and_then(|t| self.tabs.get(t))
            .and_then(|t| t.groups.get(group))
    }

    /// The control at `group`, `control` of the front tab.
    fn control_at(&self, group: usize, control: usize) -> Option<&Control> {
        self.group(group).and_then(|g| g.controls.get(control))
    }
}

// ============================================================================
// Geometry
// ============================================================================

/// Where everything is, for one width: what [`Ribbon::layout`] answers, what
/// input is tested against and what [`draw`] draws.
#[derive(Clone, Debug, PartialEq)]
pub struct RibbonLayout {
    /// The room the ribbon takes at the top of the window: the strip, the
    /// body unless it is minimized, and the Quick Access Toolbar's row when
    /// it has anything on it. The page goes under it.
    pub rect: Rect,
    /// The window, which a menu the ribbon opens is kept inside.
    pub viewport: (f32, f32),
    /// The Quick Access Toolbar, over the strip or under the body; `None`
    /// while nothing is on it.
    pub qat: Option<QatSlots>,
    /// The strip of tabs.
    pub strip: Rect,
    /// Each shown tab.
    pub tabs: Vec<TabSlot>,
    /// The front tab's groups, under the strip; `None` when minimized.
    pub body: Option<BodySlots>,
    /// A panel over the page: a folded group whole, or a minimized ribbon's
    /// front tab.
    pub panel: Option<BodySlots>,
}

/// Where the Quick Access Toolbar is.
#[derive(Clone, Debug, PartialEq)]
pub struct QatSlots {
    /// Its row.
    pub rect: Rect,
    /// Its buttons: which of [`Ribbon::qat`], and where.
    pub buttons: Vec<(usize, Rect)>,
}

/// Where a tab is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabSlot {
    /// The tab, by its place in the tabs the ribbon was given.
    pub tab: usize,
    /// Where it is.
    pub rect: Rect,
}

/// Where a row of groups is: the body, or a panel.
#[derive(Clone, Debug, PartialEq)]
pub struct BodySlots {
    /// The whole of it.
    pub rect: Rect,
    /// Each group shown, left to right.
    pub groups: Vec<GroupSlot>,
    /// The `»` button, when some groups do not fit even folded.
    pub overflow: Option<OverflowSlot>,
}

/// Where a group is.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupSlot {
    /// The group, by its place in the front tab.
    pub group: usize,
    /// The whole of it, its name included.
    pub rect: Rect,
    /// The button it is folded into, when it is folded.
    pub button: Option<Rect>,
    /// Its name, under its controls; `None` when it is folded, when the name
    /// is on the button.
    pub label: Option<Rect>,
    /// Its controls; none when it is folded.
    pub controls: Vec<ControlSlot>,
}

/// Where a control is.
#[derive(Clone, Debug, PartialEq)]
pub struct ControlSlot {
    /// The control, by its place in its group.
    pub control: usize,
    /// The whole of it.
    pub rect: Rect,
    /// The part a press does the command of.
    pub face: Rect,
    /// The part that opens its menu, where it has one.
    pub arrow: Option<Rect>,
    /// A gallery's choices shown in the group: which choice, and where.
    pub choices: Vec<(usize, Rect)>,
}

/// Where the `»` button is, and the groups it holds.
#[derive(Clone, Debug, PartialEq)]
pub struct OverflowSlot {
    /// Where it is.
    pub rect: Rect,
    /// The groups it holds, by their place in the front tab.
    pub groups: Vec<usize>,
}

/// A column of a group: one large control or gallery, or up to three rows.
struct Column {
    full: bool,
    width: f32,
    items: Vec<(usize, f32)>,
}

impl Ribbon {
    /// Where everything is with the ribbon `width` wide at `x`, `y`, in a
    /// window `viewport` big.
    #[must_use]
    pub fn layout(&self, x: f32, y: f32, width: f32, viewport: (f32, f32)) -> RibbonLayout {
        // The toolbar takes a row only when it has something on it.
        let pinned: Vec<usize> = (0..self.custom.qat.len())
            .filter(|&i| {
                self.custom
                    .qat
                    .get(i)
                    .is_some_and(|&id| self.command_control(id).is_some())
            })
            .collect();
        let qat_above = !pinned.is_empty() && !self.custom.qat_below;
        let strip = Rect::new(
            x,
            if qat_above { y + QAT_HEIGHT } else { y },
            width,
            STRIP_HEIGHT,
        );
        let tabs = self.lay_tabs(strip);
        let body_rect = Rect::new(x, strip.bottom(), width, BODY_HEIGHT);
        let front = self.front();
        let body = if self.custom.minimized {
            None
        } else {
            front.map(|t| self.lay_body(t, body_rect))
        };
        let ribbon_bottom = if self.custom.minimized {
            strip.bottom()
        } else {
            body_rect.bottom()
        };
        let qat = (!pinned.is_empty())
            .then(|| lay_qat(x, if qat_above { y } else { ribbon_bottom }, width, &pinned));
        let bottom = if qat_above {
            ribbon_bottom
        } else {
            qat.as_ref().map_or(ribbon_bottom, |q| q.rect.bottom())
        };
        let rect = Rect::new(x, y, width, bottom - y);
        let panel = match self.panel {
            Some(Panel::Tab) if self.custom.minimized => front.map(|t| self.lay_body(t, body_rect)),
            Some(Panel::Group { group, anchor }) => self.lay_group_panel(group, anchor, rect),
            _ => None,
        };
        RibbonLayout {
            rect,
            viewport,
            qat,
            strip,
            tabs,
            body,
            panel,
        }
    }

    fn lay_tabs(&self, strip: Rect) -> Vec<TabSlot> {
        let mut x = strip.x + STRIP_INDENT;
        let mut contextual = false;
        let mut slots = Vec::new();
        for i in self.visible_tabs() {
            let Some(tab) = self.tabs.get(i) else {
                continue;
            };
            if tab.context.is_some() && !contextual {
                contextual = true;
                x += CONTEXT_GAP;
            }
            let width =
                text::measure(&tab.label, TAB_SIZE, FontWeightHint::Regular) + 2.0 * TAB_PAD;
            slots.push(TabSlot {
                tab: i,
                rect: Rect::new(x, strip.y + TAB_TOP, width, strip.h - TAB_TOP),
            });
            x += width + TAB_GAP;
        }
        slots
    }

    /// The front tab `tab`'s groups laid out in `area`: whole while they fit,
    /// folded lowest priority first when they do not, and behind `»` what
    /// does not fit folded.
    fn lay_body(&self, tab: usize, area: Rect) -> BodySlots {
        let groups: &[Group] = self.tabs.get(tab).map_or(&[], |t| &t.groups);
        let whole: Vec<f32> = groups.iter().map(group_width).collect();
        let small: Vec<f32> = groups.iter().map(folded_width).collect();
        let width_of = |i: usize, folded: &[bool]| {
            if folded.get(i).copied().unwrap_or(false) {
                small.get(i).copied().unwrap_or(0.0)
            } else {
                whole.get(i).copied().unwrap_or(0.0)
            }
        };
        let total = |folded: &[bool]| (0..groups.len()).map(|i| width_of(i, folded)).sum::<f32>();

        let mut folded = vec![false; groups.len()];
        let mut order: Vec<usize> = (0..groups.len()).collect();
        order.sort_by_key(|&i| {
            (
                groups.get(i).map_or(0, |g| g.priority),
                std::cmp::Reverse(i),
            )
        });
        for i in order {
            if total(&folded) <= area.w {
                break;
            }
            if let Some(f) = folded.get_mut(i) {
                *f = true;
            }
        }

        let mut shown: Vec<usize> = (0..groups.len()).collect();
        let mut hidden = Vec::new();
        if total(&folded) > area.w {
            shown.clear();
            let mut used = OVERFLOW_WIDTH;
            for i in 0..groups.len() {
                let w = width_of(i, &folded);
                if hidden.is_empty() && used + w <= area.w {
                    shown.push(i);
                    used += w;
                } else {
                    hidden.push(i);
                }
            }
        }

        let mut x = area.x;
        let mut slots = Vec::new();
        for i in shown {
            let Some(group) = groups.get(i) else {
                continue;
            };
            let slot = if folded.get(i).copied().unwrap_or(false) {
                lay_folded(i, group, x, area.y)
            } else {
                lay_group(i, group, x, area.y)
            };
            x = slot.rect.right();
            slots.push(slot);
        }
        // Inside the ribbon even when the ribbon is narrower than it: a
        // button drawn past the ribbon's edge is one the window may not show.
        let overflow = (!hidden.is_empty()).then(|| OverflowSlot {
            rect: Rect::new(
                x,
                area.y + CONTENT_TOP,
                OVERFLOW_WIDTH.min((area.right() - x).max(0.0)),
                CONTENT_HEIGHT,
            ),
            groups: hidden,
        });
        BodySlots {
            rect: area,
            groups: slots,
            overflow,
        }
    }

    /// A folded group shown whole, hanging under where it is folded and kept
    /// within the ribbon's width.
    fn lay_group_panel(&self, group: usize, anchor: Rect, ribbon: Rect) -> Option<BodySlots> {
        let g = self.group(group)?;
        let width = group_width(g);
        let x = anchor.x.min(ribbon.right() - width).max(ribbon.x);
        let slot = lay_group(group, g, x, anchor.bottom());
        Some(BodySlots {
            rect: slot.rect,
            groups: vec![slot],
            overflow: None,
        })
    }

    /// What is at `x`, `y`; `None` when it is none of the ribbon's.
    #[must_use]
    pub fn hit(&self, layout: &RibbonLayout, x: f32, y: f32) -> Option<Hit> {
        if let Some(panel) = &layout.panel
            && panel.rect.contains(x, y)
        {
            return Some(hit_body(panel, x, y));
        }
        if let Some(qat) = &layout.qat
            && qat.rect.contains(x, y)
        {
            return Some(
                qat.buttons
                    .iter()
                    .find(|(_, r)| r.contains(x, y))
                    .map_or(Hit::Blank, |(i, _)| Hit::Qat(*i)),
            );
        }
        if layout.strip.contains(x, y) {
            return Some(
                layout
                    .tabs
                    .iter()
                    .find(|t| t.rect.contains(x, y))
                    .map_or(Hit::Blank, |t| Hit::Tab(t.tab)),
            );
        }
        if let Some(body) = &layout.body
            && body.rect.contains(x, y)
        {
            return Some(hit_body(body, x, y));
        }
        None
    }
}

fn hit_body(body: &BodySlots, x: f32, y: f32) -> Hit {
    if body
        .overflow
        .as_ref()
        .is_some_and(|o| o.rect.contains(x, y))
    {
        return Hit::Overflow;
    }
    for g in &body.groups {
        if g.button.is_some_and(|b| b.contains(x, y)) {
            return Hit::Folded(g.group);
        }
        for c in &g.controls {
            if !c.rect.contains(x, y) {
                continue;
            }
            let part = if let Some((i, _)) = c.choices.iter().find(|(_, r)| r.contains(x, y)) {
                Part::Choice(*i)
            } else if c.arrow.is_some_and(|a| a.contains(x, y)) {
                Part::Arrow
            } else {
                Part::Face
            };
            return Hit::Control {
                group: g.group,
                control: c.control,
                part,
            };
        }
    }
    Hit::Blank
}

/// How wide `label` is in a control's face.
fn label_width(label: &str) -> f32 {
    text::measure(label, LABEL_SIZE, FontWeightHint::Regular)
}

/// A large button's name, on one line or -- when it is wide and has a space
/// in it -- on two, broken where the longer line is shortest.
fn large_lines(label: &str) -> Vec<String> {
    if label_width(label) <= LARGE_WRAP || !label.contains(' ') {
        return vec![label.to_owned()];
    }
    let mut best: Option<(f32, usize)> = None;
    for (at, _) in label.match_indices(' ') {
        let (left, right) = label.split_at(at);
        let longer = label_width(left).max(label_width(right.trim_start()));
        if best.is_none_or(|(w, _)| longer < w) {
            best = Some((longer, at));
        }
    }
    best.map_or_else(
        || vec![label.to_owned()],
        |(_, at)| {
            let (left, right) = label.split_at(at);
            vec![left.to_owned(), right.trim_start().to_owned()]
        },
    )
}

/// A command's tooltip: its name, then why it cannot be used -- `design.txt`
/// asks every disabled control to say -- or what more it does.
fn command_tip(command: &Command) -> String {
    let more = if command.enabled {
        command.tooltip.clone()
    } else {
        Some(
            command
                .disabled_reason
                .clone()
                .unwrap_or_else(|| "Not available now".to_owned()),
        )
    };
    match more {
        Some(more) => format!("{}\n{more}", command.label),
        None => command.label.clone(),
    }
}

/// The first field of [`Ribbon::customization_text`]: the name of its shape,
/// so that text of a later shape is ignored rather than misread.
const TEXT_VERSION: &str = "ribbon1";

/// Whether an id can go into the customization text as it is: letters,
/// digits, `_`, `-` and `.`, none of them a separator of the text.
fn plain(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// A `tab/group/command` entry of the customization text.
fn entry_of(entry: &str) -> Option<(String, String, CommandId)> {
    let mut parts = entry.split('/');
    let (tab, group, id) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || !plain(tab) || !plain(group) {
        return None;
    }
    Some((tab.to_owned(), group.to_owned(), id.parse().ok()?))
}

/// Key tips for things named `labels`, in order: each the first letter of
/// one of its name's words that is free, else another letter of its name,
/// else any free letter -- so Cut is `c` and Copy, coming after it, `o`. Past
/// twenty-six things every tip is two letters, the first chosen the same way,
/// so that no tip is the start of another. A thing past 676 gets none.
fn tip_letters(labels: &[&str]) -> Vec<String> {
    const ABC: &str = "abcdefghijklmnopqrstuvwxyz";
    let two = labels.len() > ABC.len();
    let mut used: BTreeSet<String> = BTreeSet::new();
    labels
        .iter()
        .map(|label| {
            let initials = label
                .split_whitespace()
                .filter_map(|word| word.chars().next());
            let preferred: Vec<char> = initials
                .chain(label.chars())
                .chain(ABC.chars())
                .filter(char::is_ascii_alphabetic)
                .map(|c| c.to_ascii_lowercase())
                .collect();
            let pick = if two {
                preferred
                    .iter()
                    .flat_map(|first| ABC.chars().map(move |second| format!("{first}{second}")))
                    .find(|tip| !used.contains(tip))
            } else {
                preferred
                    .iter()
                    .map(char::to_string)
                    .find(|tip| !used.contains(tip))
            };
            let tip = pick.unwrap_or_default();
            if !tip.is_empty() {
                used.insert(tip.clone());
            }
            tip
        })
        .collect()
}

/// How wide a control is.
fn control_width(control: &Control) -> f32 {
    let arrow = if matches!(control, Control::Split { .. }) {
        ARROW_WIDTH
    } else {
        0.0
    };
    match control {
        Control::Button { command, size }
        | Control::Toggle { command, size, .. }
        | Control::Split { command, size, .. } => match size {
            ButtonSize::Large => {
                let lines = large_lines(&command.label)
                    .iter()
                    .map(|line| label_width(line))
                    .fold(0.0_f32, f32::max);
                // A split button's arrow sits after its last line.
                let widest = if arrow > 0.0 { lines + arrow } else { lines };
                widest.max(LARGE_ICON as f32) + 2.0 * LARGE_PAD
            }
            ButtonSize::Medium => {
                BUTTON_PAD
                    + SMALL_ICON as f32
                    + ICON_GAP
                    + label_width(&command.label)
                    + BUTTON_PAD
                    + arrow
            }
            ButtonSize::Small => 2.0 * BUTTON_PAD + SMALL_ICON as f32 + arrow,
        },
        Control::Dropdown { width, .. } => width.max(MIN_DROPDOWN),
        Control::Gallery { shown, .. } => (*shown).max(1) as f32 * GALLERY_CELL + GALLERY_MORE,
    }
}

/// A group's controls in columns: a large control or a gallery takes one to
/// itself, and the others stack three to a column, in order.
fn columns(group: &Group) -> Vec<Column> {
    let mut columns: Vec<Column> = Vec::new();
    for (i, control) in group.controls.iter().enumerate() {
        let width = control_width(control);
        if control.takes_column() {
            columns.push(Column {
                full: true,
                width,
                items: vec![(i, width)],
            });
            continue;
        }
        match columns.last_mut() {
            Some(column) if !column.full && column.items.len() < ROWS => {
                column.width = column.width.max(width);
                column.items.push((i, width));
            }
            _ => columns.push(Column {
                full: false,
                width,
                items: vec![(i, width)],
            }),
        }
    }
    columns
}

/// How wide a group is shown whole: its columns, or its name if that is
/// wider.
fn group_width(group: &Group) -> f32 {
    let columns = columns(group);
    let gaps = columns.len().saturating_sub(1) as f32 * COLUMN_GAP;
    let content: f32 = columns.iter().map(|c| c.width).sum::<f32>() + gaps;
    let label = text::measure(&group.label, GROUP_LABEL_SIZE, FontWeightHint::Regular);
    content.max(label) + 2.0 * GROUP_PAD
}

/// How wide a group is folded: a large button with its picture, its name and
/// an arrow.
fn folded_width(group: &Group) -> f32 {
    let lines = large_lines(&group.label)
        .iter()
        .map(|line| label_width(line))
        .fold(0.0_f32, f32::max);
    lines.max(LARGE_ICON as f32) + 2.0 * LARGE_PAD + 2.0 * GROUP_PAD
}

/// The Quick Access Toolbar's row at `x`, `y`, `width` wide: a small square
/// button for each of `pinned` -- its places in the toolbar -- that fits.
fn lay_qat(x: f32, y: f32, width: f32, pinned: &[usize]) -> QatSlots {
    let rect = Rect::new(x, y, width, QAT_HEIGHT);
    let side = QAT_HEIGHT - 2.0 * QAT_PAD;
    let mut left = x + STRIP_INDENT;
    let mut buttons = Vec::new();
    for &i in pinned {
        if left + side > rect.right() {
            break;
        }
        buttons.push((i, Rect::new(left, y + QAT_PAD, side, side)));
        left += side + QAT_PAD;
    }
    QatSlots { rect, buttons }
}

/// A group folded into one button at `x`, `y`.
fn lay_folded(index: usize, group: &Group, x: f32, y: f32) -> GroupSlot {
    let width = folded_width(group);
    GroupSlot {
        group: index,
        rect: Rect::new(x, y, width, BODY_HEIGHT),
        button: Some(Rect::new(
            x + GROUP_PAD,
            y + CONTENT_TOP,
            width - 2.0 * GROUP_PAD,
            CONTENT_HEIGHT + GROUP_LABEL_HEIGHT - CONTENT_TOP,
        )),
        label: None,
        controls: Vec::new(),
    }
}

/// A group shown whole at `x`, `y`.
fn lay_group(index: usize, group: &Group, x: f32, y: f32) -> GroupSlot {
    let width = group_width(group);
    let columns = columns(group);
    let gaps = columns.len().saturating_sub(1) as f32 * COLUMN_GAP;
    let content: f32 = columns.iter().map(|c| c.width).sum::<f32>() + gaps;
    let top = y + CONTENT_TOP;
    // Centred when the group's name is wider than its controls.
    let mut cx = x + GROUP_PAD + ((width - 2.0 * GROUP_PAD - content) / 2.0).max(0.0);
    let mut controls = Vec::new();
    for column in &columns {
        if column.full {
            for &(i, _) in &column.items {
                if let Some(control) = group.controls.get(i) {
                    controls.push(lay_control(
                        i,
                        control,
                        Rect::new(cx, top, column.width, CONTENT_HEIGHT),
                    ));
                }
            }
        } else {
            // Fewer than three rows sit in the middle of the column.
            let empty = ROWS.saturating_sub(column.items.len()) as f32;
            let first = top + empty * ROW_HEIGHT / 2.0;
            for (row, &(i, w)) in column.items.iter().enumerate() {
                if let Some(control) = group.controls.get(i) {
                    controls.push(lay_control(
                        i,
                        control,
                        Rect::new(cx, first + row as f32 * ROW_HEIGHT, w, ROW_HEIGHT),
                    ));
                }
            }
        }
        cx += column.width + COLUMN_GAP;
    }
    GroupSlot {
        group: index,
        rect: Rect::new(x, y, width, BODY_HEIGHT),
        button: None,
        label: Some(Rect::new(
            x,
            y + CONTENT_TOP + CONTENT_HEIGHT,
            width,
            GROUP_LABEL_HEIGHT,
        )),
        controls,
    }
}

/// Which of a gallery's choices it shows: the first `shown`, or -- when the
/// chosen one is further on -- the run that ends with it.
fn gallery_window(count: usize, shown: usize, selected: Option<usize>) -> std::ops::Range<usize> {
    let shown = shown.max(1).min(count);
    let start = selected
        .filter(|&s| s < count && s >= shown)
        .map_or(0, |s| s.saturating_add(1).saturating_sub(shown));
    start..start.saturating_add(shown).min(count)
}

/// Where a control's parts are, in `rect`.
fn lay_control(index: usize, control: &Control, rect: Rect) -> ControlSlot {
    let mut slot = ControlSlot {
        control: index,
        rect,
        face: rect,
        arrow: None,
        choices: Vec::new(),
    };
    match control {
        Control::Split { size, .. } => match size {
            ButtonSize::Large => {
                slot.face = Rect::new(rect.x, rect.y, rect.w, LARGE_FACE);
                slot.arrow = Some(Rect::new(
                    rect.x,
                    rect.y + LARGE_FACE,
                    rect.w,
                    (rect.h - LARGE_FACE).max(0.0),
                ));
            }
            ButtonSize::Medium | ButtonSize::Small => {
                slot.face = Rect::new(rect.x, rect.y, (rect.w - ARROW_WIDTH).max(0.0), rect.h);
                slot.arrow = Some(Rect::new(
                    rect.right() - ARROW_WIDTH,
                    rect.y,
                    ARROW_WIDTH,
                    rect.h,
                ));
            }
        },
        // The whole box opens the list, as a field that offers choices does.
        Control::Dropdown { .. } => slot.arrow = Some(rect),
        Control::Gallery {
            choices,
            selected,
            shown,
            ..
        } => {
            let mut x = rect.x;
            for i in gallery_window(choices.len(), *shown, *selected) {
                slot.choices
                    .push((i, Rect::new(x, rect.y + 2.0, GALLERY_CELL, rect.h - 4.0)));
                x += GALLERY_CELL;
            }
            slot.arrow = Some(Rect::new(
                rect.right() - GALLERY_MORE,
                rect.y,
                GALLERY_MORE,
                rect.h,
            ));
        }
        Control::Button { .. } | Control::Toggle { .. } => {}
    }
    slot
}

/// The control slot at `group`, `control` in `layout`: in the panel if one is
/// open, else in the body.
fn find_slot(layout: &RibbonLayout, group: usize, control: usize) -> Option<&ControlSlot> {
    layout
        .panel
        .iter()
        .chain(layout.body.iter())
        .flat_map(|b| &b.groups)
        .filter(|g| g.group == group)
        .flat_map(|g| &g.controls)
        .find(|c| c.control == control)
}

// ============================================================================
// Input
// ============================================================================

impl Ribbon {
    /// Handle a pointer event, `layout` being where everything was drawn.
    pub fn handle_mouse(&mut self, layout: &RibbonLayout, event: &MouseEvent) -> RibbonEvent {
        let (x, y) = (event.x, event.y);
        if self.menu.is_some() {
            return self.menu_mouse(event);
        }
        let hit = self.hit(layout, x, y);
        if matches!(event.kind, MouseEventKind::Press(_)) {
            // The pointer has taken over from the keyboard, and a tooltip is
            // not wanted over what is being pressed.
            self.tips = None;
            self.tooltip = None;
            self.resting = None;
        }
        match &event.kind {
            MouseEventKind::Move | MouseEventKind::Enter => {
                if hit != self.hover {
                    self.hover = hit;
                    // Something new under the pointer: its tooltip starts
                    // over, timed from the next tick.
                    self.tooltip = None;
                    self.resting = matches!(
                        hit,
                        Some(Hit::Control { .. } | Hit::Folded(_) | Hit::Overflow | Hit::Qat(_))
                    )
                    .then_some((x, y, layout.viewport));
                }
                if hit.is_some() || self.panel.is_some() {
                    RibbonEvent::Handled
                } else {
                    RibbonEvent::Ignored
                }
            }
            MouseEventKind::Leave => {
                self.hover = None;
                self.tooltip = None;
                self.resting = None;
                RibbonEvent::Handled
            }
            MouseEventKind::Press(MouseButton::Left) => self.press(layout, hit, x, y),
            MouseEventKind::Press(MouseButton::Right) => {
                // A right-click closes an open panel, as any press outside it
                // does, and offers the changes the user can make there.
                if self.panel.is_some()
                    && !layout.panel.as_ref().is_some_and(|p| p.rect.contains(x, y))
                {
                    self.panel = None;
                }
                self.open_custom_menu(hit, x, y, layout.viewport)
            }
            MouseEventKind::Release(MouseButton::Left) => self.release(hit),
            MouseEventKind::DoubleClick(MouseButton::Left) => match hit {
                Some(Hit::Tab(_)) => {
                    self.set_minimized(!self.custom.minimized);
                    RibbonEvent::Customized
                }
                Some(_) => RibbonEvent::Handled,
                None => RibbonEvent::Ignored,
            },
            _ => {
                if hit.is_some() {
                    RibbonEvent::Handled
                } else {
                    RibbonEvent::Ignored
                }
            }
        }
    }

    /// A left press on `hit`: a tab comes to the front at once, and a
    /// control's arrow opens its menu at once; a face waits for the release,
    /// as every button does, so a press dragged off it does nothing.
    fn press(&mut self, layout: &RibbonLayout, hit: Option<Hit>, x: f32, y: f32) -> RibbonEvent {
        let in_panel = layout.panel.as_ref().is_some_and(|p| p.rect.contains(x, y));
        // A press anywhere but the panel open, or a minimized ribbon's tabs,
        // closes the panel -- and does nothing else, as a press outside a menu
        // does nothing else.
        if self.panel.is_some() && !in_panel && !matches!(hit, Some(Hit::Tab(_))) {
            self.panel = None;
            return RibbonEvent::Handled;
        }
        match hit {
            None => RibbonEvent::Ignored,
            Some(Hit::Tab(i)) => self.press_tab(i),
            Some(Hit::Control {
                group,
                control,
                part: Part::Arrow,
            }) => self.open_control_menu(layout, group, control),
            Some(hit @ Hit::Control { .. }) => {
                self.pressed = Some(hit);
                RibbonEvent::Handled
            }
            Some(hit @ Hit::Qat(i)) => {
                let Some(&id) = self.custom.qat.get(i) else {
                    return RibbonEvent::Handled;
                };
                match self.command_control(id) {
                    // A list on the toolbar opens as it does on its tab.
                    Some(Control::Dropdown { .. } | Control::Gallery { .. }) => {
                        let anchor = layout
                            .qat
                            .as_ref()
                            .and_then(|q| q.buttons.iter().find(|(b, _)| *b == i))
                            .map(|(_, r)| *r);
                        anchor.map_or(RibbonEvent::Handled, |anchor| {
                            self.open_command_menu(id, anchor, layout.viewport)
                        })
                    }
                    Some(_) => {
                        self.pressed = Some(hit);
                        RibbonEvent::Handled
                    }
                    None => RibbonEvent::Handled,
                }
            }
            Some(Hit::Folded(group)) => {
                let anchor = layout
                    .body
                    .iter()
                    .chain(layout.panel.iter())
                    .flat_map(|b| &b.groups)
                    .find(|g| g.group == group)
                    .map(|g| g.rect);
                self.panel = match (self.panel, anchor) {
                    (Some(Panel::Group { group: open, .. }), _) if open == group => None,
                    (_, Some(anchor)) => Some(Panel::Group { group, anchor }),
                    (_, None) => None,
                };
                RibbonEvent::Handled
            }
            Some(Hit::Overflow) => self.open_overflow_menu(layout),
            Some(Hit::Blank) => RibbonEvent::Handled,
        }
    }

    fn press_tab(&mut self, tab: usize) -> RibbonEvent {
        let Some(id) = self.tabs.get(tab).map(|t| t.id.clone()) else {
            return RibbonEvent::Handled;
        };
        let changed = self.front != id;
        self.front = id.clone();
        self.menu = None;
        if self.custom.minimized {
            // Pressing the tab whose commands are showing puts them away.
            self.panel = if !changed && self.panel == Some(Panel::Tab) {
                None
            } else {
                Some(Panel::Tab)
            };
        } else {
            self.panel = None;
        }
        if changed {
            RibbonEvent::TabSelected(id)
        } else {
            RibbonEvent::Handled
        }
    }

    /// A left release on `hit`: the command of the face it was pressed on,
    /// if it is still over it.
    fn release(&mut self, hit: Option<Hit>) -> RibbonEvent {
        let pressed = self.pressed.take();
        match (pressed, hit) {
            (
                Some(Hit::Control {
                    group,
                    control,
                    part,
                }),
                Some(now),
            ) if pressed == Some(now) => self.activate(group, control, part),
            (Some(Hit::Qat(i)), Some(now)) if pressed == Some(now) => {
                let Some(&id) = self.custom.qat.get(i) else {
                    return RibbonEvent::Handled;
                };
                let press = self
                    .command_control(id)
                    .map_or(Press::Nothing, |c| press_of(&c, Part::Face));
                self.use_command(id, press)
            }
            (_, None) => RibbonEvent::Ignored,
            _ => RibbonEvent::Handled,
        }
    }

    /// Do what pressing `part` of the control at `group`, `control` does.
    fn activate(&mut self, group: usize, control: usize, part: Part) -> RibbonEvent {
        let Some(c) = self.control_at(group, control) else {
            return RibbonEvent::Handled;
        };
        let event = self.use_command(c.command().id, press_of(c, part));
        // A command given is the end of the panel it was given from.
        if event != RibbonEvent::Handled {
            self.panel = None;
        }
        event
    }

    /// Do `press` to the command `id`, wherever it appears. A toggle's state
    /// and a list's choice belong to the command, not to one of its buttons:
    /// Bold on two tabs is on in both, or neither.
    fn use_command(&mut self, id: CommandId, press: Press) -> RibbonEvent {
        let Some(c) = self.command_control(id) else {
            return RibbonEvent::Handled;
        };
        if !c.command().enabled {
            return RibbonEvent::Handled;
        }
        match press {
            Press::Face => RibbonEvent::Command(id),
            Press::Toggle(on) => {
                self.set_on(id, on);
                RibbonEvent::Toggled { id, on }
            }
            Press::Choose(index) => {
                self.set_selected(id, Some(index));
                RibbonEvent::Chose { id, index }
            }
            Press::Nothing => RibbonEvent::Handled,
        }
    }

    /// The first control of `id` shown, or the offered command's button.
    fn command_control(&self, id: CommandId) -> Option<Control> {
        self.tabs
            .iter()
            .flat_map(|t| &t.groups)
            .flat_map(|g| &g.controls)
            .find(|c| c.command().id == id)
            .cloned()
            .or_else(|| self.catalog_control(id))
    }

    /// Open the menu under the control at `group`, `control`: a split
    /// button's, or a dropdown's or gallery's list of choices.
    fn open_control_menu(
        &mut self,
        layout: &RibbonLayout,
        group: usize,
        control: usize,
    ) -> RibbonEvent {
        let Some(anchor) = find_slot(layout, group, control).map(|s| s.rect) else {
            return RibbonEvent::Handled;
        };
        let Some(id) = self.control_at(group, control).map(|c| c.command().id) else {
            return RibbonEvent::Handled;
        };
        self.open_command_menu(id, anchor, layout.viewport)
    }

    /// Open the menu of the command `id` under `anchor`: a split button's
    /// rows, or a dropdown's or gallery's choices.
    fn open_command_menu(
        &mut self,
        id: CommandId,
        anchor: Rect,
        viewport: (f32, f32),
    ) -> RibbonEvent {
        let Some(c) = self.command_control(id) else {
            return RibbonEvent::Handled;
        };
        if !c.command().enabled {
            return RibbonEvent::Handled;
        }
        let mut acts = Vec::new();
        let items = match &c {
            Control::Split { menu, .. } => split_rows(menu, id, &mut acts),
            Control::Dropdown {
                choices, selected, ..
            }
            | Control::Gallery {
                choices, selected, ..
            } => choice_rows(id, choices, *selected, &mut acts),
            Control::Button { .. } | Control::Toggle { .. } => return RibbonEvent::Handled,
        };
        let mut menu = ContextMenu::new(items);
        // A list at least as wide as the box it drops from.
        menu.set_min_width(anchor.w);
        menu.show(anchor.x, anchor.bottom(), viewport);
        self.menu = Some(OpenMenu { menu, acts });
        RibbonEvent::Handled
    }

    /// Open the `»` button's menu: each group it holds as a submenu of its
    /// controls.
    fn open_overflow_menu(&mut self, layout: &RibbonLayout) -> RibbonEvent {
        let Some(overflow) = layout
            .panel
            .iter()
            .chain(layout.body.iter())
            .find_map(|b| b.overflow.as_ref())
        else {
            return RibbonEvent::Handled;
        };
        let mut acts = Vec::new();
        let mut items = Vec::new();
        for &g in &overflow.groups {
            let Some(group) = self.group(g) else {
                continue;
            };
            items.push(MenuItem::Submenu {
                id: SUBMENU_ROW,
                label: group.label.clone(),
                icon: group.icon.clone(),
                enabled: true,
                children: group_rows(group, &mut acts),
            });
        }
        let mut menu = ContextMenu::new(items);
        menu.show(overflow.rect.x, overflow.rect.bottom(), layout.viewport);
        self.menu = Some(OpenMenu { menu, acts });
        RibbonEvent::Handled
    }

    /// The menu of changes a right-click offers, for what it landed on: a
    /// command's place on the Quick Access Toolbar and in its group, a
    /// group's commands, a tab's place -- and, wherever it lands, the tabs
    /// to show again, where the toolbar goes, minimizing, and undoing every
    /// change.
    fn open_custom_menu(
        &mut self,
        hit: Option<Hit>,
        x: f32,
        y: f32,
        viewport: (f32, f32),
    ) -> RibbonEvent {
        let Some(hit) = hit else {
            return RibbonEvent::Ignored;
        };
        let mut m = MenuBuilder::default();
        let front = self
            .front()
            .and_then(|t| self.tabs.get(t))
            .map(|t| t.id.clone())
            .unwrap_or_default();
        match hit {
            Hit::Control { group, control, .. } => {
                if let (Some(g), Some(c)) = (self.group(group), self.control_at(group, control)) {
                    let id = c.command().id;
                    self.pin_row(&mut m, id);
                    m.row(
                        "Remove from this group",
                        Act::Remove(front.clone(), g.id.clone(), id),
                        true,
                    );
                    self.add_rows(&mut m, &front, g);
                }
            }
            Hit::Folded(group) => {
                if let Some(g) = self.group(group) {
                    self.add_rows(&mut m, &front, g);
                }
            }
            Hit::Qat(i) => {
                if let Some(&id) = self.custom.qat.get(i) {
                    m.row(
                        "Remove from Quick Access Toolbar",
                        Act::Pin(id, false),
                        true,
                    );
                }
            }
            Hit::Tab(t) => {
                if let Some(tab) = self.tabs.get(t) {
                    let id = tab.id.clone();
                    m.row(
                        "Hide this tab",
                        Act::HideTab(id.clone(), true),
                        self.can_hide(&id),
                    );
                    if tab.context.is_none() {
                        m.row(
                            "Move left",
                            Act::MoveTab(id.clone(), true),
                            self.move_target(&id, true).is_some(),
                        );
                        m.row(
                            "Move right",
                            Act::MoveTab(id.clone(), false),
                            self.move_target(&id, false).is_some(),
                        );
                    }
                }
            }
            Hit::Overflow | Hit::Blank => {}
        }
        m.separator();
        let hidden: Vec<(String, String)> = self
            .custom
            .hidden
            .iter()
            .filter_map(|id| {
                self.tabs
                    .iter()
                    .find(|t| t.id == *id)
                    .map(|t| (t.id.clone(), t.label.clone()))
            })
            .collect();
        if !hidden.is_empty() {
            let children: Vec<MenuItem> = hidden
                .into_iter()
                .map(|(id, label)| m.child(label, None, Act::HideTab(id, false), true))
                .collect();
            m.submenu("Show a hidden tab", children);
        }
        let below = self.custom.qat_below;
        m.row(
            if below {
                "Show the Quick Access Toolbar over the ribbon"
            } else {
                "Show the Quick Access Toolbar under the ribbon"
            },
            Act::QatBelow(!below),
            true,
        );
        let minimized = self.custom.minimized;
        m.row(
            if minimized {
                "Expand the ribbon"
            } else {
                "Collapse the ribbon"
            },
            Act::Minimize(!minimized),
            true,
        );
        if self.customized() {
            m.row("Undo all changes to the ribbon", Act::Reset, true);
        }
        let mut menu = ContextMenu::new(m.items);
        menu.show(x, y, viewport);
        self.menu = Some(OpenMenu { menu, acts: m.acts });
        RibbonEvent::Handled
    }

    /// The row that puts the command `id` on the Quick Access Toolbar, or
    /// takes it off.
    fn pin_row(&self, m: &mut MenuBuilder, id: CommandId) {
        if self.custom.qat.contains(&id) {
            m.row(
                "Remove from Quick Access Toolbar",
                Act::Pin(id, false),
                true,
            );
        } else {
            m.row("Add to Quick Access Toolbar", Act::Pin(id, true), true);
        }
    }

    /// A submenu of the commands that could be put in the group `group` of
    /// the tab `tab`: every command the ribbon has that the group has not.
    fn add_rows(&self, m: &mut MenuBuilder, tab: &str, group: &Group) {
        let there: BTreeSet<CommandId> = group.controls.iter().map(|c| c.command().id).collect();
        let children: Vec<MenuItem> = self
            .catalog()
            .into_iter()
            .filter(|c| !there.contains(&c.id))
            .map(|c| {
                m.child(
                    c.label,
                    c.icon,
                    Act::Add(tab.to_owned(), group.id.clone(), c.id),
                    true,
                )
            })
            .collect();
        m.submenu("Add a command to this group", children);
    }

    /// Whether the tab `id` can be hidden: not the last ordinary tab shown.
    fn can_hide(&self, id: &str) -> bool {
        let ordinary: Vec<&str> = self
            .visible_tabs()
            .into_iter()
            .filter_map(|i| self.tabs.get(i))
            .filter(|t| t.context.is_none())
            .map(|t| t.id.as_str())
            .collect();
        ordinary != [id]
    }

    /// Where moving the tab `id` one place left (`true`) or right would put
    /// it: every ordinary tab's id in the order shown -- hidden ones too, so
    /// a hidden tab keeps its place for when it is shown again -- and the two
    /// places to swap. `None` when there is no shown tab that way.
    fn move_target(&self, id: &str, left: bool) -> Option<(Vec<String>, usize, usize)> {
        let mut all: Vec<usize> = (0..self.tabs.len())
            .filter(|&i| self.tabs.get(i).is_some_and(|t| t.context.is_none()))
            .collect();
        all.sort_by_key(|&i| self.order_of(i));
        let ids: Vec<String> = all
            .iter()
            .filter_map(|&i| self.tabs.get(i).map(|t| t.id.clone()))
            .collect();
        let at = ids.iter().position(|t| t == id)?;
        let shown = |t: &String| !self.custom.hidden.contains(t);
        let other = if left {
            ids.get(..at)?.iter().rposition(shown)?
        } else {
            let next = at.checked_add(1)?;
            ids.get(next..)?.iter().position(shown)?.checked_add(next)?
        };
        Some((ids, at, other))
    }

    /// A pointer event while a menu is open: the menu's, all of it.
    fn menu_mouse(&mut self, event: &MouseEvent) -> RibbonEvent {
        let Some(open) = self.menu.as_mut() else {
            return RibbonEvent::Ignored;
        };
        match &event.kind {
            MouseEventKind::Move => {
                open.menu.handle_mouse_move(event.x, event.y);
                RibbonEvent::Handled
            }
            MouseEventKind::Scroll { dy, .. } => {
                open.menu.handle_scroll(event.x, event.y, *dy);
                RibbonEvent::Handled
            }
            MouseEventKind::Press(_) => {
                let chosen = open.menu.handle_click(event.x, event.y);
                let act = chosen.and_then(|id| {
                    usize::try_from(id)
                        .ok()
                        .and_then(|i| open.acts.get(i).cloned())
                });
                if chosen.is_some() || !open.menu.is_visible() {
                    self.menu = None;
                }
                act.map_or(RibbonEvent::Handled, |act| self.do_act(act))
            }
            _ => RibbonEvent::Handled,
        }
    }

    fn do_act(&mut self, act: Act) -> RibbonEvent {
        let event = match act {
            Act::Press(id) => {
                let press = self
                    .command_control(id)
                    .map_or(Press::Nothing, |c| press_of(&c, Part::Face));
                self.use_command(id, press)
            }
            Act::SplitRow { id, row } => RibbonEvent::MenuItem { id, item: row },
            Act::Choose { id, index } => {
                let fits = self.command_control(id).is_some_and(|c| {
                    matches!(&c, Control::Dropdown { choices, .. } | Control::Gallery { choices, .. }
                        if index < choices.len())
                });
                if fits {
                    self.use_command(id, Press::Choose(index))
                } else {
                    RibbonEvent::Handled
                }
            }
            Act::Pin(id, on) => {
                if on {
                    self.pin(id);
                } else {
                    self.unpin(id);
                }
                RibbonEvent::Customized
            }
            Act::QatBelow(below) => {
                self.set_qat_below(below);
                RibbonEvent::Customized
            }
            Act::Minimize(minimized) => {
                self.set_minimized(minimized);
                RibbonEvent::Customized
            }
            Act::HideTab(id, hide) => {
                self.hide_tab(&id, hide);
                RibbonEvent::Customized
            }
            Act::MoveTab(id, left) => {
                self.move_tab(&id, left);
                RibbonEvent::Customized
            }
            Act::Remove(tab, group, id) => {
                self.remove_from_group(&tab, &group, id);
                RibbonEvent::Customized
            }
            Act::Add(tab, group, id) => {
                self.add_to_group(&tab, &group, id);
                RibbonEvent::Customized
            }
            Act::Reset => {
                self.reset_customization();
                RibbonEvent::Customized
            }
        };
        // A command given is the end of the panel it was given from.
        if event != RibbonEvent::Handled {
            self.panel = None;
        }
        event
    }

    /// Handle a key, `layout` being where everything was drawn: the open
    /// menu's while one is open, the key tips' while they are shown; F10
    /// shows them; Ctrl+F1 minimizes the ribbon or brings it back; Escape
    /// closes an open panel.
    pub fn handle_key(&mut self, layout: &RibbonLayout, key: &KeyEvent) -> RibbonEvent {
        if !key.pressed {
            return RibbonEvent::Ignored;
        }
        if let Some(open) = self.menu.as_mut() {
            // The menu has the keyboard while it is open: a key it has no use
            // for goes nowhere else, as a key pressed over an open menu does.
            return match open.menu.handle_key(key) {
                Some(MenuAction::Selected(id)) => {
                    let act = usize::try_from(id)
                        .ok()
                        .and_then(|i| open.acts.get(i).cloned());
                    self.menu = None;
                    act.map_or(RibbonEvent::Handled, |act| self.do_act(act))
                }
                Some(MenuAction::Closed) => {
                    self.menu = None;
                    RibbonEvent::Handled
                }
                Some(MenuAction::None) | None => RibbonEvent::Handled,
            };
        }
        if self.tips.is_some() {
            return self.tip_key(layout, key);
        }
        match key.key {
            Key::F10 if key.modifiers == crate::event::Modifiers::NONE => {
                self.tips = Some(String::new());
                self.tooltip = None;
                self.resting = None;
                RibbonEvent::Handled
            }
            Key::F1 if key.modifiers.ctrl => {
                self.set_minimized(!self.custom.minimized);
                RibbonEvent::Customized
            }
            Key::Escape if self.panel.is_some() => {
                self.panel = None;
                RibbonEvent::Handled
            }
            _ => RibbonEvent::Ignored,
        }
    }

    /// A key while key tips are shown. A tip typed whole does what it leads
    /// to; the start of a two-letter tip waits for the rest; Escape steps
    /// back -- out of an open panel, then out of the tips -- and F10 leaves
    /// them. Any other key leaves them too, and is the application's: the
    /// user has gone back to typing.
    fn tip_key(&mut self, layout: &RibbonLayout, key: &KeyEvent) -> RibbonEvent {
        match key.key {
            Key::Escape => {
                if self.panel.is_some() {
                    self.panel = None;
                    self.tips = Some(String::new());
                } else {
                    self.tips = None;
                }
                return RibbonEvent::Handled;
            }
            Key::F10 => {
                self.tips = None;
                return RibbonEvent::Handled;
            }
            Key::Backspace => {
                if let Some(typed) = self.tips.as_mut() {
                    typed.pop();
                }
                return RibbonEvent::Handled;
            }
            _ => {}
        }
        let Some(c) = key
            .text
            .chars()
            .next()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
        else {
            self.tips = None;
            return RibbonEvent::Ignored;
        };
        let mut typed = self.tips.clone().unwrap_or_default();
        typed.push(c);
        let tips = self.key_tips(layout);
        let starting: Vec<&KeyTip> = tips.iter().filter(|t| t.keys.starts_with(&typed)).collect();
        match starting.as_slice() {
            // Nothing starts so: start again, rather than leave the user
            // typing blind into a half-finished tip.
            [] => {
                self.tips = Some(String::new());
                RibbonEvent::Handled
            }
            [one] if one.keys == typed => {
                let target = one.target;
                self.tips = Some(String::new());
                self.tip_act(layout, target)
            }
            _ => {
                self.tips = Some(typed);
                RibbonEvent::Handled
            }
        }
    }

    /// Do what the key tip for `target` leads to. A tab or a folded group
    /// keeps the tips shown, now for what it brought up; anything else is the
    /// end of them, a menu it opens taking the keyboard with its first row
    /// lit.
    fn tip_act(&mut self, layout: &RibbonLayout, target: TipTarget) -> RibbonEvent {
        match target {
            TipTarget::Tab(tab) => self.press_tab(tab),
            TipTarget::Folded(group) => {
                let anchor = layout
                    .body
                    .iter()
                    .chain(layout.panel.iter())
                    .flat_map(|b| &b.groups)
                    .find(|g| g.group == group)
                    .map(|g| g.rect);
                if let Some(anchor) = anchor {
                    self.panel = Some(Panel::Group { group, anchor });
                }
                RibbonEvent::Handled
            }
            TipTarget::Overflow => {
                self.tips = None;
                let event = self.open_overflow_menu(layout);
                self.light_first_row(None);
                event
            }
            TipTarget::Control { group, control } => {
                self.tips = None;
                match self.control_at(group, control) {
                    Some(Control::Button { .. } | Control::Toggle { .. }) => {
                        self.activate(group, control, Part::Face)
                    }
                    Some(Control::Split { .. }) => self.open_split_by_key(layout, group, control),
                    Some(
                        Control::Dropdown { selected, .. } | Control::Gallery { selected, .. },
                    ) => {
                        let selected = *selected;
                        let event = self.open_control_menu(layout, group, control);
                        self.light_first_row(selected);
                        event
                    }
                    None => RibbonEvent::Handled,
                }
            }
        }
    }

    /// Light row `row` -- or the first -- of the open menu, so that Enter
    /// chooses it and the arrows move from it.
    fn light_first_row(&mut self, row: Option<usize>) {
        if let Some(open) = self.menu.as_mut() {
            open.menu.highlight(row.unwrap_or(0));
        }
    }

    /// A split button reached from the keyboard: a menu of its face first --
    /// what a press on it does -- then its own rows, so that both are within
    /// reach of the arrow keys.
    fn open_split_by_key(
        &mut self,
        layout: &RibbonLayout,
        group: usize,
        control: usize,
    ) -> RibbonEvent {
        let Some(anchor) = find_slot(layout, group, control).map(|s| s.rect) else {
            return RibbonEvent::Handled;
        };
        let Some(Control::Split { command, menu, .. }) = self.control_at(group, control) else {
            return RibbonEvent::Handled;
        };
        if !command.enabled {
            return RibbonEvent::Handled;
        }
        let mut acts = vec![Act::Press(command.id)];
        let mut items = vec![
            MenuItem::Action {
                id: 0,
                label: command.label.clone(),
                shortcut: None,
                icon: command.icon.clone(),
                enabled: true,
                checked: None,
            },
            MenuItem::Separator,
        ];
        items.extend(split_rows(menu, command.id, &mut acts));
        let mut menu = ContextMenu::new(items);
        menu.set_min_width(anchor.w);
        menu.show(anchor.x, anchor.bottom(), layout.viewport);
        menu.highlight(0);
        self.menu = Some(OpenMenu { menu, acts });
        RibbonEvent::Handled
    }
}

/// A split button's menu, its rows renumbered to report through `acts`.
fn split_rows(rows: &[MenuItem], id: CommandId, acts: &mut Vec<Act>) -> Vec<MenuItem> {
    rows.iter()
        .map(|row| match row {
            MenuItem::Action {
                id: row,
                label,
                shortcut,
                icon,
                enabled,
                checked,
            } => {
                let n = acts.len() as MenuItemId;
                acts.push(Act::SplitRow { id, row: *row });
                MenuItem::Action {
                    id: n,
                    label: label.clone(),
                    shortcut: shortcut.clone(),
                    icon: icon.clone(),
                    enabled: *enabled,
                    checked: *checked,
                }
            }
            MenuItem::Separator => MenuItem::Separator,
            MenuItem::Submenu {
                label,
                icon,
                enabled,
                children,
                ..
            } => MenuItem::Submenu {
                id: SUBMENU_ROW,
                label: label.clone(),
                icon: icon.clone(),
                enabled: *enabled,
                children: split_rows(children, id, acts),
            },
        })
        .collect()
}

/// A dropdown's or gallery's choices as menu rows, the chosen one checked.
fn choice_rows(
    id: CommandId,
    choices: &[Choice],
    selected: Option<usize>,
    acts: &mut Vec<Act>,
) -> Vec<MenuItem> {
    choices
        .iter()
        .enumerate()
        .map(|(index, choice)| {
            let n = acts.len() as MenuItemId;
            acts.push(Act::Choose { id, index });
            MenuItem::Action {
                id: n,
                label: choice.label.clone(),
                shortcut: None,
                icon: choice.icon.clone(),
                enabled: true,
                checked: Some(selected == Some(index)),
            }
        })
        .collect()
}

/// A group's controls as menu rows, for the `»` button: a button or toggle a
/// row, a split button, a dropdown and a gallery each a submenu.
fn group_rows(group: &Group, acts: &mut Vec<Act>) -> Vec<MenuItem> {
    let mut rows = Vec::new();
    for control in &group.controls {
        let command = control.command();
        let press = |checked: Option<bool>, acts: &mut Vec<Act>| {
            let n = acts.len() as MenuItemId;
            acts.push(Act::Press(command.id));
            MenuItem::Action {
                id: n,
                label: command.label.clone(),
                shortcut: None,
                icon: command.icon.clone(),
                enabled: command.enabled,
                checked,
            }
        };
        match control {
            Control::Button { .. } => rows.push(press(None, acts)),
            Control::Toggle { on, .. } => rows.push(press(Some(*on), acts)),
            Control::Split { menu, .. } => {
                let mut children = vec![press(None, acts), MenuItem::Separator];
                children.extend(split_rows(menu, command.id, acts));
                rows.push(MenuItem::Submenu {
                    id: SUBMENU_ROW,
                    label: command.label.clone(),
                    icon: command.icon.clone(),
                    enabled: command.enabled,
                    children,
                });
            }
            Control::Dropdown {
                choices, selected, ..
            }
            | Control::Gallery {
                choices, selected, ..
            } => rows.push(MenuItem::Submenu {
                id: SUBMENU_ROW,
                label: command.label.clone(),
                icon: command.icon.clone(),
                enabled: command.enabled,
                children: choice_rows(command.id, choices, *selected, acts),
            }),
        }
    }
    rows
}

// ============================================================================
// Drawing
// ============================================================================

/// Draw the ribbon: the strip and its tabs, the front tab's groups, an open
/// panel over the page, and an open menu over everything. Pictures are found
/// through `icons`, which answers an image for a name at a size, as a menu's
/// are ([`ContextMenu::render_with_icons`]).
pub fn draw<S: CommandSink + ?Sized>(
    sink: &mut S,
    p: &Palette,
    ribbon: &Ribbon,
    layout: &RibbonLayout,
    icons: &dyn Fn(&str, u32) -> Option<u64>,
) {
    // The title bar's own colour: a ribbon under a title bar reads as one
    // piece of window.
    fill(sink, layout.strip, p.surface0, CornerRadii::ZERO);
    let front = ribbon.front();
    sink.emit(RenderCommand::PushClip {
        x: layout.strip.x,
        y: layout.strip.y,
        width: layout.strip.w,
        height: layout.strip.h,
    });
    for slot in &layout.tabs {
        draw_tab(sink, p, ribbon, slot, front == Some(slot.tab));
    }
    sink.emit(RenderCommand::PopClip);
    match &layout.body {
        Some(body) => draw_body(sink, p, ribbon, body, icons, false),
        // The strip's lower edge, where the body would begin.
        None => line(
            sink,
            layout.strip.x,
            layout.strip.bottom() - 0.5,
            layout.strip.right(),
            layout.strip.bottom() - 0.5,
            p.surface1,
        ),
    }
    if let Some(qat) = &layout.qat {
        draw_qat(sink, p, ribbon, qat, icons);
    }
    if let Some(panel) = &layout.panel {
        sink.emit(RenderCommand::BoxShadow {
            x: panel.rect.x,
            y: panel.rect.y,
            width: panel.rect.w,
            height: panel.rect.h,
            offset_x: 0.0,
            offset_y: 2.0,
            blur: 8.0,
            spread: 0.0,
            color: p.shadow(),
            corner_radii: CornerRadii::all(RADIUS),
        });
        draw_body(sink, p, ribbon, panel, icons, true);
    }
    if let Some(typed) = &ribbon.tips {
        // Only the tips that what has been typed still leads to.
        for tip in ribbon.key_tips(layout) {
            if tip.keys.starts_with(typed.as_str()) {
                draw_tip(sink, p, &tip);
            }
        }
    }
    if let Some(open) = &ribbon.menu {
        for command in open.menu.render_with_icons(p, icons) {
            sink.emit(command);
        }
    }
    if let Some(tip) = ribbon.tooltip() {
        for command in tip.render(p) {
            sink.emit(command);
        }
    }
}

/// The Quick Access Toolbar: over the ribbon, in the strip's colour, the
/// top of the one piece of window; under it, on the body's, a line above.
/// Each button its command's picture -- or, with none, its name's first
/// letter -- lit under the pointer, chosen when a toggle is on.
fn draw_qat<S: CommandSink + ?Sized>(
    sink: &mut S,
    p: &Palette,
    ribbon: &Ribbon,
    qat: &QatSlots,
    icons: &dyn Fn(&str, u32) -> Option<u64>,
) {
    if ribbon.custom.qat_below {
        fill(sink, qat.rect, p.base, CornerRadii::ZERO);
        line(
            sink,
            qat.rect.x,
            qat.rect.y + 0.5,
            qat.rect.right(),
            qat.rect.y + 0.5,
            p.surface1,
        );
    } else {
        fill(sink, qat.rect, p.surface0, CornerRadii::ZERO);
    }
    for &(i, r) in &qat.buttons {
        let Some(control) = ribbon
            .custom
            .qat
            .get(i)
            .and_then(|&id| ribbon.command_control(id))
        else {
            continue;
        };
        let command = control.command();
        if let Control::Toggle { on: true, .. } = control {
            fill(sink, r, p.selection_fill(), CornerRadii::all(RADIUS));
            stroke(sink, r, p.selection_border(), 1.0);
        }
        if command.enabled && ribbon.hover == Some(Hit::Qat(i)) {
            let pressed = ribbon.pressed == Some(Hit::Qat(i));
            fill(
                sink,
                r,
                if pressed { p.surface2 } else { p.surface1 },
                CornerRadii::all(RADIUS),
            );
        }
        let side = SMALL_ICON as f32;
        let drawn = command
            .icon
            .as_deref()
            .and_then(|name| icons(name, SMALL_ICON));
        if drawn.is_some() {
            picture(
                sink,
                p,
                command.icon.as_deref(),
                r.x + (r.w - side) / 2.0,
                r.y + (r.h - side) / 2.0,
                SMALL_ICON,
                command.enabled,
                icons,
            );
        } else {
            let initial: String = command.label.chars().take(1).collect();
            let w = label_width(&initial);
            label(
                sink,
                r.x + (r.w - w) / 2.0,
                text_top(r.y, r.h, LABEL_SIZE),
                &initial,
                if command.enabled { p.text } else { p.overlay0 },
                LABEL_SIZE,
                r.w,
            );
        }
    }
}

/// A key tip: its keys, capitals, on the accent, inside the foot of what it
/// reaches.
fn draw_tip<S: CommandSink + ?Sized>(sink: &mut S, p: &Palette, tip: &KeyTip) {
    let keys = tip.keys.to_uppercase();
    let width = text::measure(&keys, TIP_SIZE, FontWeightHint::Bold) + 2.0 * TIP_PAD;
    let badge = Rect::new(
        tip.rect.x + (tip.rect.w - width) / 2.0,
        tip.rect.bottom() - TIP_HEIGHT,
        width,
        TIP_HEIGHT,
    );
    fill(sink, badge, p.accent, CornerRadii::all(RADIUS));
    sink.emit(RenderCommand::Text {
        x: badge.x + TIP_PAD,
        y: text_top(badge.y, badge.h, TIP_SIZE),
        text: keys,
        color: p.on_accent(),
        font_size: TIP_SIZE,
        font_weight: FontWeightHint::Bold,
        max_width: Some(width),
        overflow: TextOverflow::Clip,
    });
}

fn fill<S: CommandSink + ?Sized>(sink: &mut S, r: Rect, color: Color, corner_radii: CornerRadii) {
    sink.emit(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii,
    });
}

fn stroke<S: CommandSink + ?Sized>(sink: &mut S, r: Rect, color: Color, width: f32) {
    sink.emit(RenderCommand::StrokeRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        line_width: width,
        corner_radii: CornerRadii::all(RADIUS),
    });
}

fn line<S: CommandSink + ?Sized>(sink: &mut S, x1: f32, y1: f32, x2: f32, y2: f32, color: Color) {
    sink.emit(RenderCommand::Line {
        x1,
        y1,
        x2,
        y2,
        color,
        width: 1.0,
    });
}

fn label<S: CommandSink + ?Sized>(
    sink: &mut S,
    x: f32,
    y: f32,
    words: &str,
    color: Color,
    size: f32,
    max_width: f32,
) {
    sink.emit(RenderCommand::Text {
        x,
        y,
        text: words.to_owned(),
        color,
        font_size: size,
        font_weight: FontWeightHint::Regular,
        max_width: Some(max_width.max(1.0)),
        overflow: TextOverflow::Ellipsis,
    });
}

/// A small `v`, centred on `cx`, `cy`: what opens a menu.
fn chevron<S: CommandSink + ?Sized>(sink: &mut S, cx: f32, cy: f32, color: Color) {
    line(sink, cx - 3.5, cy - 1.5, cx, cy + 2.0, color);
    line(sink, cx, cy + 2.0, cx + 3.5, cy - 1.5, color);
}

/// `icon` at `x`, `y`, `size` square, washed out when its control cannot be
/// used -- a picture has no faint colour of its own to be drawn in.
fn picture<S: CommandSink + ?Sized>(
    sink: &mut S,
    p: &Palette,
    icon: Option<&str>,
    x: f32,
    y: f32,
    size: u32,
    enabled: bool,
    icons: &dyn Fn(&str, u32) -> Option<u64>,
) {
    let Some(image_id) = icon.and_then(|name| icons(name, size)) else {
        return;
    };
    let side = size as f32;
    sink.emit(RenderCommand::Image {
        x,
        y,
        width: side,
        height: side,
        image_id,
    });
    if !enabled {
        fill(
            sink,
            Rect::new(x, y, side, side),
            Color::rgba(p.base.r, p.base.g, p.base.b, 160),
            CornerRadii::ZERO,
        );
    }
}

/// The top of a line of text `size` high, centred in a band `height` high
/// at `top`.
fn text_top(top: f32, height: f32, size: f32) -> f32 {
    top + (height - text::line_height(size, FontWeightHint::Regular)) / 2.0
}

fn draw_tab<S: CommandSink + ?Sized>(
    sink: &mut S,
    p: &Palette,
    ribbon: &Ribbon,
    slot: &TabSlot,
    front: bool,
) {
    let Some(tab) = ribbon.tabs.get(slot.tab) else {
        return;
    };
    let r = slot.rect;
    if front {
        // The front tab is the body's colour, joined to it.
        fill(sink, r, p.base, CornerRadii::top(RADIUS));
    } else if ribbon.hover == Some(Hit::Tab(slot.tab)) {
        fill(sink, r, p.surface1, CornerRadii::top(RADIUS));
    }
    if tab.context.is_some() {
        fill(
            sink,
            Rect::new(r.x, r.y, r.w, CONTEXT_BAND),
            p.accent,
            CornerRadii::top(RADIUS),
        );
    }
    label(
        sink,
        r.x + TAB_PAD,
        text_top(r.y, r.h, TAB_SIZE),
        &tab.label,
        p.text,
        TAB_SIZE,
        r.w - TAB_PAD,
    );
}

fn draw_body<S: CommandSink + ?Sized>(
    sink: &mut S,
    p: &Palette,
    ribbon: &Ribbon,
    body: &BodySlots,
    icons: &dyn Fn(&str, u32) -> Option<u64>,
    panel: bool,
) {
    fill(
        sink,
        body.rect,
        p.base,
        if panel {
            CornerRadii::all(RADIUS)
        } else {
            CornerRadii::ZERO
        },
    );
    if panel {
        stroke(sink, body.rect, p.border, 1.0);
    } else {
        line(
            sink,
            body.rect.x,
            body.rect.bottom() - 0.5,
            body.rect.right(),
            body.rect.bottom() - 0.5,
            p.surface1,
        );
    }
    for g in &body.groups {
        let Some(group) = ribbon.group(g.group) else {
            continue;
        };
        if let Some(button) = g.button {
            draw_folded(sink, p, ribbon, group, g.group, button, icons);
        } else {
            for c in &g.controls {
                if let Some(control) = group.controls.get(c.control) {
                    draw_control(sink, p, ribbon, g.group, control, c, icons);
                }
            }
            if let Some(name) = g.label {
                let w = text::measure(&group.label, GROUP_LABEL_SIZE, FontWeightHint::Regular);
                label(
                    sink,
                    name.x + ((name.w - w) / 2.0).max(GROUP_PAD),
                    text_top(name.y, name.h, GROUP_LABEL_SIZE),
                    &group.label,
                    p.subtext0,
                    GROUP_LABEL_SIZE,
                    name.w - 2.0 * GROUP_PAD,
                );
            }
        }
        if !panel {
            // The line between one group and the next.
            line(
                sink,
                g.rect.right() - 0.5,
                g.rect.y + CONTENT_TOP,
                g.rect.right() - 0.5,
                g.rect.bottom() - 4.0,
                p.surface1,
            );
        }
    }
    if let Some(overflow) = &body.overflow {
        if ribbon.hover == Some(Hit::Overflow) {
            fill(sink, overflow.rect, p.surface1, CornerRadii::all(RADIUS));
        }
        let size = LABEL_SIZE + 2.0;
        label(
            sink,
            overflow.rect.x
                + (overflow.rect.w - text::measure("»", size, FontWeightHint::Regular)) / 2.0,
            text_top(overflow.rect.y, overflow.rect.h, size),
            "»",
            p.text,
            size,
            overflow.rect.w,
        );
    }
}

fn draw_folded<S: CommandSink + ?Sized>(
    sink: &mut S,
    p: &Palette,
    ribbon: &Ribbon,
    group: &Group,
    index: usize,
    button: Rect,
    icons: &dyn Fn(&str, u32) -> Option<u64>,
) {
    let open = matches!(ribbon.panel, Some(Panel::Group { group: g, .. }) if g == index);
    if open {
        fill(sink, button, p.surface2, CornerRadii::all(RADIUS));
    } else if ribbon.hover == Some(Hit::Folded(index)) {
        fill(sink, button, p.surface1, CornerRadii::all(RADIUS));
    }
    let side = LARGE_ICON as f32;
    picture(
        sink,
        p,
        group.icon.as_deref(),
        button.x + (button.w - side) / 2.0,
        button.y + LARGE_TOP,
        LARGE_ICON,
        true,
        icons,
    );
    let mut y = button.y + LARGE_TOP + side + 3.0;
    let lh = text::line_height(LABEL_SIZE, FontWeightHint::Regular);
    for words in large_lines(&group.label) {
        let w = label_width(&words);
        label(
            sink,
            button.x + ((button.w - w) / 2.0).max(0.0),
            y,
            &words,
            p.text,
            LABEL_SIZE,
            button.w,
        );
        y += lh;
    }
    chevron(sink, button.x + button.w / 2.0, y + 4.0, p.subtext1);
}

/// The part of `hit` that is lit: the face or the arrow of the control at
/// `group`, `control`, if the pointer is over it.
fn lit(ribbon: &Ribbon, group: usize, control: usize) -> Option<Part> {
    match ribbon.hover {
        Some(Hit::Control {
            group: g,
            control: c,
            part,
        }) if g == group && c == control => Some(part),
        _ => None,
    }
}

fn draw_control<S: CommandSink + ?Sized>(
    sink: &mut S,
    p: &Palette,
    ribbon: &Ribbon,
    group: usize,
    control: &Control,
    slot: &ControlSlot,
    icons: &dyn Fn(&str, u32) -> Option<u64>,
) {
    let command = control.command();
    let enabled = command.enabled;
    let lit = if enabled {
        lit(ribbon, group, slot.control)
    } else {
        None
    };
    let pressed = ribbon.pressed.is_some_and(|h| {
        matches!(h, Hit::Control { group: g, control: c, .. } if g == group && c == slot.control)
    });
    let ink = if enabled { p.text } else { p.overlay0 };

    // The ground: the toggle that is on, the part under the pointer.
    if let Control::Toggle { on: true, .. } = control {
        fill(
            sink,
            slot.face,
            p.selection_fill(),
            CornerRadii::all(RADIUS),
        );
        stroke(sink, slot.face, p.selection_border(), 1.0);
    }
    match lit {
        Some(Part::Face)
            if !matches!(control, Control::Gallery { .. } | Control::Dropdown { .. }) =>
        {
            fill(
                sink,
                slot.face,
                if pressed { p.surface2 } else { p.surface1 },
                CornerRadii::all(RADIUS),
            );
            if let Some(arrow) = slot.arrow {
                stroke(sink, arrow, p.surface2, 1.0);
            }
        }
        Some(Part::Arrow) if !matches!(control, Control::Dropdown { .. }) => {
            if let Some(arrow) = slot.arrow {
                fill(sink, arrow, p.surface1, CornerRadii::all(RADIUS));
            }
            stroke(sink, slot.face, p.surface2, 1.0);
        }
        _ => {}
    }

    match control {
        Control::Button { size, .. }
        | Control::Toggle { size, .. }
        | Control::Split { size, .. } => {
            let split = matches!(control, Control::Split { .. });
            match size {
                ButtonSize::Large => {
                    let r = slot.rect;
                    let side = LARGE_ICON as f32;
                    picture(
                        sink,
                        p,
                        command.icon.as_deref(),
                        r.x + (r.w - side) / 2.0,
                        r.y + LARGE_TOP,
                        LARGE_ICON,
                        enabled,
                        icons,
                    );
                    let lh = text::line_height(LABEL_SIZE, FontWeightHint::Regular);
                    let mut y = r.y + LARGE_TOP + side + 3.0;
                    let lines = large_lines(&command.label);
                    let count = lines.len();
                    for (n, words) in lines.into_iter().enumerate() {
                        let w = label_width(&words);
                        // A split button's arrow follows its last line.
                        let with_arrow = split && n.saturating_add(1) == count;
                        let total = if with_arrow { w + ARROW_WIDTH } else { w };
                        let x = r.x + ((r.w - total) / 2.0).max(0.0);
                        label(sink, x, y, &words, ink, LABEL_SIZE, r.w);
                        if with_arrow {
                            chevron(sink, x + w + ARROW_WIDTH / 2.0, y + lh / 2.0, ink);
                        }
                        y += lh;
                    }
                }
                ButtonSize::Medium | ButtonSize::Small => {
                    let side = SMALL_ICON as f32;
                    let r = slot.face;
                    picture(
                        sink,
                        p,
                        command.icon.as_deref(),
                        r.x + BUTTON_PAD,
                        r.y + (r.h - side) / 2.0,
                        SMALL_ICON,
                        enabled,
                        icons,
                    );
                    if *size == ButtonSize::Medium {
                        let x = r.x + BUTTON_PAD + side + ICON_GAP;
                        label(
                            sink,
                            x,
                            text_top(r.y, r.h, LABEL_SIZE),
                            &command.label,
                            ink,
                            LABEL_SIZE,
                            r.right() - x,
                        );
                    }
                    if let Some(arrow) = slot.arrow {
                        chevron(sink, arrow.x + arrow.w / 2.0, arrow.y + arrow.h / 2.0, ink);
                    }
                }
            }
        }
        Control::Dropdown {
            choices, selected, ..
        } => {
            let r = slot.rect;
            fill(sink, r, p.mantle, CornerRadii::all(RADIUS));
            stroke(
                sink,
                r,
                if lit.is_some() { p.accent } else { p.border },
                1.0,
            );
            let (words, color) = match selected.and_then(|i| choices.get(i)) {
                Some(choice) => (choice.label.as_str(), ink),
                // The dropdown's own name, as a field's placeholder is drawn.
                None => (command.label.as_str(), p.subtext0),
            };
            label(
                sink,
                r.x + 6.0,
                text_top(r.y, r.h, LABEL_SIZE),
                words,
                if enabled { color } else { p.overlay0 },
                LABEL_SIZE,
                r.w - 6.0 - ARROW_WIDTH,
            );
            chevron(sink, r.right() - ARROW_WIDTH / 2.0, r.y + r.h / 2.0, ink);
        }
        Control::Gallery {
            choices, selected, ..
        } => {
            stroke(sink, slot.rect, p.surface1, 1.0);
            let lh = text::line_height(CHOICE_SIZE, FontWeightHint::Regular);
            for &(i, cell) in &slot.choices {
                let Some(choice) = choices.get(i) else {
                    continue;
                };
                if lit == Some(Part::Choice(i)) {
                    fill(sink, cell, p.surface1, CornerRadii::all(RADIUS));
                }
                if *selected == Some(i) {
                    stroke(sink, cell, p.accent, 2.0);
                }
                let side = LARGE_ICON as f32;
                picture(
                    sink,
                    p,
                    choice.icon.as_deref(),
                    cell.x + (cell.w - side) / 2.0,
                    cell.y + 4.0,
                    LARGE_ICON,
                    enabled,
                    icons,
                );
                let w = text::measure(&choice.label, CHOICE_SIZE, FontWeightHint::Regular);
                label(
                    sink,
                    cell.x + ((cell.w - w) / 2.0).max(2.0),
                    cell.bottom() - lh - 3.0,
                    &choice.label,
                    ink,
                    CHOICE_SIZE,
                    cell.w - 4.0,
                );
            }
            if let Some(arrow) = slot.arrow {
                if lit == Some(Part::Arrow) {
                    fill(sink, arrow, p.surface1, CornerRadii::all(RADIUS));
                }
                line(
                    sink,
                    arrow.x + 0.5,
                    arrow.y + 2.0,
                    arrow.x + 0.5,
                    arrow.bottom() - 2.0,
                    p.surface1,
                );
                chevron(sink, arrow.x + arrow.w / 2.0, arrow.y + arrow.h / 2.0, ink);
            }
        }
    }
}

#[cfg(test)]
#[path = "ribbon_tests.rs"]
mod tests;
