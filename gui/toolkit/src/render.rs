//! Render tree — backend-agnostic drawing primitives.
//!
//! The layout engine produces a list of `RenderCommand`s that any
//! rendering backend (compositor, framebuffer, software rasterizer)
//! can consume. This decouples the widget library from any specific
//! graphics API.

use crate::color::Color;
use crate::style::CornerRadii;

/// A render command — one drawing primitive.
#[derive(Clone, Debug)]
pub enum RenderCommand {
    /// Fill a rectangle.
    FillRect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
        corner_radii: CornerRadii,
    },

    /// Draw a rectangle outline (border).
    StrokeRect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
        line_width: f32,
        corner_radii: CornerRadii,
    },

    /// Draw text.
    Text {
        x: f32,
        y: f32,
        text: String,
        color: Color,
        font_size: f32,
        font_weight: FontWeightHint,
        max_width: Option<f32>,
    },

    /// Draw an image/bitmap.
    Image {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        /// Image data ID (reference to image in an asset store).
        image_id: u64,
    },

    /// Draw a line.
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        color: Color,
        width: f32,
    },

    /// Set a clip rectangle (all subsequent commands clipped to this area).
    PushClip {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },

    /// Remove the most recent clip rectangle.
    PopClip,

    /// Apply a transform (translate).
    PushTranslate { dx: f32, dy: f32 },

    /// Remove the most recent transform.
    PopTranslate,

    /// Draw a box shadow.
    BoxShadow {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        offset_x: f32,
        offset_y: f32,
        blur: f32,
        spread: f32,
        color: Color,
        corner_radii: CornerRadii,
    },
}

/// Font weight hint for the renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontWeightHint {
    #[default]
    Regular,
    Bold,
    Light,
}

/// Collected render output from a frame.
#[derive(Clone, Debug, Default)]
pub struct RenderTree {
    pub commands: Vec<RenderCommand>,
}

impl RenderTree {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn push(&mut self, cmd: RenderCommand) {
        self.commands.push(cmd);
    }

    pub fn fill_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
    ) {
        self.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color,
            corner_radii: CornerRadii::ZERO,
        });
    }

    pub fn fill_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
        radii: CornerRadii,
    ) {
        self.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color,
            corner_radii: radii,
        });
    }

    pub fn stroke_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
        line_width: f32,
    ) {
        self.push(RenderCommand::StrokeRect {
            x,
            y,
            width,
            height,
            color,
            line_width,
            corner_radii: CornerRadii::ZERO,
        });
    }

    pub fn text(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        color: Color,
        font_size: f32,
    ) {
        self.push(RenderCommand::Text {
            x,
            y,
            text: text.to_string(),
            color,
            font_size,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    pub fn clip(&mut self, x: f32, y: f32, width: f32, height: f32) {
        self.push(RenderCommand::PushClip {
            x,
            y,
            width,
            height,
        });
    }

    pub fn unclip(&mut self) {
        self.push(RenderCommand::PopClip);
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.push(RenderCommand::PushTranslate { dx, dy });
    }

    pub fn untranslate(&mut self) {
        self.push(RenderCommand::PopTranslate);
    }

    /// Total number of draw commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Clear all commands (reuse allocation for next frame).
    pub fn clear(&mut self) {
        self.commands.clear();
    }
}
