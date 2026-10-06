//! The widget tree as automation and assistive tools see it, and what they
//! may do to it: [`WidgetTree::automation`], [`WidgetTree::find`] and
//! [`WidgetTree::invoke`].
//!
//! # Why
//!
//! `roadmap-detailed.md` (the automation framework's *Automatic Widget-Level
//! Exposure*) asks that every interactive widget the toolkit builds expose
//! itself -- "a live, walkable tree of the app's widgets (an
//! accessibility/UI-automation tree, à la Windows UIAutomation / AT-SPI /
//! macOS AX)" -- so that a script, a recorded macro or a screen reader can
//! find "the *Save* button" and press it in a program whose author wrote no
//! automation code at all. A program opts a widget out
//! ([`Widget::hidden_from_automation`]); it never has to opt one in.
//!
//! This is the toolkit's half: the tree, a search over it, and the actions,
//! in the program's own process. Carrying them to another process -- over
//! the automation channel, behind the `automation.ui_inspect` and
//! `automation.ui_control` rights -- is the automation library's, which reads
//! [`WidgetTree::automation`] and calls [`WidgetTree::invoke`].
//!
//! # A node
//!
//! [`Node`] says what a widget is ([`Role`]); what it is called -- its
//! *name*: what its program called it ([`Widget::labelled`]), else a
//! button's, a label's or a box's own text, a field's placeholder, else its
//! tooltip; its tooltip as its description; what it holds ([`Value`]);
//! whether it can be used, is shown, has the keyboard and could take it; its
//! box in the window; its program's name for it -- the CSS `#name`
//! ([`Widget::named`]), which a script can find from one run to the next as
//! it cannot a [`WidgetId`]; and the nodes of what it holds.
//!
//! A widget is shown only where all that holds it is, and usable only where
//! all that holds it is enabled, as its events are delivered: a disabled
//! panel's button takes no click, and its node says so.
//!
//! # As the user would
//!
//! An action goes through the widget as a click or a key does. A button
//! pressed signals [`SignalKind::Clicked`]; a box toggled signals what it is
//! now; a radio button chosen clears the rest of its group; text set is an
//! edit -- a text area's undoable, as a paste is; a slider set lands on its
//! steps, within its bounds. Then, as after any event, the tree styles itself
//! again for the states a style hangs on (`:checked`, `:focus`) and hands its
//! program the signals. A widget its user could not use -- hidden or
//! disabled, itself or by what holds it -- refuses ([`Refusal`]).
//!
//! # What is never shown
//!
//! A secret -- a password field's text -- is never in a node, whatever the
//! tool's rights: the toolkit's secret field (`crate::secretinput`) is not a
//! widget of the tree, and a node carries no value it could take from one.
//!
//! # Components drawn outside the tree
//!
//! A component that draws itself through [`crate::frame`] -- a picker, a
//! dialog -- shows tools its parts by implementing [`Accessible`]: nodes
//! named by its own hit-box targets, and actions that answer its own
//! events, which its host acts on as it acts on a click. [`Query::find_in`]
//! searches its nodes as [`WidgetTree::find`] searches a tree's.

use super::{CheckState, SignalKind, Widget, WidgetId, WidgetKind, WidgetTree};
use crate::frame::Rect;
use crate::text::TextCursor;

/// What a widget is, to a tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// A panel holding others.
    Group,
    /// Text that names or explains.
    Label,
    /// A button.
    Button,
    /// A one-line text field.
    TextField,
    /// A text area of several lines.
    TextArea,
    /// A checkbox.
    CheckBox,
    /// One of a group of choices.
    RadioButton,
    /// What it holds, scrolled.
    ScrollPane,
    /// A line between parts.
    Separator,
    /// A progress bar.
    ProgressBar,
    /// A value between two bounds.
    Slider,
    /// A picture.
    Image,
    /// A dialog: a component's whole, its parts its children.
    Dialog,
    /// A list of items, one of which may be chosen.
    List,
    /// One item of a list.
    ListItem,
    /// A grid of cells.
    Grid,
    /// One cell of a grid.
    GridCell,
    /// A menu: a context menu, or a submenu opened from one of its rows.
    Menu,
    /// One row of a menu: an action, a check, or a row a submenu opens
    /// from.
    MenuItem,
}

impl Role {
    /// The role of a widget of `kind`.
    const fn of(kind: &WidgetKind) -> Self {
        match kind {
            WidgetKind::Container => Self::Group,
            WidgetKind::Label { .. } => Self::Label,
            WidgetKind::Button { .. } => Self::Button,
            WidgetKind::TextInput { .. } => Self::TextField,
            WidgetKind::TextArea { .. } => Self::TextArea,
            WidgetKind::Checkbox { .. } => Self::CheckBox,
            WidgetKind::RadioButton { .. } => Self::RadioButton,
            WidgetKind::ScrollView { .. } => Self::ScrollPane,
            WidgetKind::Separator { .. } => Self::Separator,
            WidgetKind::ProgressBar { .. } => Self::ProgressBar,
            WidgetKind::Slider { .. } => Self::Slider,
            WidgetKind::Image { .. } => Self::Image,
        }
    }

    /// Its name as a tool says it: "check box".
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Group => "group",
            Self::Label => "label",
            Self::Button => "button",
            Self::TextField => "text field",
            Self::TextArea => "text area",
            Self::CheckBox => "check box",
            Self::RadioButton => "radio button",
            Self::ScrollPane => "scroll pane",
            Self::Separator => "separator",
            Self::ProgressBar => "progress bar",
            Self::Slider => "slider",
            Self::Image => "image",
            Self::Dialog => "dialog",
            Self::List => "list",
            Self::ListItem => "list item",
            Self::Grid => "grid",
            Self::GridCell => "grid cell",
            Self::Menu => "menu",
            Self::MenuItem => "menu item",
        }
    }
}

/// What a widget holds.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A field's or a text area's text.
    Text(String),
    /// A checkbox's tick.
    Check(CheckState),
    /// Whether a radio button is the one of its group chosen.
    Chosen(bool),
    /// A slider's value, and its bounds.
    Range {
        /// Where it is.
        value: f64,
        /// Its least.
        min: f64,
        /// Its most.
        max: f64,
    },
    /// A progress bar's progress, of its whole.
    Progress {
        /// How far.
        value: f32,
        /// Of how much.
        max: f32,
    },
}

/// A widget, as a tool sees it -- see the module's "A node" -- or a part of
/// a component drawn outside the tree ([`Accessible`]), named by `Id`: a
/// widget's [`WidgetId`], or the component's own name for its part.
#[derive(Clone, Debug, PartialEq)]
pub struct Node<Id = WidgetId> {
    /// Its id in this run of its program.
    pub id: Id,
    /// What it is.
    pub role: Role,
    /// What it is called.
    pub name: String,
    /// Its program's name for it -- the CSS `#name` -- the same from one
    /// run to the next.
    pub key: Option<String>,
    /// Its tooltip.
    pub description: Option<String>,
    /// What it holds.
    pub value: Option<Value>,
    /// Whether it can be used: it and all that holds it enabled.
    pub enabled: bool,
    /// Whether it is shown: it and all that holds it visible.
    pub shown: bool,
    /// Whether it has the keyboard.
    pub focused: bool,
    /// Whether it could take the keyboard.
    pub focusable: bool,
    /// Its box in the window -- its border box, where it is drawn.
    pub bounds: Rect,
    /// What it holds, in order.
    pub children: Vec<Node<Id>>,
}

impl<Id> Node<Id> {
    /// A node with nothing but what it is, what it is called and where: no
    /// key, description or value, enabled and shown, without the keyboard
    /// and unable to take it, holding nothing -- for a component naming its
    /// parts ([`Accessible`]) to fill in what else is so.
    #[must_use]
    pub fn new(id: Id, role: Role, name: impl Into<String>, bounds: Rect) -> Self {
        Self {
            id,
            role,
            name: name.into(),
            key: None,
            description: None,
            value: None,
            enabled: true,
            shown: true,
            focused: false,
            focusable: false,
            bounds,
            children: Vec::new(),
        }
    }

    /// It and every node under it, each named by `rename` of its id instead:
    /// for a component that holds another, naming the held one's parts as
    /// parts of its own -- the shell's notification pane, say, among the
    /// shell's.
    #[must_use]
    pub fn map<J>(self, rename: &impl Fn(Id) -> J) -> Node<J> {
        Node {
            id: rename(self.id),
            role: self.role,
            name: self.name,
            key: self.key,
            description: self.description,
            value: self.value,
            enabled: self.enabled,
            shown: self.shown,
            focused: self.focused,
            focusable: self.focusable,
            bounds: self.bounds,
            children: self
                .children
                .into_iter()
                .map(|child| child.map(rename))
                .collect(),
        }
    }

    /// It and every node under it, depth first, in order.
    pub fn walk(&self) -> impl Iterator<Item = &Self> {
        let mut stack = vec![self];
        std::iter::from_fn(move || {
            let node = stack.pop()?;
            stack.extend(node.children.iter().rev());
            Some(node)
        })
    }
}

/// What a tool asks of a widget.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Click a button.
    Press,
    /// Tick a checkbox, or untick it: a box in its middle state is ticked,
    /// as a click ticks it.
    Toggle,
    /// Choose a radio button.
    Choose,
    /// Make a field's or a text area's text this.
    SetText(String),
    /// Move a slider to this value: within its bounds, on its steps.
    SetValue(f64),
    /// Give a widget the keyboard.
    Focus,
    /// Scroll a scroll pane to this far across and down, as far as it goes.
    ScrollTo {
        /// How far across.
        x: f32,
        /// How far down.
        y: f32,
    },
}

impl Action {
    /// Its name, as a refusal says it: "press", "set the text of".
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Press => "press",
            Self::Toggle => "toggle",
            Self::Choose => "choose",
            Self::SetText(_) => "set the text of",
            Self::SetValue(_) => "set the value of",
            Self::Focus => "focus",
            Self::ScrollTo { .. } => "scroll",
        }
    }
}

/// Why an action was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// No widget in the tree that tools see has the id.
    NoSuchWidget,
    /// It, or what holds it, is hidden: its user could not reach it.
    Hidden,
    /// It, or what holds it, is disabled: its user could not use it.
    Disabled,
    /// It is not a widget the action is for: a label is not pressed.
    NotApplicable {
        /// What it is.
        role: Role,
        /// What was asked of it.
        action: &'static str,
    },
    /// The value asked for is no number.
    NotANumber,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchWidget => f.write_str("no such widget"),
            Self::Hidden => f.write_str("the widget is hidden"),
            Self::Disabled => f.write_str("the widget is disabled"),
            Self::NotApplicable { role, action } => {
                write!(f, "cannot {action} a {}", role.name())
            }
            Self::NotANumber => f.write_str("the value is not a number"),
        }
    }
}

impl std::error::Error for Refusal {}

/// What a search asks: every part of it given must fit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    /// What it is.
    pub role: Option<Role>,
    /// What it is called, whatever the case.
    pub name: Option<String>,
    /// Its program's name for it, exactly.
    pub key: Option<String>,
    /// Text its value holds, whatever the case.
    pub text: Option<String>,
    /// Whether it could take the keyboard: what a tool walking the controls
    /// as Tab walks them asks for.
    pub focusable: Option<bool>,
}

impl Query {
    /// The ids of every node under and including `tree` that fits, in
    /// order: the search over a component's parts ([`Accessible`]), as
    /// [`WidgetTree::find`] is over a tree's widgets.
    #[must_use]
    pub fn find_in<Id: Clone>(&self, tree: &Node<Id>) -> Vec<Id> {
        tree.walk()
            .filter(|node| self.fits(node))
            .map(|node| node.id.clone())
            .collect()
    }

    /// Whether `node` fits.
    fn fits<Id>(&self, node: &Node<Id>) -> bool {
        let same = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
        self.role.is_none_or(|role| role == node.role)
            && self
                .name
                .as_deref()
                .is_none_or(|name| same(name, &node.name))
            && self
                .key
                .as_deref()
                .is_none_or(|key| node.key.as_deref() == Some(key))
            && self.text.as_deref().is_none_or(|text| match &node.value {
                Some(Value::Text(held)) => held.to_lowercase().contains(&text.to_lowercase()),
                _ => false,
            })
            && self.focusable.is_none_or(|f| f == node.focusable)
    }
}

/// A component drawn outside the widget tree -- through [`crate::frame`],
/// as the font and character pickers are -- that shows tools its parts and
/// acts on them as its user would, as a [`WidgetTree`] does its widgets.
///
/// `roadmap-detailed.md`'s "custom-drawn or canvas widgets can supply their
/// own nodes via a toolkit hook so they aren't invisible to automation" is
/// this. A part is named by `Part` -- the target its frame's hit boxes
/// already name it by -- and an action on it answers the component's own
/// event, the one its host acts on when the user clicks the same part.
pub trait Accessible {
    /// What names a part: the component's hit-box target.
    type Part: Clone + PartialEq + std::fmt::Debug;
    /// What the component says happened -- a pick, a choice kept.
    type Event;

    /// Its parts as nodes, the component drawn `width` by `height` in its
    /// own space: a host that draws it elsewhere moves the boxes by where.
    fn automation(&self, width: f32, height: f32) -> Node<Self::Part>;

    /// Do `action` to `part`, the component drawn `width` by `height`, as
    /// its user would; answers what the component says of it, for its host
    /// to act on as it does the same from a click.
    ///
    /// # Errors
    ///
    /// [`Refusal`]: no such part, one its user could not reach or use, one
    /// the action is not for, or a value that is no number. A refused
    /// action changes nothing.
    fn invoke(
        &mut self,
        part: &Self::Part,
        action: Action,
        width: f32,
        height: f32,
    ) -> Result<Option<Self::Event>, Refusal>;
}

impl Widget {
    /// What a tool calls it: what its program said, else its own text, a
    /// field's placeholder, else its tooltip.
    fn accessible_name(&self) -> String {
        if let Some(label) = &self.label {
            return label.clone();
        }
        let own = match &self.kind {
            WidgetKind::Label { text } | WidgetKind::Button { text, .. } => text.as_str(),
            WidgetKind::Checkbox { label, .. } | WidgetKind::RadioButton { label, .. } => {
                label.as_str()
            }
            WidgetKind::TextInput { placeholder, .. }
            | WidgetKind::TextArea { placeholder, .. } => placeholder.as_str(),
            _ => "",
        };
        if own.is_empty() {
            self.tooltip.clone().unwrap_or_default()
        } else {
            own.to_string()
        }
    }

    /// What it holds.
    fn held(&self) -> Option<Value> {
        Some(match &self.kind {
            WidgetKind::TextInput { value, .. } => Value::Text(value.clone()),
            WidgetKind::TextArea { area, .. } => Value::Text(area.text().to_string()),
            WidgetKind::Checkbox { checked, .. } => Value::Check(*checked),
            WidgetKind::RadioButton { selected, .. } => Value::Chosen(*selected),
            WidgetKind::Slider { slider } => Value::Range {
                value: slider.value(),
                min: slider.min(),
                max: slider.max(),
            },
            WidgetKind::ProgressBar { value, max } => Value::Progress {
                value: *value,
                max: *max,
            },
            _ => return None,
        })
    }

    /// Its node, laid out in a space whose origin is `space` in the window,
    /// held by what is `enabled` and `shown` -- or `None` where its program
    /// keeps it from tools.
    fn node(&self, space: (f32, f32), enabled: bool, shown: bool) -> Option<Node> {
        if !self.exposed {
            return None;
        }
        let enabled = enabled && self.enabled;
        let shown = shown && self.visible;
        // A fixed widget is placed in the window, whatever holds it.
        let space = if self.is_fixed() { (0.0, 0.0) } else { space };
        let bounds = Rect::new(
            space.0 + self.layout.x + self.layout.margin.left,
            space.1 + self.layout.y + self.layout.margin.top,
            self.layout.border_box_width(),
            self.layout.border_box_height(),
        );
        let (cx, cy) = self.children_origin();
        let inner = (space.0 + cx, space.1 + cy);
        Some(Node {
            id: self.id,
            role: Role::of(&self.kind),
            name: self.accessible_name(),
            key: self.name.clone(),
            description: self.tooltip.clone(),
            value: self.held(),
            enabled,
            shown,
            focused: self.focused,
            focusable: enabled && shown && self.accepts_focus(),
            bounds,
            children: self
                .children
                .iter()
                .filter_map(|child| child.node(inner, enabled, shown))
                .collect(),
        })
    }

    /// Where the widget `id` is among what this holds, as child indices
    /// from here, and whether it and all between are enabled and shown --
    /// or `None` where it is not here, or kept from tools.
    fn reach(&self, id: WidgetId) -> Option<(Vec<usize>, bool, bool)> {
        if !self.exposed {
            return None;
        }
        if self.id == id {
            return Some((Vec::new(), self.enabled, self.visible));
        }
        self.children.iter().enumerate().find_map(|(index, child)| {
            let (mut path, enabled, shown) = child.reach(id)?;
            path.insert(0, index);
            Some((path, enabled && self.enabled, shown && self.visible))
        })
    }

    /// Do `action` to this widget, as its user would. `Focus` and
    /// `Choose`'s group are the tree's, and are done there.
    fn act(&mut self, action: &Action) -> Result<(), Refusal> {
        let role = Role::of(&self.kind);
        let not_for = || Refusal::NotApplicable {
            role,
            action: action.name(),
        };
        match (action, &mut self.kind) {
            (Action::Press, WidgetKind::Button { pressed, .. }) => {
                *pressed = false;
                self.signals.push(SignalKind::Clicked);
            }
            (Action::Toggle, WidgetKind::Checkbox { checked, .. }) => {
                *checked = checked.toggled();
                self.signals.push(SignalKind::Toggled(*checked));
            }
            (Action::Choose, WidgetKind::RadioButton { selected, .. }) => {
                if !*selected {
                    self.signals.push(SignalKind::Chosen);
                }
                *selected = true;
            }
            (
                Action::SetText(text),
                WidgetKind::TextInput {
                    value,
                    cursor,
                    selection_anchor,
                    ..
                },
            ) => {
                *selection_anchor = None;
                if value != text {
                    value.clone_from(text);
                    self.signals.push(SignalKind::Edited);
                }
                *cursor = TextCursor::from(value.len());
            }
            (Action::SetText(text), WidgetKind::TextArea { area, .. }) => {
                if area.text() != text {
                    // Over everything, as a paste over a selection of it all
                    // is: one edit, which Ctrl+Z takes back.
                    area.select_all();
                    area.insert_str(text);
                    self.signals.push(SignalKind::Edited);
                }
            }
            (Action::SetValue(value), WidgetKind::Slider { slider }) => {
                if !value.is_finite() {
                    return Err(Refusal::NotANumber);
                }
                let before = slider.value();
                slider.set_value(*value);
                if slider.value().to_bits() != before.to_bits() {
                    self.signals.push(SignalKind::Moved(slider.value()));
                }
            }
            (
                Action::ScrollTo { x, y },
                WidgetKind::ScrollView {
                    scroll_x,
                    scroll_y,
                    content_width,
                    content_height,
                },
            ) => {
                if !x.is_finite() || !y.is_finite() {
                    return Err(Refusal::NotANumber);
                }
                *scroll_x = x.clamp(0.0, (*content_width - self.layout.width).max(0.0));
                *scroll_y = y.clamp(0.0, (*content_height - self.layout.height).max(0.0));
            }
            _ => return Err(not_for()),
        }
        Ok(())
    }
}

impl WidgetTree {
    /// The tree as tools see it: its root's node, and every node under it --
    /// or `None` where its program keeps the whole of it from tools.
    #[must_use]
    pub fn automation(&self) -> Option<Node> {
        self.root.node((0.0, 0.0), true, true)
    }

    /// The node of the widget `id`, if tools can see it.
    #[must_use]
    pub fn automation_node(&self, id: WidgetId) -> Option<Node> {
        let tree = self.automation()?;
        tree.walk().find(|node| node.id == id).cloned()
    }

    /// Every widget tools can see that `query` fits, in the tree's order.
    #[must_use]
    pub fn find(&self, query: &Query) -> Vec<WidgetId> {
        self.automation()
            .map_or_else(Vec::new, |tree| query.find_in(&tree))
    }

    /// Do `action` to the widget `id`, as its user would (the module's "As
    /// the user would"): the widget's signals reach its program, and the
    /// tree is styled again for any state a style hangs on.
    ///
    /// # Errors
    ///
    /// [`Refusal`]: no such widget tools can see, a widget its user could
    /// not reach or use, one the action is not for, or a value that is no
    /// number. A refused action changes nothing.
    pub fn invoke(&mut self, id: WidgetId, action: Action) -> Result<(), Refusal> {
        let (path, enabled, shown) = self.root.reach(id).ok_or(Refusal::NoSuchWidget)?;
        if !shown {
            return Err(Refusal::Hidden);
        }
        if !enabled {
            return Err(Refusal::Disabled);
        }
        let before = self.states_if_styled();
        match &action {
            Action::Focus => {
                let takes = self.root.at_path(&path).is_some_and(Widget::accepts_focus);
                if !takes {
                    let role = self
                        .root
                        .at_path(&path)
                        .map_or(Role::Group, |w| Role::of(&w.kind));
                    return Err(Refusal::NotApplicable {
                        role,
                        action: action.name(),
                    });
                }
                self.set_focus(Some(id));
            }
            _ => {
                self.root
                    .at_path_mut(&path)
                    .ok_or(Refusal::NoSuchWidget)?
                    .act(&action)?;
                // A radio button chosen: the rest of its group are not, as
                // when its user clicks it.
                if action == Action::Choose
                    && let Some((&index, parent)) = path.split_last()
                    && let Some(holder) = self.root.at_path_mut(parent)
                {
                    holder.choose_radio(index);
                }
            }
        }
        let restyled = self.restyle(before);
        if self.animating && !restyled {
            self.layout();
        }
        self.send_signals();
        Ok(())
    }
}
