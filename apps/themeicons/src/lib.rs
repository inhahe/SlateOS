//! Icons from the user's icon theme, for a program under `oswindow::app`.
//!
//! No font SlateOS has draws emoji, so a picture drawn as an emoji character
//! is a box on screen (`requests/c-e-draw-pictures-from-the-icon-theme.md`).
//! The desktop draws every picture as an icon from the theme the user chose
//! (`appearance::icons`), uploading each to its window before the frame that
//! names it. A program under `oswindow::app` never holds its window -- the
//! loop does -- so it hands its pictures over through `App::take_images`
//! instead, and [`Icons`] is that for icons:
//!
//! - **Drawing**: [`Icons::icon`] answers the image id to draw an icon under,
//!   in a `RenderCommand::Image`.
//! - **After drawing**: [`Icons::frame_drawn`] queues an upload for each icon
//!   the frame names that the window does not hold yet.
//! - **`App::take_images`**: [`Icons::take_images`] hands them to the loop,
//!   which sends them before the frame -- the loop asks after drawing.
//! - **When the appearance changes**: [`Icons::set_theme`] (another icon
//!   theme) and [`Icons::forget`] (other colours) drop every icon from the
//!   window; the next frame names them again, under new ids, drawn anew.
//!
//! An icon's id is the hash of its name, size and colour
//! (`IconRegistry::icon`), so the same icon named in every frame is uploaded
//! once, and one drawn in two colours is two pictures.

use std::borrow::Cow;
use std::collections::BTreeSet;

use appearance::icons::{IconRegistry, IconTheme, upload_missing};
use guitk::canvas::WireBytes;
use guitk::color::Color;
use guitk::render::RenderCommand;
use oswindow::app::ImageChange;

/// A program's icons: what it has drawn, what its window holds, and what is
/// still to be sent. See the module docs.
#[derive(Debug)]
pub struct Icons {
    /// What each id drawn stands for: name, size, colour.
    registry: IconRegistry,
    /// Where the icons are drawn from.
    theme: IconTheme,
    /// The icons the window holds, or will once the queued changes are sent.
    held: BTreeSet<u64>,
    /// Uploads and drops not yet handed to the loop, in order.
    changes: Vec<ImageChange>,
}

impl Icons {
    /// Icons drawn from `theme` -- the user's, `AppearanceSettings::icon_theme`.
    #[must_use]
    pub fn new(theme: IconTheme) -> Self {
        Self {
            registry: IconRegistry::default(),
            theme,
            held: BTreeSet::new(),
            changes: Vec::new(),
        }
    }

    /// The image id under which to draw the icon `name`, `px` pixels square,
    /// in `color` -- the colour the text in its place would have had. A
    /// `RenderCommand::Image` naming it draws the icon once
    /// [`Self::frame_drawn`] has seen that frame. A name the theme does not
    /// draw draws nothing; the names are the freedesktop Icon Naming
    /// Specification's, and a missing one falls back by dropping its last
    /// `-part`.
    pub fn icon(&self, name: impl Into<Cow<'static, str>>, px: u32, color: Color) -> u64 {
        self.registry.icon(name, px, color)
    }

    /// [`Self::icon`], drawing `fallback` where nothing draws `name`: for a
    /// name from outside the program -- a file's type, a desktop entry's icon
    /// -- which the theme may not have.
    pub fn icon_or(
        &self,
        name: impl Into<Cow<'static, str>>,
        fallback: &'static str,
        px: u32,
        color: Color,
    ) -> u64 {
        self.registry.icon_or(name, fallback, px, color)
    }

    /// The frame `commands` has been drawn: queue an upload for each icon it
    /// names that the window does not hold. An icon nothing draws is
    /// remembered all the same, so it is not looked for again every frame.
    pub fn frame_drawn(&mut self, commands: &[RenderCommand]) {
        let Self {
            registry,
            theme,
            held,
            changes,
        } = self;
        let queued: Result<(), std::convert::Infallible> = upload_missing(
            commands,
            theme,
            |id| registry.request(id),
            // Only an id this registry gave: a picture the program uploads
            // itself under an id that happens to carry the icon tag is the
            // program's, never held here, so never dropped by `forget`.
            |id| registry.request(id).is_some() && held.insert(id),
            |id, icon| {
                changes.push(ImageChange::Upload {
                    id,
                    width: icon.size,
                    height: icon.size,
                    stride: icon.size.saturating_mul(4),
                    format: oswindow::PixelFormat::Argb8888,
                    bytes: WireBytes::from_le_argb(&icon.argb),
                });
                Ok(())
            },
        );
        // Queuing cannot fail: the error type has no values.
        let Ok(()) = queued;
    }

    /// The uploads and drops queued since last asked, in order: what
    /// `App::take_images` hands the loop.
    pub fn take_images(&mut self) -> Vec<ImageChange> {
        std::mem::take(&mut self.changes)
    }

    /// Draw from `theme` from now on -- the user chose another icon theme --
    /// every icon drawn again from it ([`Self::forget`]). The same theme
    /// again changes nothing.
    pub fn set_theme(&mut self, theme: IconTheme) {
        if theme != self.theme {
            self.theme = theme;
            self.forget();
        }
    }

    /// Forget every icon and drop it from the window: the colours changed,
    /// so the next frame names its icons under new ids, and the old ones
    /// would only take up the window's image budget. The next frame's are
    /// uploaded afresh.
    pub fn forget(&mut self) {
        self.registry.clear();
        for id in std::mem::take(&mut self.held) {
            self.changes.push(ImageChange::Drop(id));
        }
    }

    /// The theme the icons are drawn from.
    #[must_use]
    pub fn theme(&self) -> &IconTheme {
        &self.theme
    }

    /// Whether `id` is an icon this program drew -- an id [`Self::icon`] or
    /// [`Self::icon_or`] gave since the icons were last forgotten -- rather
    /// than a picture of its own. The tag alone cannot say: a program may
    /// number its own pictures by a hash, half of which carry it.
    #[must_use]
    pub fn drew(&self, id: u64) -> bool {
        self.registry.request(id).is_some()
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::indexing_slicing)]
mod tests;
