//! Enable/disable controls system for the GUI toolkit.
//!
//! Provides a comprehensive system for enabling/disabling UI controls with
//! optional reason tooltips that explain WHY something is disabled.
//!
//! # Components
//!
//! - [`DisabledState`] — per-widget enabled/disabled state with optional reason
//! - [`WhyDisabled`] — says why a disabled control is disabled, while the
//!   pointer rests on it
//! - [`DisabledGroup`] — enable/disable multiple controls at once
//! - [`ConditionalEnable`] — declarative rules for automatic enable/disable
//! - [`FormValidator`] — form validation with per-field rules
//!
//! # Example
//!
//! ```ignore
//! let mut group = DisabledGroup::new(GroupId(1), "Login fields");
//! group.disable(Some("Please accept the terms first".into()));
//! assert!(group.is_disabled());
//! ```

use crate::color::Color;
use crate::menu::Tooltip;
use crate::palette::Palette;
use crate::render::RenderCommand;
use crate::widget::WidgetId;

// ---------------------------------------------------------------------------
// DisabledState
// ---------------------------------------------------------------------------

/// Per-widget enabled/disabled state.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum DisabledState {
    /// Normal interaction allowed.
    #[default]
    Enabled,
    /// Grayed out; shows reason on hover.
    Disabled { reason: Option<String> },
    /// Temporarily disabled with an optional note about when it will be available.
    DisabledTemporary {
        reason: Option<String>,
        until: Option<String>,
    },
}

impl DisabledState {
    /// Returns `true` if the widget is in any disabled state.
    pub fn is_disabled(&self) -> bool {
        !matches!(self, Self::Enabled)
    }

    /// Returns `true` if the widget is enabled.
    pub fn is_enabled(&self) -> bool {
        matches!(self, Self::Enabled)
    }

    /// Get the reason string, if any.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Enabled => None,
            Self::Disabled { reason } => reason.as_deref(),
            Self::DisabledTemporary { reason, .. } => reason.as_deref(),
        }
    }

    /// Get the "until" hint for temporary disabled states.
    pub fn until(&self) -> Option<&str> {
        match self {
            Self::DisabledTemporary { until, .. } => until.as_deref(),
            _ => None,
        }
    }

    /// Transition to enabled.
    pub fn enable(&mut self) {
        *self = Self::Enabled;
    }

    /// Transition to disabled with an optional reason.
    pub fn disable(&mut self, reason: Option<String>) {
        *self = Self::Disabled { reason };
    }

    /// Transition to temporarily disabled.
    pub fn disable_temporary(&mut self, reason: Option<String>, until: Option<String>) {
        *self = Self::DisabledTemporary { reason, until };
    }
}

// ---------------------------------------------------------------------------
// How a disabled control looks
// ---------------------------------------------------------------------------

/// Default opacity multiplier for disabled controls.
///
/// `pub` since 2026-09-08. It was private, and `render_disabled` takes the
/// opacity as a *parameter*, so this module documented 50% as the standard and
/// gave no caller any way to ask for it -- every one would have written `0.5`
/// itself, which is how a standard stops being one. Found by removing this
/// file's `#![allow(dead_code)]`, which had been suppressing the "never used"
/// warning that says exactly this.
pub const DISABLED_OPACITY: f32 = 0.5;

// ---------------------------------------------------------------------------
// Saying why
// ---------------------------------------------------------------------------

/// One disabled control as a window drew it: its box, `(x, y, width,
/// height)`, and why it is disabled -- what [`WhyDisabled`] is handed.
pub type Disabled<'a> = ((f32, f32, f32, f32), &'a str);

/// Says why a disabled control is disabled, while the pointer rests on it --
/// what `design.txt` asks of every program: "a hover tooltip for any menu
/// option or button that's currently disabled, explaining why it's
/// disabled/how to enable it".
///
/// One per window. The toolkit's controls are drawn by their owner every
/// frame, so the owner is what knows which are disabled and why: it hands
/// those over with each pointer move ([`pointer_at`](Self::pointer_at)) --
/// each one's box, as drawn, and its reason -- lets time pass
/// ([`tick`](Self::tick), waking for [`due_in`](Self::due_in)), and draws
/// what [`render`](Self::render) gives over everything else. The reason
/// appears after the toolkit's tooltip delay, beside the pointer and on the
/// screen, as any [`Tooltip`] does; a menu's greyed rows say theirs the same
/// way ([`ContextMenu::explain`](crate::menu::ContextMenu::explain)).
#[derive(Default)]
pub struct WhyDisabled {
    /// The disabled control the pointer rests on.
    resting: Option<Explaining>,
}

/// The disabled control the pointer rests on: its box as drawn, its
/// reason, and the tooltip saying the reason.
struct Explaining {
    bounds: (f32, f32, f32, f32),
    why: String,
    tooltip: Tooltip,
}

impl WhyDisabled {
    /// Nothing to say yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The pointer is at `(x, y)` at `now_ms`, on a screen `viewport` big,
    /// over a window whose disabled controls are `disabled`. The first whose
    /// box holds the pointer and whose reason is not empty is explained: its
    /// reason after the delay, the wait going on while the pointer stays on
    /// it. Over none, any reason shown or waiting goes.
    ///
    /// Returns whether what [`render`](Self::render) draws changed -- a
    /// reason that was showing went, or gave way to another's wait -- the
    /// moment to repaint.
    pub fn pointer_at(
        &mut self,
        (x, y): (f32, f32),
        now_ms: u64,
        viewport: (f32, f32),
        disabled: &[Disabled<'_>],
    ) -> bool {
        let over = disabled.iter().find(|((left, top, width, height), why)| {
            !why.is_empty() && x >= *left && x < left + width && y >= *top && y < top + height
        });
        let was = self.is_showing();
        match over {
            Some((bounds, why)) => {
                let same = self.resting.as_ref().is_some_and(|resting| {
                    same_box(resting.bounds, *bounds) && resting.why == *why
                });
                if same {
                    return false;
                }
                let mut tooltip = Tooltip::new(why);
                tooltip.start_hover(x, y, now_ms, viewport);
                self.resting = Some(Explaining {
                    bounds: *bounds,
                    why: (*why).to_owned(),
                    tooltip,
                });
            }
            None => self.resting = None,
        }
        was
    }

    /// The pointer left the window: any reason shown or waiting goes.
    /// Returns whether one that was showing went.
    pub fn pointer_left(&mut self) -> bool {
        let was = self.is_showing();
        self.resting = None;
        was
    }

    /// Let time pass, at `now_ms`. Returns whether a reason appeared now --
    /// the moment to repaint.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        let Some(Explaining { tooltip, .. }) = self.resting.as_mut() else {
            return false;
        };
        let was = tooltip.is_visible();
        tooltip.tick(now_ms);
        !was && tooltip.is_visible()
    }

    /// How long until a reason appears, in milliseconds from `now_ms`;
    /// `None` when none is waiting to. For an owner that sleeps while nothing
    /// moves: a deadline with no wake-up behind it never comes.
    #[must_use]
    pub fn due_in(&self, now_ms: u64) -> Option<u64> {
        self.resting
            .as_ref()
            .and_then(|resting| resting.tooltip.due_in(now_ms))
    }

    /// The reason showing now, if one is.
    #[must_use]
    pub fn showing(&self) -> Option<&str> {
        self.resting
            .as_ref()
            .filter(|resting| resting.tooltip.is_visible())
            .map(|resting| resting.why.as_str())
    }

    /// What to draw over the window: the reason showing, if one is.
    #[must_use]
    pub fn render(&self, palette: &Palette) -> Vec<RenderCommand> {
        self.resting
            .as_ref()
            .map(|resting| resting.tooltip.render(palette))
            .unwrap_or_default()
    }

    fn is_showing(&self) -> bool {
        self.showing().is_some()
    }
}

/// Whether two boxes are the one drawn twice: the same four numbers, bit for
/// bit -- the owner passes the box it drew, unchanged between frames, so
/// identity is what is asked, not nearness.
fn same_box(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
    a.0.to_bits() == b.0.to_bits()
        && a.1.to_bits() == b.1.to_bits()
        && a.2.to_bits() == b.2.to_bits()
        && a.3.to_bits() == b.3.to_bits()
}

// ---------------------------------------------------------------------------
// Render helpers
// ---------------------------------------------------------------------------

/// Reduce opacity of all colors in a list of render commands.
///
/// Multiplies the alpha channel of every color in the commands by the given
/// opacity factor (0.0 = fully transparent, 1.0 = no change).
pub fn render_disabled(commands: &[RenderCommand], opacity: f32) -> Vec<RenderCommand> {
    let opacity = opacity.clamp(0.0, 1.0);
    commands
        .iter()
        .map(|cmd| apply_opacity(cmd, opacity))
        .collect()
}

/// Apply opacity reduction to a single render command.
fn apply_opacity(cmd: &RenderCommand, opacity: f32) -> RenderCommand {
    match cmd {
        RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color,
            corner_radii,
        } => RenderCommand::FillRect {
            x: *x,
            y: *y,
            width: *width,
            height: *height,
            color: reduce_alpha(*color, opacity),
            corner_radii: *corner_radii,
        },
        RenderCommand::StrokeRect {
            x,
            y,
            width,
            height,
            color,
            line_width,
            corner_radii,
        } => RenderCommand::StrokeRect {
            x: *x,
            y: *y,
            width: *width,
            height: *height,
            color: reduce_alpha(*color, opacity),
            line_width: *line_width,
            corner_radii: *corner_radii,
        },
        RenderCommand::Text {
            x,
            y,
            text,
            color,
            font_size,
            font_weight,
            max_width,
            overflow,
        } => RenderCommand::Text {
            x: *x,
            y: *y,
            text: text.clone(),
            color: reduce_alpha(*color, opacity),
            font_size: *font_size,
            font_weight: *font_weight,
            max_width: *max_width,
            // Greying a control out changes what it says about itself, not what
            // it says. A label that was marked as truncated stays marked.
            overflow: *overflow,
        },
        RenderCommand::Line {
            x1,
            y1,
            x2,
            y2,
            color,
            width,
        } => RenderCommand::Line {
            x1: *x1,
            y1: *y1,
            x2: *x2,
            y2: *y2,
            color: reduce_alpha(*color, opacity),
            width: *width,
        },
        RenderCommand::BoxShadow {
            x,
            y,
            width,
            height,
            offset_x,
            offset_y,
            blur,
            spread,
            color,
            corner_radii,
        } => RenderCommand::BoxShadow {
            x: *x,
            y: *y,
            width: *width,
            height: *height,
            offset_x: *offset_x,
            offset_y: *offset_y,
            blur: *blur,
            spread: *spread,
            color: reduce_alpha(*color, opacity),
            corner_radii: *corner_radii,
        },
        // Structural commands (clips, transforms, images) pass through unchanged.
        other => other.clone(),
    }
}

/// Reduce a color's alpha by the given factor.
fn reduce_alpha(color: Color, factor: f32) -> Color {
    let new_alpha = ((color.a as f32) * factor) as u8;
    Color::rgba(color.r, color.g, color.b, new_alpha)
}

// ---------------------------------------------------------------------------
// DisabledGroup
// ---------------------------------------------------------------------------

/// Identifier for a group of controls that can be disabled together.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GroupId(pub u64);

/// A group of related controls that can be enabled/disabled together.
///
/// Useful for form sections that depend on a toggle/checkbox.
/// Nested groups are supported: a control is disabled if ANY ancestor group
/// is disabled.
#[derive(Clone, Debug)]
pub struct DisabledGroup {
    /// Unique group identifier.
    pub id: GroupId,
    /// Human-readable label for debugging.
    pub label: String,
    /// Whether this group is explicitly disabled.
    disabled: bool,
    /// Reason for being disabled.
    reason: Option<String>,
    /// Widget IDs that belong to this group.
    members: Vec<WidgetId>,
    /// Parent group (for nesting).
    parent: Option<GroupId>,
}

impl DisabledGroup {
    /// Create a new enabled group.
    pub fn new(id: GroupId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            disabled: false,
            reason: None,
            members: Vec::new(),
            parent: None,
        }
    }

    /// Create a nested group under a parent.
    pub fn with_parent(id: GroupId, label: impl Into<String>, parent: GroupId) -> Self {
        Self {
            id,
            label: label.into(),
            disabled: false,
            reason: None,
            members: Vec::new(),
            parent: Some(parent),
        }
    }

    /// Disable this group with an optional reason.
    pub fn disable(&mut self, reason: Option<String>) {
        self.disabled = true;
        self.reason = reason;
    }

    /// Enable this group.
    pub fn enable(&mut self) {
        self.disabled = false;
        self.reason = None;
    }

    /// Whether this group is explicitly disabled (not considering parents).
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Whether this group is enabled (not considering parents).
    pub fn is_enabled(&self) -> bool {
        !self.disabled
    }

    /// Get the reason this group is disabled.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Get the parent group ID, if any.
    pub fn parent(&self) -> Option<GroupId> {
        self.parent
    }

    /// Add a widget to this group.
    pub fn add_member(&mut self, widget_id: WidgetId) {
        if !self.members.contains(&widget_id) {
            self.members.push(widget_id);
        }
    }

    /// Remove a widget from this group.
    pub fn remove_member(&mut self, widget_id: &WidgetId) {
        self.members.retain(|id| id != widget_id);
    }

    /// Get all widget IDs in this group.
    pub fn members(&self) -> &[WidgetId] {
        &self.members
    }
}

/// Manager for multiple disabled groups, handling nesting.
#[derive(Clone, Debug, Default)]
pub struct GroupManager {
    groups: Vec<DisabledGroup>,
}

impl GroupManager {
    /// Create a new empty group manager.
    pub fn new() -> Self {
        Self { groups: Vec::new() }
    }

    /// Register a group.
    pub fn add_group(&mut self, group: DisabledGroup) {
        self.groups.push(group);
    }

    /// Disable a group by ID.
    pub fn disable_group(&mut self, id: GroupId, reason: Option<String>) {
        if let Some(group) = self.groups.iter_mut().find(|g| g.id == id) {
            group.disable(reason);
        }
    }

    /// Enable a group by ID.
    pub fn enable_group(&mut self, id: GroupId) {
        if let Some(group) = self.groups.iter_mut().find(|g| g.id == id) {
            group.enable();
        }
    }

    /// Check if a widget is effectively disabled (its group or any ancestor group is disabled).
    pub fn is_widget_disabled(&self, widget_id: WidgetId) -> bool {
        // Find which group(s) this widget belongs to.
        for group in &self.groups {
            if group.members().contains(&widget_id) && self.is_group_effectively_disabled(group.id)
            {
                return true;
            }
        }
        false
    }

    /// Check if a group is effectively disabled (itself or any ancestor).
    pub fn is_group_effectively_disabled(&self, id: GroupId) -> bool {
        let group = match self.groups.iter().find(|g| g.id == id) {
            Some(g) => g,
            None => return false,
        };

        if group.is_disabled() {
            return true;
        }

        // Check parent chain.
        if let Some(parent_id) = group.parent() {
            return self.is_group_effectively_disabled(parent_id);
        }

        false
    }

    /// Get the effective reason for a widget being disabled (checks group chain).
    pub fn widget_disabled_reason(&self, widget_id: WidgetId) -> Option<String> {
        for group in &self.groups {
            if group.members().contains(&widget_id)
                && let Some(reason) = self.group_effective_reason(group.id)
            {
                return Some(reason);
            }
        }
        None
    }

    /// Get the effective reason for a group being disabled (walks parent chain).
    fn group_effective_reason(&self, id: GroupId) -> Option<String> {
        let group = self.groups.iter().find(|g| g.id == id)?;

        if group.is_disabled() {
            return group.reason().map(|s| s.to_string());
        }

        if let Some(parent_id) = group.parent() {
            return self.group_effective_reason(parent_id);
        }

        None
    }

    /// Get a group by ID.
    pub fn get_group(&self, id: GroupId) -> Option<&DisabledGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// Get a mutable reference to a group by ID.
    pub fn get_group_mut(&mut self, id: GroupId) -> Option<&mut DisabledGroup> {
        self.groups.iter_mut().find(|g| g.id == id)
    }
}

// ---------------------------------------------------------------------------
// ConditionalEnable
// ---------------------------------------------------------------------------

/// Declarative condition for when a widget should be enabled.
pub enum EnableWhen {
    /// Always enabled.
    Always,
    /// Enabled when the specified field is not empty.
    FieldNotEmpty(WidgetId),
    /// Enabled when the specified checkbox is checked.
    CheckboxChecked(WidgetId),
    /// All sub-conditions must be true.
    AllOf(Vec<EnableWhen>),
    /// Any sub-condition suffices.
    AnyOf(Vec<EnableWhen>),
    /// Arbitrary function.
    Custom(fn() -> bool),
}

/// Context for evaluating `EnableWhen` conditions.
///
/// Provides access to widget states needed for condition evaluation.
pub struct EnableContext {
    /// Field values indexed by widget ID (empty string if field doesn't exist).
    field_values: Vec<(WidgetId, String)>,
    /// Checkbox states indexed by widget ID.
    checkbox_states: Vec<(WidgetId, bool)>,
}

impl EnableContext {
    /// Create a new empty context.
    pub fn new() -> Self {
        Self {
            field_values: Vec::new(),
            checkbox_states: Vec::new(),
        }
    }

    /// Set the value of a text field.
    pub fn set_field_value(&mut self, id: WidgetId, value: impl Into<String>) {
        let value = value.into();
        if let Some(entry) = self.field_values.iter_mut().find(|(wid, _)| *wid == id) {
            entry.1 = value;
        } else {
            self.field_values.push((id, value));
        }
    }

    /// Set the state of a checkbox.
    pub fn set_checkbox_state(&mut self, id: WidgetId, checked: bool) {
        if let Some(entry) = self.checkbox_states.iter_mut().find(|(wid, _)| *wid == id) {
            entry.1 = checked;
        } else {
            self.checkbox_states.push((id, checked));
        }
    }

    /// Get the value of a text field.
    pub fn field_value(&self, id: WidgetId) -> &str {
        self.field_values
            .iter()
            .find(|(wid, _)| *wid == id)
            .map(|(_, v)| v.as_str())
            .unwrap_or("")
    }

    /// Get the state of a checkbox.
    pub fn checkbox_checked(&self, id: WidgetId) -> bool {
        self.checkbox_states
            .iter()
            .find(|(wid, _)| *wid == id)
            .map(|(_, v)| *v)
            .unwrap_or(false)
    }
}

impl Default for EnableContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Evaluates an `EnableWhen` condition against a context.
pub fn evaluate_condition(condition: &EnableWhen, ctx: &EnableContext) -> bool {
    match condition {
        EnableWhen::Always => true,
        EnableWhen::FieldNotEmpty(id) => !ctx.field_value(*id).is_empty(),
        EnableWhen::CheckboxChecked(id) => ctx.checkbox_checked(*id),
        EnableWhen::AllOf(conditions) => conditions.iter().all(|c| evaluate_condition(c, ctx)),
        EnableWhen::AnyOf(conditions) => conditions.iter().any(|c| evaluate_condition(c, ctx)),
        EnableWhen::Custom(f) => f(),
    }
}

/// A binding between a widget and its enable condition.
pub struct ConditionalEnable {
    /// The widget whose enabled state is controlled.
    pub target: WidgetId,
    /// The condition that determines whether the target is enabled.
    pub condition: EnableWhen,
    /// Reason to show when disabled (derived from condition type if not set).
    pub disabled_reason: Option<String>,
}

impl ConditionalEnable {
    /// Create a new conditional enable binding.
    pub fn new(target: WidgetId, condition: EnableWhen) -> Self {
        Self {
            target,
            condition,
            disabled_reason: None,
        }
    }

    /// Set the reason shown when the widget is disabled.
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.disabled_reason = Some(reason.into());
        self
    }

    /// Evaluate the condition and return the resulting disabled state.
    pub fn evaluate(&self, ctx: &EnableContext) -> DisabledState {
        if evaluate_condition(&self.condition, ctx) {
            DisabledState::Enabled
        } else {
            DisabledState::Disabled {
                reason: self.disabled_reason.clone(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// FormValidator
// ---------------------------------------------------------------------------

/// Validation rule for a form field.
pub enum ValidationRule {
    /// Field must not be empty.
    Required,
    /// Minimum character count.
    MinLength(usize),
    /// Maximum character count.
    MaxLength(usize),
    /// Simplified pattern matching (checked via `contains` for basic cases).
    Pattern(String),
    /// Custom validation function with error message.
    Custom(fn(&str) -> bool, String),
    /// Basic email format check (must contain `@` and a dot after it).
    Email,
    /// Must be a valid number.
    Numeric,
}

impl ValidationRule {
    /// Validate a value against this rule.
    pub fn validate(&self, value: &str) -> ValidationResult {
        match self {
            Self::Required => {
                if value.trim().is_empty() {
                    ValidationResult::invalid("This field is required")
                } else {
                    ValidationResult::valid()
                }
            }
            Self::MinLength(min) => {
                if value.len() < *min {
                    ValidationResult::invalid(format!("Must be at least {} characters", min))
                } else {
                    ValidationResult::valid()
                }
            }
            Self::MaxLength(max) => {
                if value.len() > *max {
                    ValidationResult::invalid(format!("Must be at most {} characters", max))
                } else {
                    ValidationResult::valid()
                }
            }
            Self::Pattern(pattern) => {
                if value.contains(pattern.as_str()) {
                    ValidationResult::valid()
                } else {
                    ValidationResult::invalid(format!("Must match pattern: {}", pattern))
                }
            }
            Self::Custom(f, msg) => {
                if f(value) {
                    ValidationResult::valid()
                } else {
                    ValidationResult::invalid(msg.clone())
                }
            }
            Self::Email => {
                if is_valid_email(value) {
                    ValidationResult::valid()
                } else {
                    ValidationResult::invalid("Invalid email address")
                }
            }
            Self::Numeric => {
                if value.trim().is_empty() || value.parse::<f64>().is_ok() {
                    ValidationResult::valid()
                } else {
                    ValidationResult::invalid("Must be a number")
                }
            }
        }
    }
}

/// Basic email validation: a non-empty local part, an `@`, and a domain whose
/// first dot is neither the first nor the last character.
///
/// Deliberately far short of RFC 5322 — no quoting, no comments, no bracketed
/// address literals, and a second `@` is accepted because the split takes the
/// first one. This is the "your typing has clearly gone wrong" check that runs
/// as the user types, not an authority on whether an address exists; only
/// sending mail to it settles that. Rejecting more aggressively here would turn
/// a form into an argument with people whose real addresses are unusual.
///
/// Written with `split_once` rather than `find` plus a slice: the offsets the
/// old form carried — `&value[at_pos + 1..]`, `dot_pos == domain.len() - 1` —
/// were correct only because of the `if`s standing above them, and both were
/// arithmetic on a `usize` whose bounds lived in a different statement.
fn is_valid_email(value: &str) -> bool {
    let Some((local, domain)) = value.trim().split_once('@') else {
        return false;
    };
    // A non-empty part on each side of the first dot is exactly "the domain
    // does not begin with a dot, and does not end at its first one".
    !local.is_empty()
        && domain
            .split_once('.')
            .is_some_and(|(host, rest)| !host.is_empty() && !rest.is_empty())
}

/// Result of validating a single field.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidationResult {
    pub valid: bool,
    pub message: Option<String>,
}

impl ValidationResult {
    /// Create a valid result.
    pub fn valid() -> Self {
        Self {
            valid: true,
            message: None,
        }
    }

    /// Create an invalid result with a message.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            valid: false,
            message: Some(message.into()),
        }
    }
}

/// Per-field validation state.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidationState {
    pub valid: bool,
    pub message: Option<String>,
}

impl ValidationState {
    /// Create a valid state.
    pub fn valid() -> Self {
        Self {
            valid: true,
            message: None,
        }
    }

    /// Create an invalid state.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            valid: false,
            message: Some(message.into()),
        }
    }
}

impl Default for ValidationState {
    fn default() -> Self {
        Self::valid()
    }
}

impl From<ValidationResult> for ValidationState {
    /// A rule's verdict *is* the field's state when that rule is the one that
    /// failed; the two types differ only in which layer names them.
    fn from(result: ValidationResult) -> Self {
        Self {
            valid: result.valid,
            message: result.message,
        }
    }
}

/// A field registration for the form validator, and its verdict as of the last
/// time it was checked.
///
/// The state lives here rather than in a second `Vec` keyed by `widget_id`.
/// Two collections that must agree about which fields exist is an invariant
/// maintained by hand at every call site, and this one was not maintained: a
/// query for an unregistered widget used to *push* an entry into the state list
/// so it could return a reference to something, after which the form counted a
/// field nobody had registered as part of its own validity, for good. One
/// collection cannot disagree with itself.
struct FormField {
    /// Widget ID of the field.
    widget_id: WidgetId,
    /// Label, used where an error is reported away from the field itself.
    label: String,
    /// Validation rules (all must pass).
    rules: Vec<ValidationRule>,
    /// Verdict from the last `check`; valid until the field is first checked.
    state: ValidationState,
}

impl FormField {
    /// Run the rules in order and record the first failure, if any.
    ///
    /// Returns whether the field now passes. The iterator stops at the first
    /// failing rule, so a later rule never overwrites an earlier complaint --
    /// the user is told the first thing that is wrong, not the last.
    fn check(&mut self, value: &str) -> bool {
        self.state = self
            .rules
            .iter()
            .map(|rule| rule.validate(value))
            .find(|result| !result.valid)
            .map_or_else(ValidationState::valid, ValidationState::from);
        self.state.valid
    }
}

/// The verdict reported for a widget the form has never heard of.
///
/// A `static` rather than an entry invented on demand: a validator asked about
/// a widget it does not own has nothing to say, and saying so must not change
/// what the validator is. Held as a value so the answer can be handed out by
/// reference like any registered field's.
static UNREGISTERED: ValidationState = ValidationState {
    valid: true,
    message: None,
};

/// Form-level validator that checks all fields and controls submit button state.
pub struct FormValidator {
    fields: Vec<FormField>,
    /// Widget ID of the submit button (disabled when form is invalid).
    submit_button: Option<WidgetId>,
}

impl FormValidator {
    /// Create a new form validator.
    pub fn new() -> Self {
        Self {
            fields: Vec::new(),
            submit_button: None,
        }
    }

    /// Set the submit button widget ID.
    pub fn set_submit_button(&mut self, id: WidgetId) {
        self.submit_button = Some(id);
    }

    /// Get the submit button widget ID.
    pub fn submit_button(&self) -> Option<WidgetId> {
        self.submit_button
    }

    /// Register a field with its validation rules.
    pub fn add_field(
        &mut self,
        widget_id: WidgetId,
        label: impl Into<String>,
        rules: Vec<ValidationRule>,
    ) {
        self.fields.push(FormField {
            widget_id,
            label: label.into(),
            rules,
            state: ValidationState::valid(),
        });
    }

    /// Validate a single field by its widget ID. Returns the validation state.
    ///
    /// A widget this form does not own is reported valid and left alone; see
    /// `UNREGISTERED`.
    pub fn validate_field(&mut self, widget_id: WidgetId, value: &str) -> &ValidationState {
        match self.fields.iter_mut().find(|f| f.widget_id == widget_id) {
            Some(field) => {
                field.check(value);
                &field.state
            }
            None => &UNREGISTERED,
        }
    }

    /// Validate all fields with provided values. Returns overall validity.
    ///
    /// A field with no entry in `values` is checked against the empty string,
    /// which is what an untouched text box holds -- so a `Required` field the
    /// caller forgot to pass fails rather than passing by omission.
    pub fn validate_all(&mut self, values: &[(WidgetId, &str)]) -> bool {
        let mut all_valid = true;
        for field in &mut self.fields {
            let value = values
                .iter()
                .find(|(id, _)| *id == field.widget_id)
                .map_or("", |(_, v)| *v);
            if !field.check(value) {
                all_valid = false;
            }
        }
        all_valid
    }

    /// Whether the entire form is currently valid (based on last validation run).
    pub fn is_valid(&self) -> bool {
        self.fields.iter().all(|f| f.state.valid)
    }

    /// Get the validation state of a field.
    pub fn field_state(&self, widget_id: WidgetId) -> Option<&ValidationState> {
        self.fields
            .iter()
            .find(|f| f.widget_id == widget_id)
            .map(|f| &f.state)
    }

    /// Get the disabled state for the submit button based on form validity.
    ///
    /// The reason names the field. The submit button sits at the bottom of the
    /// form, nowhere near whatever failed, so an unqualified "Required" -- which
    /// is what this used to hand over -- tells the user nothing about which of
    /// five boxes to go back to. This is what `FormField::label` is for.
    pub fn submit_disabled_state(&self) -> DisabledState {
        let Some(failed) = self.fields.iter().find(|f| !f.state.valid) else {
            return DisabledState::Enabled;
        };
        let label = &failed.label;
        let reason = failed.state.message.as_ref().map_or_else(
            || format!("{label} is invalid"),
            |message| format!("{label}: {message}"),
        );
        DisabledState::Disabled {
            reason: Some(reason),
        }
    }
}

impl Default for FormValidator {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;
    use crate::render::{FontWeightHint, TextOverflow};
    use crate::style::CornerRadii;

    // -- DisabledState tests --

    #[test]
    fn disabled_state_default_is_enabled() {
        let state = DisabledState::default();
        assert!(state.is_enabled());
        assert!(!state.is_disabled());
        assert!(state.reason().is_none());
    }

    #[test]
    fn disabled_state_transitions() {
        let mut state = DisabledState::Enabled;
        assert!(state.is_enabled());

        state.disable(Some("Not available".into()));
        assert!(state.is_disabled());
        assert_eq!(state.reason(), Some("Not available"));

        state.enable();
        assert!(state.is_enabled());
        assert!(state.reason().is_none());

        state.disable_temporary(
            Some("Updating".into()),
            Some("After update completes".into()),
        );
        assert!(state.is_disabled());
        assert_eq!(state.reason(), Some("Updating"));
        assert_eq!(state.until(), Some("After update completes"));
    }

    #[test]
    fn disabled_state_no_reason() {
        let state = DisabledState::Disabled { reason: None };
        assert!(state.is_disabled());
        assert!(state.reason().is_none());
    }

    #[test]
    fn disabled_temporary_no_until() {
        let state = DisabledState::DisabledTemporary {
            reason: Some("Loading".into()),
            until: None,
        };
        assert!(state.is_disabled());
        assert_eq!(state.reason(), Some("Loading"));
        assert!(state.until().is_none());
    }

    // -- DisabledGroup tests --

    #[test]
    fn group_simple_enable_disable() {
        let mut group = DisabledGroup::new(GroupId(1), "Test group");
        assert!(group.is_enabled());

        group.disable(Some("Form incomplete".into()));
        assert!(group.is_disabled());
        assert_eq!(group.reason(), Some("Form incomplete"));

        group.enable();
        assert!(group.is_enabled());
        assert!(group.reason().is_none());
    }

    #[test]
    fn group_members() {
        let mut group = DisabledGroup::new(GroupId(1), "Fields");
        let w1 = WidgetId(100);
        let w2 = WidgetId(101);

        group.add_member(w1);
        group.add_member(w2);
        assert_eq!(group.members().len(), 2);

        // No duplicates.
        group.add_member(w1);
        assert_eq!(group.members().len(), 2);

        group.remove_member(&w1);
        assert_eq!(group.members().len(), 1);
        assert_eq!(group.members()[0], w2);
    }

    #[test]
    fn group_nested_disable() {
        let mut mgr = GroupManager::new();

        let mut parent = DisabledGroup::new(GroupId(1), "Parent");
        let w_parent = WidgetId(10);
        parent.add_member(w_parent);
        mgr.add_group(parent);

        let mut child = DisabledGroup::with_parent(GroupId(2), "Child", GroupId(1));
        let w_child = WidgetId(20);
        child.add_member(w_child);
        mgr.add_group(child);

        // Neither disabled initially.
        assert!(!mgr.is_widget_disabled(w_parent));
        assert!(!mgr.is_widget_disabled(w_child));

        // Disable parent -> child is also effectively disabled.
        mgr.disable_group(GroupId(1), Some("Parent disabled".into()));
        assert!(mgr.is_widget_disabled(w_parent));
        assert!(mgr.is_widget_disabled(w_child));
        assert!(mgr.is_group_effectively_disabled(GroupId(2)));

        // Enable parent -> child is no longer effectively disabled.
        mgr.enable_group(GroupId(1));
        assert!(!mgr.is_widget_disabled(w_child));

        // Disable only child.
        mgr.disable_group(GroupId(2), Some("Child only".into()));
        assert!(!mgr.is_widget_disabled(w_parent));
        assert!(mgr.is_widget_disabled(w_child));
    }

    #[test]
    fn group_effective_reason() {
        let mut mgr = GroupManager::new();

        let mut parent = DisabledGroup::new(GroupId(1), "Parent");
        parent.add_member(WidgetId(10));
        mgr.add_group(parent);

        let mut child = DisabledGroup::with_parent(GroupId(2), "Child", GroupId(1));
        child.add_member(WidgetId(20));
        mgr.add_group(child);

        mgr.disable_group(GroupId(1), Some("Terms not accepted".into()));
        assert_eq!(
            mgr.widget_disabled_reason(WidgetId(20)),
            Some("Terms not accepted".into())
        );
    }

    // -- ConditionalEnable tests --

    #[test]
    fn condition_always() {
        let ctx = EnableContext::new();
        assert!(evaluate_condition(&EnableWhen::Always, &ctx));
    }

    #[test]
    fn condition_field_not_empty() {
        let field_id = WidgetId(1);
        let mut ctx = EnableContext::new();

        // Empty field -> condition false.
        ctx.set_field_value(field_id, "");
        assert!(!evaluate_condition(
            &EnableWhen::FieldNotEmpty(field_id),
            &ctx
        ));

        // Non-empty -> condition true.
        ctx.set_field_value(field_id, "hello");
        assert!(evaluate_condition(
            &EnableWhen::FieldNotEmpty(field_id),
            &ctx
        ));
    }

    #[test]
    fn condition_checkbox_checked() {
        let cb_id = WidgetId(2);
        let mut ctx = EnableContext::new();

        // Unchecked -> false.
        ctx.set_checkbox_state(cb_id, false);
        assert!(!evaluate_condition(
            &EnableWhen::CheckboxChecked(cb_id),
            &ctx
        ));

        // Checked -> true.
        ctx.set_checkbox_state(cb_id, true);
        assert!(evaluate_condition(
            &EnableWhen::CheckboxChecked(cb_id),
            &ctx
        ));
    }

    #[test]
    fn condition_all_of() {
        let f1 = WidgetId(1);
        let f2 = WidgetId(2);
        let mut ctx = EnableContext::new();
        ctx.set_field_value(f1, "a");
        ctx.set_field_value(f2, "");

        let cond = EnableWhen::AllOf(vec![
            EnableWhen::FieldNotEmpty(f1),
            EnableWhen::FieldNotEmpty(f2),
        ]);
        // One empty -> false.
        assert!(!evaluate_condition(&cond, &ctx));

        ctx.set_field_value(f2, "b");
        assert!(evaluate_condition(&cond, &ctx));
    }

    #[test]
    fn condition_any_of() {
        let f1 = WidgetId(1);
        let f2 = WidgetId(2);
        let mut ctx = EnableContext::new();
        ctx.set_field_value(f1, "");
        ctx.set_field_value(f2, "");

        let cond = EnableWhen::AnyOf(vec![
            EnableWhen::FieldNotEmpty(f1),
            EnableWhen::FieldNotEmpty(f2),
        ]);
        // Both empty -> false.
        assert!(!evaluate_condition(&cond, &ctx));

        // One filled -> true.
        ctx.set_field_value(f1, "x");
        assert!(evaluate_condition(&cond, &ctx));
    }

    #[test]
    fn condition_custom() {
        let ctx = EnableContext::new();
        let cond_true = EnableWhen::Custom(|| true);
        let cond_false = EnableWhen::Custom(|| false);
        assert!(evaluate_condition(&cond_true, &ctx));
        assert!(!evaluate_condition(&cond_false, &ctx));
    }

    #[test]
    fn conditional_enable_evaluation() {
        let target = WidgetId(10);
        let field = WidgetId(1);
        let mut ctx = EnableContext::new();
        ctx.set_field_value(field, "");

        let ce = ConditionalEnable::new(target, EnableWhen::FieldNotEmpty(field))
            .with_reason("Please fill in the name field");

        let state = ce.evaluate(&ctx);
        assert!(state.is_disabled());
        assert_eq!(state.reason(), Some("Please fill in the name field"));

        ctx.set_field_value(field, "Alice");
        let state = ce.evaluate(&ctx);
        assert!(state.is_enabled());
    }

    // -- Render opacity tests --

    #[test]
    fn render_opacity_reduction() {
        let commands = vec![
            RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
                color: Color::rgba(200, 100, 50, 255),
                corner_radii: CornerRadii::ZERO,
            },
            RenderCommand::Text {
                x: 10.0,
                y: 10.0,
                text: "Hello".into(),
                color: Color::rgb(0, 0, 0),
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            },
        ];

        let result = render_disabled(&commands, 0.5);
        assert_eq!(result.len(), 2);

        // Check first command alpha was halved.
        if let RenderCommand::FillRect { color, .. } = &result[0] {
            assert_eq!(color.a, 127); // 255 * 0.5 = 127.5 -> 127
        } else {
            panic!("Expected FillRect");
        }

        // Check text color alpha was halved.
        if let RenderCommand::Text { color, .. } = &result[1] {
            assert_eq!(color.a, 127);
        } else {
            panic!("Expected Text");
        }
    }

    #[test]
    fn render_opacity_passthrough_clips() {
        let commands = vec![
            RenderCommand::PushClip {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            RenderCommand::PopClip,
        ];

        let result = render_disabled(&commands, 0.5);
        assert_eq!(result.len(), 2);
        // Structural commands remain unchanged.
        assert!(matches!(result[0], RenderCommand::PushClip { .. }));
        assert!(matches!(result[1], RenderCommand::PopClip));
    }

    #[test]
    fn render_opacity_zero_makes_transparent() {
        let commands = vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            color: Color::rgb(255, 0, 0),
            corner_radii: CornerRadii::ZERO,
        }];

        let result = render_disabled(&commands, 0.0);
        if let RenderCommand::FillRect { color, .. } = &result[0] {
            assert_eq!(color.a, 0);
        } else {
            panic!("Expected FillRect");
        }
    }

    #[test]
    fn render_opacity_one_no_change() {
        let commands = vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            color: Color::rgba(100, 150, 200, 200),
            corner_radii: CornerRadii::ZERO,
        }];

        let result = render_disabled(&commands, 1.0);
        if let RenderCommand::FillRect { color, .. } = &result[0] {
            assert_eq!(color.a, 200);
        } else {
            panic!("Expected FillRect");
        }
    }

    // -- FormValidator tests --

    #[test]
    fn validation_rule_required() {
        let rule = ValidationRule::Required;
        assert!(rule.validate("hello").valid);
        assert!(!rule.validate("").valid);
        assert!(!rule.validate("   ").valid);
    }

    #[test]
    fn validation_rule_min_length() {
        let rule = ValidationRule::MinLength(3);
        assert!(rule.validate("abc").valid);
        assert!(rule.validate("abcd").valid);
        assert!(!rule.validate("ab").valid);
        assert!(!rule.validate("").valid);
    }

    #[test]
    fn validation_rule_max_length() {
        let rule = ValidationRule::MaxLength(5);
        assert!(rule.validate("abc").valid);
        assert!(rule.validate("abcde").valid);
        assert!(!rule.validate("abcdef").valid);
    }

    #[test]
    fn validation_rule_email() {
        let rule = ValidationRule::Email;
        assert!(rule.validate("user@example.com").valid);
        assert!(rule.validate("a@b.c").valid);
        assert!(!rule.validate("noatsign").valid);
        assert!(!rule.validate("@nodomain").valid);
        assert!(!rule.validate("user@").valid);
        assert!(!rule.validate("user@nodot").valid);
        assert!(!rule.validate("").valid);
        assert!(!rule.validate("user@.com").valid);
        assert!(!rule.validate("user@com.").valid);
    }

    #[test]
    fn validation_rule_numeric() {
        let rule = ValidationRule::Numeric;
        assert!(rule.validate("123").valid);
        assert!(rule.validate("3.14").valid);
        assert!(rule.validate("-42").valid);
        assert!(rule.validate("").valid); // Empty is valid for numeric (use Required to mandate)
        assert!(!rule.validate("abc").valid);
        assert!(!rule.validate("12.34.56").valid);
    }

    #[test]
    fn validation_rule_pattern() {
        let rule = ValidationRule::Pattern("@".into());
        assert!(rule.validate("hello@world").valid);
        assert!(!rule.validate("helloworld").valid);
    }

    #[test]
    fn validation_rule_custom() {
        let rule = ValidationRule::Custom(|v| v.starts_with("OK"), "Must start with OK".into());
        assert!(rule.validate("OK fine").valid);
        assert!(!rule.validate("Not ok").valid);
    }

    #[test]
    fn form_validator_multi_field() {
        let mut validator = FormValidator::new();
        let name_field = WidgetId(1);
        let email_field = WidgetId(2);
        let submit = WidgetId(100);

        validator.add_field(name_field, "Name", vec![ValidationRule::Required]);
        validator.add_field(
            email_field,
            "Email",
            vec![ValidationRule::Required, ValidationRule::Email],
        );
        validator.set_submit_button(submit);

        // Both empty -> invalid.
        let valid = validator.validate_all(&[(name_field, ""), (email_field, "")]);
        assert!(!valid);
        assert!(!validator.is_valid());
        assert!(validator.submit_disabled_state().is_disabled());

        // Name filled, email empty -> invalid.
        let valid = validator.validate_all(&[(name_field, "Alice"), (email_field, "")]);
        assert!(!valid);

        // Both filled correctly -> valid.
        let valid =
            validator.validate_all(&[(name_field, "Alice"), (email_field, "alice@example.com")]);
        assert!(valid);
        assert!(validator.is_valid());
        assert!(validator.submit_disabled_state().is_enabled());
    }

    #[test]
    fn form_validator_single_field() {
        let mut validator = FormValidator::new();
        let field = WidgetId(1);
        validator.add_field(
            field,
            "Username",
            vec![ValidationRule::Required, ValidationRule::MinLength(3)],
        );

        let state = validator.validate_field(field, "ab");
        assert!(!state.valid);
        assert!(state.message.as_deref().unwrap().contains("3 characters"));

        let state = validator.validate_field(field, "abc");
        assert!(state.valid);
    }

    /// Asking about a widget the form does not own must not enrol it. The old
    /// code pushed a state entry so it had something to return a reference to,
    /// after which `is_valid` counted a field nobody had registered.
    #[test]
    fn asking_about_an_unregistered_widget_does_not_enrol_it() {
        let mut validator = FormValidator::new();
        let known = WidgetId(1);
        let stranger = WidgetId(2);
        validator.add_field(known, "Name", vec![ValidationRule::Required]);

        // Anything at all is reported valid, including what would fail a rule.
        let state = validator.validate_field(stranger, "");
        assert!(state.valid);
        assert!(state.message.is_none());
        assert!(
            validator.field_state(stranger).is_none(),
            "the stranger must not have become a field"
        );

        // And the form's own validity still turns only on the field that exists.
        assert!(validator.validate_all(&[(known, "Alice")]));
        assert!(validator.is_valid());
        assert!(!validator.validate_all(&[(known, "")]));
        assert!(!validator.is_valid());
    }

    /// A field the caller left out of `values` is checked against the empty
    /// string, so a `Required` field cannot pass by being forgotten.
    #[test]
    fn a_field_missing_from_the_values_is_checked_as_empty() {
        let mut validator = FormValidator::new();
        let name = WidgetId(1);
        let email = WidgetId(2);
        validator.add_field(name, "Name", vec![ValidationRule::Required]);
        validator.add_field(email, "Email", vec![ValidationRule::Required]);

        assert!(!validator.validate_all(&[(name, "Alice")]));
        assert!(!validator.field_state(email).unwrap().valid);
    }

    /// The submit button is nowhere near the field that failed, so its reason
    /// has to say which field it is talking about.
    #[test]
    fn the_submit_buttons_reason_names_the_field_that_failed() {
        let mut validator = FormValidator::new();
        let name = WidgetId(1);
        let email = WidgetId(2);
        validator.add_field(name, "Name", vec![ValidationRule::Required]);
        validator.add_field(email, "Email", vec![ValidationRule::Email]);

        assert!(
            validator.submit_disabled_state().is_enabled(),
            "nothing checked yet"
        );

        validator.validate_all(&[(name, ""), (email, "nope")]);
        let reason = validator
            .submit_disabled_state()
            .reason()
            .expect("a disabled submit button says why")
            .to_string();
        assert!(reason.starts_with("Name"), "reason was {reason:?}");

        // With the first field fixed the reason moves to the next failure.
        validator.validate_all(&[(name, "Alice"), (email, "nope")]);
        let reason = validator
            .submit_disabled_state()
            .reason()
            .expect("still disabled")
            .to_string();
        assert!(reason.starts_with("Email"), "reason was {reason:?}");
    }

    /// The first failing rule is the one reported; a later rule must not
    /// overwrite it, and a passing later rule must not clear it.
    #[test]
    fn the_first_failing_rule_is_the_one_reported() {
        let mut validator = FormValidator::new();
        let field = WidgetId(1);
        validator.add_field(
            field,
            "Username",
            vec![
                ValidationRule::MinLength(5),
                ValidationRule::MaxLength(2),
                ValidationRule::Required,
            ],
        );

        let state = validator.validate_field(field, "abc");
        assert!(!state.valid);
        assert!(
            state.message.as_deref().unwrap().contains('5'),
            "expected the MinLength complaint, got {:?}",
            state.message
        );
    }

    /// Re-checking a field replaces its verdict rather than accumulating one.
    #[test]
    fn a_field_that_becomes_valid_stops_being_reported_as_invalid() {
        let mut validator = FormValidator::new();
        let field = WidgetId(1);
        validator.add_field(field, "Name", vec![ValidationRule::Required]);

        assert!(!validator.validate_field(field, "").valid);
        assert!(!validator.is_valid());
        let state = validator.validate_field(field, "Alice");
        assert!(state.valid);
        assert!(state.message.is_none(), "the old complaint must be gone");
        assert!(validator.is_valid());
        assert!(validator.submit_disabled_state().is_enabled());
    }

    /// The offsets `is_valid_email` used to compute by hand were all at the
    /// ends of the string, which is where an off-by-one shows up.
    #[test]
    fn email_validation_holds_at_the_ends_of_the_string() {
        let rule = ValidationRule::Email;
        for good in [
            "a@b.c",
            "user@example.com",
            "a@b.c.d",
            "a@b.c.",
            "  a@b.c  ",
        ] {
            assert!(rule.validate(good).valid, "{good:?} should be accepted");
        }
        for bad in [
            "", "@", ".", "a@", "@b.c", "a@.", "a@.c", "a@b.", "a@b", "ab.c", "@.",
        ] {
            assert!(!rule.validate(bad).valid, "{bad:?} should be rejected");
        }
    }

    // -- WhyDisabled --

    const SCREEN: (f32, f32) = (1280.0, 800.0);
    const DELAY: u64 = 500;
    const SAVE: (f32, f32, f32, f32) = (10.0, 10.0, 80.0, 24.0);
    const PRINT: (f32, f32, f32, f32) = (100.0, 10.0, 80.0, 24.0);

    fn controls() -> Vec<Disabled<'static>> {
        vec![
            (SAVE, "The file is read-only"),
            (PRINT, "No printer is set up"),
            ((200.0, 10.0, 80.0, 24.0), ""),
        ]
    }

    fn texts(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// **The reason a disabled control gives appears once the pointer has
    /// rested on it for the tooltip delay**, and is drawn; moving within the
    /// control does not start the wait again.
    #[test]
    fn a_disabled_control_says_why_once_the_pointer_rests_on_it() {
        let mut why = WhyDisabled::new();
        assert!(!why.pointer_at((20.0, 20.0), 1000, SCREEN, &controls()));
        assert_eq!(why.due_in(1000), Some(DELAY));
        assert!(!why.pointer_at((30.0, 20.0), 1200, SCREEN, &controls()));
        assert_eq!(why.due_in(1200), Some(DELAY - 200), "the wait went on");
        assert!(!why.tick(1400));
        assert_eq!(why.showing(), None);

        assert!(why.tick(1000 + DELAY), "it appeared");
        assert!(!why.tick(1000 + DELAY + 10), "it appears once");
        assert_eq!(why.showing(), Some("The file is read-only"));
        assert_eq!(why.due_in(1000 + DELAY), None);
        let palette = Palette::for_mode(false);
        assert!(texts(&why.render(&palette)).contains(&"The file is read-only".to_owned()));
    }

    /// **Another control's reason takes its own wait, and leaving every
    /// control, or the window, puts the reason away** -- each said, so the
    /// owner repaints.
    #[test]
    fn the_reason_goes_with_the_pointer() {
        let mut why = WhyDisabled::new();
        why.pointer_at((20.0, 20.0), 0, SCREEN, &controls());
        why.tick(DELAY);
        assert!(
            why.pointer_at((110.0, 20.0), DELAY, SCREEN, &controls()),
            "the shown reason went"
        );
        assert_eq!(why.showing(), None);
        assert_eq!(why.due_in(DELAY), Some(DELAY));
        assert!(why.tick(2 * DELAY));
        assert_eq!(why.showing(), Some("No printer is set up"));

        assert!(why.pointer_at((500.0, 500.0), 2 * DELAY, SCREEN, &controls()));
        assert_eq!(why.showing(), None);
        assert_eq!(why.due_in(2 * DELAY), None);
        assert!(texts(&why.render(&Palette::for_mode(false))).is_empty());

        why.pointer_at((110.0, 20.0), 0, SCREEN, &controls());
        why.tick(DELAY);
        assert!(why.pointer_left());
        assert!(!why.pointer_left(), "nothing left to put away");
        assert_eq!(why.due_in(0), None);
    }

    /// **A control with no reason says nothing**, and an edge of a box is
    /// in it on its left and top and out of it on its right and bottom, as
    /// every box here is.
    #[test]
    fn only_a_reason_is_said_and_only_inside_the_box() {
        let mut why = WhyDisabled::new();
        why.pointer_at((210.0, 20.0), 0, SCREEN, &controls());
        assert_eq!(why.due_in(0), None, "no reason given");
        assert!(!why.tick(10 * DELAY));

        why.pointer_at((SAVE.0, SAVE.1), 0, SCREEN, &controls());
        assert!(why.due_in(0).is_some(), "the top-left corner is inside");
        why.pointer_at((SAVE.0 + SAVE.2, SAVE.1), 0, SCREEN, &controls());
        assert_eq!(why.due_in(0), None, "the right edge is outside");
        why.pointer_at((SAVE.0, SAVE.1 + SAVE.3), 0, SCREEN, &controls());
        assert_eq!(why.due_in(0), None, "the bottom edge is outside");
    }
}
