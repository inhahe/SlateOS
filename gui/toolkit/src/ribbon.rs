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
use crate::menu::{ContextMenu, MenuAction, MenuItem, MenuItemId};
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
    /// The ribbon, or a panel it opened, where nothing is.
    Blank,
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

/// What a row of a menu the ribbon built does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    /// Press a control's face: a button's, a toggle's, a split button's.
    Press { group: usize, control: usize },
    /// A split button's own menu row.
    SplitRow { id: CommandId, row: MenuItemId },
    /// Choose a dropdown's or a gallery's choice.
    Choose {
        group: usize,
        control: usize,
        index: usize,
    },
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

/// A ribbon: its tabs, which is in front, which contexts are active, whether
/// it is minimized, and what the pointer is doing to it.
pub struct Ribbon {
    tabs: Vec<RibbonTab>,
    /// The front tab's id. Kept by name rather than place, so that a tab
    /// appearing or going before it leaves the same tab in front.
    front: String,
    contexts: BTreeSet<String>,
    minimized: bool,
    hover: Option<Hit>,
    pressed: Option<Hit>,
    panel: Option<Panel>,
    menu: Option<OpenMenu>,
}

impl Ribbon {
    /// A ribbon of `tabs`, the first ordinary one in front.
    #[must_use]
    pub fn new(tabs: Vec<RibbonTab>) -> Self {
        let mut ribbon = Self {
            tabs,
            front: String::new(),
            contexts: BTreeSet::new(),
            minimized: false,
            hover: None,
            pressed: None,
            panel: None,
            menu: None,
        };
        ribbon.front = ribbon
            .visible_tabs()
            .first()
            .and_then(|&i| ribbon.tabs.get(i))
            .map_or_else(String::new, |t| t.id.clone());
        ribbon
    }

    /// The tabs, as given.
    #[must_use]
    pub fn tabs(&self) -> &[RibbonTab] {
        &self.tabs
    }

    /// Put `tabs` in place of the ribbon's. The front tab stays in front if
    /// it is still there; anything open is closed, since it named a place in
    /// the tabs that were.
    pub fn set_tabs(&mut self, tabs: Vec<RibbonTab>) {
        self.tabs = tabs;
        self.close();
        self.hover = None;
        self.pressed = None;
        self.keep_front_visible();
    }

    /// The indices of the tabs shown, in the order they are shown: the
    /// ordinary ones, then those of the active contexts.
    #[must_use]
    pub fn visible_tabs(&self) -> Vec<usize> {
        let ordinary = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| t.context.is_none());
        let contextual = self.tabs.iter().enumerate().filter(|(_, t)| {
            t.context
                .as_ref()
                .is_some_and(|context| self.contexts.contains(context))
        });
        ordinary.chain(contextual).map(|(i, _)| i).collect()
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
        self.minimized
    }

    /// Show the tabs alone, or the tabs and the front tab's commands.
    pub fn set_minimized(&mut self, minimized: bool) {
        self.minimized = minimized;
        self.close();
    }

    /// Enable or disable every control of `id`; a reason says why it cannot
    /// be used. Answers whether any control has that command.
    pub fn set_enabled(&mut self, id: CommandId, enabled: bool, reason: Option<&str>) -> bool {
        self.each_control(id, |control| {
            let command = control.command_mut();
            command.enabled = enabled;
            command.disabled_reason = if enabled {
                None
            } else {
                reason.map(str::to_owned)
            };
        })
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
    }

    fn each_control(&mut self, id: CommandId, mut f: impl FnMut(&mut Control)) -> bool {
        let mut any = false;
        for control in self
            .tabs
            .iter_mut()
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

    fn control_at_mut(&mut self, group: usize, control: usize) -> Option<&mut Control> {
        let front = self.front()?;
        self.tabs
            .get_mut(front)?
            .groups
            .get_mut(group)?
            .controls
            .get_mut(control)
    }
}

// ============================================================================
// Geometry
// ============================================================================

/// Where everything is, for one width: what [`Ribbon::layout`] answers, what
/// input is tested against and what [`draw`] draws.
#[derive(Clone, Debug, PartialEq)]
pub struct RibbonLayout {
    /// The room the ribbon takes at the top of the window: the strip, and
    /// the body unless it is minimized. The page goes under it.
    pub rect: Rect,
    /// The window, which a menu the ribbon opens is kept inside.
    pub viewport: (f32, f32),
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
        let strip = Rect::new(x, y, width, STRIP_HEIGHT);
        let tabs = self.lay_tabs(strip);
        let body_rect = Rect::new(x, strip.bottom(), width, BODY_HEIGHT);
        let front = self.front();
        let body = if self.minimized {
            None
        } else {
            front.map(|t| self.lay_body(t, body_rect))
        };
        let rect = Rect::new(
            x,
            y,
            width,
            if self.minimized {
                STRIP_HEIGHT
            } else {
                STRIP_HEIGHT + BODY_HEIGHT
            },
        );
        let panel = match self.panel {
            Some(Panel::Tab) if self.minimized => front.map(|t| self.lay_body(t, body_rect)),
            Some(Panel::Group { group, anchor }) => self.lay_group_panel(group, anchor, rect),
            _ => None,
        };
        RibbonLayout {
            rect,
            viewport,
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
        match &event.kind {
            MouseEventKind::Move | MouseEventKind::Enter => {
                self.hover = hit;
                if hit.is_some() || self.panel.is_some() {
                    RibbonEvent::Handled
                } else {
                    RibbonEvent::Ignored
                }
            }
            MouseEventKind::Leave => {
                self.hover = None;
                RibbonEvent::Handled
            }
            MouseEventKind::Press(MouseButton::Left) => self.press(layout, hit),
            MouseEventKind::Release(MouseButton::Left) => self.release(hit),
            MouseEventKind::DoubleClick(MouseButton::Left) => match hit {
                Some(Hit::Tab(_)) => {
                    self.set_minimized(!self.minimized);
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
    fn press(&mut self, layout: &RibbonLayout, hit: Option<Hit>) -> RibbonEvent {
        let in_panel = matches!(
            hit,
            Some(Hit::Control { .. } | Hit::Folded(_) | Hit::Blank | Hit::Overflow)
        ) && layout
            .panel
            .as_ref()
            .is_some_and(|p| self.pointer_in(p, hit));
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

    /// Whether `hit` came from inside `panel` -- which, the hit having been
    /// found there first when the pointer was over it, is whether the panel
    /// holds the thing hit.
    fn pointer_in(&self, panel: &BodySlots, hit: Option<Hit>) -> bool {
        match hit {
            Some(Hit::Control { group, .. } | Hit::Folded(group)) => {
                panel.groups.iter().any(|g| g.group == group)
                    && !matches!(self.panel, Some(Panel::Group { group: open, .. }) if open != group)
            }
            Some(Hit::Overflow) => panel.overflow.is_some(),
            Some(Hit::Blank) => true,
            _ => false,
        }
    }

    fn press_tab(&mut self, tab: usize) -> RibbonEvent {
        let Some(id) = self.tabs.get(tab).map(|t| t.id.clone()) else {
            return RibbonEvent::Handled;
        };
        let changed = self.front != id;
        self.front = id.clone();
        self.menu = None;
        if self.minimized {
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
            (_, None) => RibbonEvent::Ignored,
            _ => RibbonEvent::Handled,
        }
    }

    /// Do what pressing `part` of the control at `group`, `control` does.
    fn activate(&mut self, group: usize, control: usize, part: Part) -> RibbonEvent {
        let Some(c) = self.control_at_mut(group, control) else {
            return RibbonEvent::Handled;
        };
        if !c.command().enabled {
            return RibbonEvent::Handled;
        }
        let event = match (c, part) {
            (Control::Button { command, .. } | Control::Split { command, .. }, Part::Face) => {
                RibbonEvent::Command(command.id)
            }
            (Control::Toggle { command, on, .. }, Part::Face) => {
                *on = !*on;
                RibbonEvent::Toggled {
                    id: command.id,
                    on: *on,
                }
            }
            (
                Control::Gallery {
                    command,
                    choices,
                    selected,
                    ..
                },
                Part::Choice(i),
            ) if i < choices.len() => {
                *selected = Some(i);
                RibbonEvent::Chose {
                    id: command.id,
                    index: i,
                }
            }
            _ => RibbonEvent::Handled,
        };
        // A command given is the end of the panel it was given from.
        if event != RibbonEvent::Handled {
            self.panel = None;
        }
        event
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
        let Some(c) = self.control_at(group, control) else {
            return RibbonEvent::Handled;
        };
        if !c.command().enabled {
            return RibbonEvent::Handled;
        }
        let mut acts = Vec::new();
        let items = match c {
            Control::Split { command, menu, .. } => split_rows(menu, command.id, &mut acts),
            Control::Dropdown {
                choices, selected, ..
            }
            | Control::Gallery {
                choices, selected, ..
            } => choice_rows(group, control, choices, *selected, &mut acts),
            Control::Button { .. } | Control::Toggle { .. } => return RibbonEvent::Handled,
        };
        let mut menu = ContextMenu::new(items);
        // A list at least as wide as the box it drops from.
        menu.set_min_width(anchor.w);
        menu.show(anchor.x, anchor.bottom(), layout.viewport);
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
                children: group_rows(g, group, &mut acts),
            });
        }
        let mut menu = ContextMenu::new(items);
        menu.show(overflow.rect.x, overflow.rect.bottom(), layout.viewport);
        self.menu = Some(OpenMenu { menu, acts });
        RibbonEvent::Handled
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
                        .and_then(|i| open.acts.get(i).copied())
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
        match act {
            Act::Press { group, control } => self.activate(group, control, Part::Face),
            Act::SplitRow { id, row } => {
                self.panel = None;
                RibbonEvent::MenuItem { id, item: row }
            }
            Act::Choose {
                group,
                control,
                index,
            } => {
                let Some(
                    Control::Dropdown {
                        command,
                        choices,
                        selected,
                        ..
                    }
                    | Control::Gallery {
                        command,
                        choices,
                        selected,
                        ..
                    },
                ) = self.control_at_mut(group, control)
                else {
                    return RibbonEvent::Handled;
                };
                if index >= choices.len() {
                    return RibbonEvent::Handled;
                }
                *selected = Some(index);
                let id = command.id;
                self.panel = None;
                RibbonEvent::Chose { id, index }
            }
        }
    }

    /// Handle a key: the open menu's while one is open; Ctrl+F1 minimizes the
    /// ribbon or brings it back; Escape closes an open panel.
    pub fn handle_key(&mut self, key: &KeyEvent) -> RibbonEvent {
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
                        .and_then(|i| open.acts.get(i).copied());
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
        match key.key {
            Key::F1 if key.modifiers.ctrl => {
                self.set_minimized(!self.minimized);
                RibbonEvent::Customized
            }
            Key::Escape if self.panel.is_some() => {
                self.panel = None;
                RibbonEvent::Handled
            }
            _ => RibbonEvent::Ignored,
        }
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
    group: usize,
    control: usize,
    choices: &[Choice],
    selected: Option<usize>,
    acts: &mut Vec<Act>,
) -> Vec<MenuItem> {
    choices
        .iter()
        .enumerate()
        .map(|(index, choice)| {
            let n = acts.len() as MenuItemId;
            acts.push(Act::Choose {
                group,
                control,
                index,
            });
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
fn group_rows(g: usize, group: &Group, acts: &mut Vec<Act>) -> Vec<MenuItem> {
    let mut rows = Vec::new();
    for (c, control) in group.controls.iter().enumerate() {
        let command = control.command();
        let press = |checked: Option<bool>, acts: &mut Vec<Act>| {
            let n = acts.len() as MenuItemId;
            acts.push(Act::Press {
                group: g,
                control: c,
            });
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
                children: choice_rows(g, c, choices, *selected, acts),
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
    if let Some(open) = &ribbon.menu {
        for command in open.menu.render_with_icons(p, icons) {
            sink.emit(command);
        }
    }
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
