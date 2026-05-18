//! Enable/disable controls system for the GUI toolkit.
//!
//! Provides a comprehensive system for enabling/disabling UI controls with
//! optional reason tooltips that explain WHY something is disabled.
//!
//! # Components
//!
//! - [`DisabledState`] — per-widget enabled/disabled state with optional reason
//! - [`DisabledOverlay`] — rendering helper for disabled appearance
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

#![allow(dead_code)]

use crate::color::Color;
use crate::render::{FontWeightHint, RenderCommand};
use crate::style::CornerRadii;
use crate::widget::WidgetId;

// ---------------------------------------------------------------------------
// DisabledState
// ---------------------------------------------------------------------------

/// Per-widget enabled/disabled state.
#[derive(Clone, Debug, PartialEq)]
pub enum DisabledState {
    /// Normal interaction allowed.
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

impl Default for DisabledState {
    fn default() -> Self {
        Self::Enabled
    }
}

// ---------------------------------------------------------------------------
// DisabledOverlay
// ---------------------------------------------------------------------------

/// Default opacity multiplier for disabled controls.
const DISABLED_OPACITY: f32 = 0.5;

/// Tooltip delay before showing reason (milliseconds).
const TOOLTIP_DELAY_MS: u64 = 500;

/// Tooltip background color (Catppuccin Mocha surface0).
const TOOLTIP_BG: Color = Color::rgb(49, 50, 68);
/// Tooltip text color (Catppuccin Mocha text).
const TOOLTIP_FG: Color = Color::rgb(205, 214, 244);
/// Tooltip border color (Catppuccin Mocha overlay0).
const TOOLTIP_BORDER: Color = Color::rgb(108, 112, 134);

/// Tooltip padding in pixels.
const TOOLTIP_PADDING: f32 = 6.0;
/// Tooltip font size.
const TOOLTIP_FONT_SIZE: f32 = 12.0;
/// Tooltip corner radius.
const TOOLTIP_RADIUS: f32 = 4.0;
/// Tooltip vertical offset from control.
const TOOLTIP_OFFSET_Y: f32 = 4.0;

/// Rendering helper for disabled widget appearance.
///
/// When a control is disabled, the overlay:
/// - Reduces opacity to ~50%
/// - Changes cursor to "not-allowed"
/// - Prevents all mouse/keyboard interaction
/// - Shows tooltip with reason on hover (after standard delay)
pub struct DisabledOverlay {
    /// Whether the mouse is currently hovering over the disabled widget.
    hover: bool,
    /// Accumulated hover time in milliseconds.
    hover_time_ms: u64,
    /// The disabled state to render for.
    state: DisabledState,
}

impl DisabledOverlay {
    /// Create a new overlay for the given disabled state.
    pub fn new(state: DisabledState) -> Self {
        Self {
            hover: false,
            hover_time_ms: 0,
            state,
        }
    }

    /// Update hover state. Returns `true` if tooltip should now be visible.
    pub fn update_hover(&mut self, hovering: bool, elapsed_ms: u64) -> bool {
        if hovering {
            if !self.hover {
                self.hover = true;
                self.hover_time_ms = 0;
            }
            self.hover_time_ms = self.hover_time_ms.saturating_add(elapsed_ms);
        } else {
            self.hover = false;
            self.hover_time_ms = 0;
        }
        self.should_show_tooltip()
    }

    /// Whether the tooltip should be displayed.
    pub fn should_show_tooltip(&self) -> bool {
        self.hover && self.hover_time_ms >= TOOLTIP_DELAY_MS && self.state.reason().is_some()
    }

    /// Get the tooltip delay constant.
    pub fn tooltip_delay_ms() -> u64 {
        TOOLTIP_DELAY_MS
    }

    /// Get the disabled state.
    pub fn state(&self) -> &DisabledState {
        &self.state
    }
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
    commands.iter().map(|cmd| apply_opacity(cmd, opacity)).collect()
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
        } => RenderCommand::Text {
            x: *x,
            y: *y,
            text: text.clone(),
            color: reduce_alpha(*color, opacity),
            font_size: *font_size,
            font_weight: *font_weight,
            max_width: *max_width,
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

/// Render a reason tooltip at the specified position.
///
/// The tooltip is positioned above the control by default. If `above` is false,
/// it is positioned below.
pub fn render_reason_tooltip(reason: &str, x: f32, y: f32, above: bool) -> Vec<RenderCommand> {
    // Estimate text width (rough: 7px per character at 12pt).
    let char_width = TOOLTIP_FONT_SIZE * 0.58;
    let text_width = reason.len() as f32 * char_width;
    let box_width = text_width + TOOLTIP_PADDING * 2.0;
    let box_height = TOOLTIP_FONT_SIZE + TOOLTIP_PADDING * 2.0;

    let box_y = if above {
        y - box_height - TOOLTIP_OFFSET_Y
    } else {
        y + TOOLTIP_OFFSET_Y
    };

    let radii = CornerRadii::all(TOOLTIP_RADIUS);

    vec![
        // Shadow
        RenderCommand::BoxShadow {
            x,
            y: box_y,
            width: box_width,
            height: box_height,
            offset_x: 0.0,
            offset_y: 2.0,
            blur: 4.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 80),
            corner_radii: radii,
        },
        // Background
        RenderCommand::FillRect {
            x,
            y: box_y,
            width: box_width,
            height: box_height,
            color: TOOLTIP_BG,
            corner_radii: radii,
        },
        // Border
        RenderCommand::StrokeRect {
            x,
            y: box_y,
            width: box_width,
            height: box_height,
            color: TOOLTIP_BORDER,
            line_width: 1.0,
            corner_radii: radii,
        },
        // Text
        RenderCommand::Text {
            x: x + TOOLTIP_PADDING,
            y: box_y + TOOLTIP_PADDING,
            text: reason.to_string(),
            color: TOOLTIP_FG,
            font_size: TOOLTIP_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(text_width),
        },
    ]
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
            if group.members().contains(&widget_id) && self.is_group_effectively_disabled(group.id) {
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
            if group.members().contains(&widget_id) {
                if let Some(reason) = self.group_effective_reason(group.id) {
                    return Some(reason);
                }
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
        EnableWhen::AllOf(conditions) => {
            conditions.iter().all(|c| evaluate_condition(c, ctx))
        }
        EnableWhen::AnyOf(conditions) => {
            conditions.iter().any(|c| evaluate_condition(c, ctx))
        }
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
                    ValidationResult::invalid(format!(
                        "Must be at least {} characters",
                        min
                    ))
                } else {
                    ValidationResult::valid()
                }
            }
            Self::MaxLength(max) => {
                if value.len() > *max {
                    ValidationResult::invalid(format!(
                        "Must be at most {} characters",
                        max
                    ))
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

/// Basic email validation: requires non-empty local part, `@`, and a domain
/// with at least one dot.
fn is_valid_email(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    let Some(at_pos) = value.find('@') else {
        return false;
    };
    if at_pos == 0 {
        return false;
    }
    let domain = &value[at_pos + 1..];
    if domain.is_empty() {
        return false;
    }
    // Domain must have at least one dot and no leading/trailing dots.
    let Some(dot_pos) = domain.find('.') else {
        return false;
    };
    if dot_pos == 0 || dot_pos == domain.len() - 1 {
        return false;
    }
    true
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

/// A field registration for the form validator.
struct FormField {
    /// Widget ID of the field.
    widget_id: WidgetId,
    /// Label for error messages.
    label: String,
    /// Validation rules (all must pass).
    rules: Vec<ValidationRule>,
}

/// Form-level validator that checks all fields and controls submit button state.
pub struct FormValidator {
    fields: Vec<FormField>,
    /// Widget ID of the submit button (disabled when form is invalid).
    submit_button: Option<WidgetId>,
    /// Cached validation states per field.
    states: Vec<(WidgetId, ValidationState)>,
}

impl FormValidator {
    /// Create a new form validator.
    pub fn new() -> Self {
        Self {
            fields: Vec::new(),
            submit_button: None,
            states: Vec::new(),
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
        });
        self.states.push((widget_id, ValidationState::valid()));
    }

    /// Validate a single field by its widget ID. Returns the validation state.
    pub fn validate_field(&mut self, widget_id: WidgetId, value: &str) -> &ValidationState {
        let field_idx = self.fields.iter().position(|f| f.widget_id == widget_id);
        let Some(idx) = field_idx else {
            // Field not registered; return a static valid state.
            // We cannot return a reference to a temporary, so ensure there is an entry.
            if self.states.iter().all(|(id, _)| *id != widget_id) {
                self.states.push((widget_id, ValidationState::valid()));
            }
            return &self.states.iter().find(|(id, _)| *id == widget_id).expect("just pushed").1;
        };

        let mut result = ValidationState::valid();
        for rule in &self.fields[idx].rules {
            let r = rule.validate(value);
            if !r.valid {
                result = ValidationState {
                    valid: false,
                    message: r.message,
                };
                break;
            }
        }

        // Update cached state.
        if let Some(entry) = self.states.iter_mut().find(|(id, _)| *id == widget_id) {
            entry.1 = result;
        }

        &self.states.iter().find(|(id, _)| *id == widget_id).expect("state exists").1
    }

    /// Validate all fields with provided values. Returns overall validity.
    pub fn validate_all(&mut self, values: &[(WidgetId, &str)]) -> bool {
        let mut all_valid = true;
        for field in &self.fields {
            let value = values
                .iter()
                .find(|(id, _)| *id == field.widget_id)
                .map(|(_, v)| *v)
                .unwrap_or("");

            let mut state = ValidationState::valid();
            for rule in &field.rules {
                let r = rule.validate(value);
                if !r.valid {
                    state = ValidationState {
                        valid: false,
                        message: r.message,
                    };
                    all_valid = false;
                    break;
                }
            }

            if let Some(entry) = self.states.iter_mut().find(|(id, _)| *id == field.widget_id) {
                entry.1 = state;
            }
        }
        all_valid
    }

    /// Whether the entire form is currently valid (based on last validation run).
    pub fn is_valid(&self) -> bool {
        self.states.iter().all(|(_, s)| s.valid)
    }

    /// Get the validation state of a field.
    pub fn field_state(&self, widget_id: WidgetId) -> Option<&ValidationState> {
        self.states.iter().find(|(id, _)| *id == widget_id).map(|(_, s)| s)
    }

    /// Get the disabled state for the submit button based on form validity.
    pub fn submit_disabled_state(&self) -> DisabledState {
        if self.is_valid() {
            DisabledState::Enabled
        } else {
            // Collect first error message for the reason.
            let reason = self
                .states
                .iter()
                .find(|(_, s)| !s.valid)
                .and_then(|(_, s)| s.message.clone());
            DisabledState::Disabled { reason }
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
    use super::*;

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
        assert!(!evaluate_condition(&EnableWhen::FieldNotEmpty(field_id), &ctx));

        // Non-empty -> condition true.
        ctx.set_field_value(field_id, "hello");
        assert!(evaluate_condition(&EnableWhen::FieldNotEmpty(field_id), &ctx));
    }

    #[test]
    fn condition_checkbox_checked() {
        let cb_id = WidgetId(2);
        let mut ctx = EnableContext::new();

        // Unchecked -> false.
        ctx.set_checkbox_state(cb_id, false);
        assert!(!evaluate_condition(&EnableWhen::CheckboxChecked(cb_id), &ctx));

        // Checked -> true.
        ctx.set_checkbox_state(cb_id, true);
        assert!(evaluate_condition(&EnableWhen::CheckboxChecked(cb_id), &ctx));
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

    // -- Tooltip rendering --

    #[test]
    fn tooltip_renders_four_commands() {
        let commands = render_reason_tooltip("Not available yet", 50.0, 100.0, true);
        // Shadow, fill, stroke, text = 4 commands.
        assert_eq!(commands.len(), 4);
        assert!(matches!(commands[0], RenderCommand::BoxShadow { .. }));
        assert!(matches!(commands[1], RenderCommand::FillRect { .. }));
        assert!(matches!(commands[2], RenderCommand::StrokeRect { .. }));
        assert!(matches!(commands[3], RenderCommand::Text { .. }));
    }

    #[test]
    fn tooltip_above_positions_correctly() {
        let y = 100.0;
        let commands = render_reason_tooltip("Reason", 0.0, y, true);
        if let RenderCommand::FillRect { y: box_y, .. } = &commands[1] {
            assert!(*box_y < y, "Tooltip above should have y < control y");
        } else {
            panic!("Expected FillRect");
        }
    }

    #[test]
    fn tooltip_below_positions_correctly() {
        let y = 100.0;
        let commands = render_reason_tooltip("Reason", 0.0, y, false);
        if let RenderCommand::FillRect { y: box_y, .. } = &commands[1] {
            assert!(*box_y > y, "Tooltip below should have y > control y");
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
        validator.add_field(field, "Username", vec![
            ValidationRule::Required,
            ValidationRule::MinLength(3),
        ]);

        let state = validator.validate_field(field, "ab");
        assert!(!state.valid);
        assert!(state.message.as_deref().unwrap().contains("3 characters"));

        let state = validator.validate_field(field, "abc");
        assert!(state.valid);
    }

    // -- DisabledOverlay tests --

    #[test]
    fn overlay_tooltip_not_shown_initially() {
        let overlay = DisabledOverlay::new(DisabledState::Disabled {
            reason: Some("Offline".into()),
        });
        assert!(!overlay.should_show_tooltip());
    }

    #[test]
    fn overlay_tooltip_shown_after_delay() {
        let mut overlay = DisabledOverlay::new(DisabledState::Disabled {
            reason: Some("Offline".into()),
        });

        // Hover but not long enough.
        overlay.update_hover(true, 200);
        assert!(!overlay.should_show_tooltip());

        // Accumulate past threshold.
        overlay.update_hover(true, 400);
        assert!(overlay.should_show_tooltip());
    }

    #[test]
    fn overlay_tooltip_hidden_on_leave() {
        let mut overlay = DisabledOverlay::new(DisabledState::Disabled {
            reason: Some("Offline".into()),
        });

        overlay.update_hover(true, 600);
        assert!(overlay.should_show_tooltip());

        overlay.update_hover(false, 0);
        assert!(!overlay.should_show_tooltip());
    }

    #[test]
    fn overlay_no_tooltip_without_reason() {
        let mut overlay = DisabledOverlay::new(DisabledState::Disabled { reason: None });
        overlay.update_hover(true, 1000);
        // No reason -> no tooltip even after delay.
        assert!(!overlay.should_show_tooltip());
    }
}
