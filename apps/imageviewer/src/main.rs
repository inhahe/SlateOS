//! Slate OS Image Viewer
//!
//! Graphical photo/image viewer with:
//! - Image display with zoom, pan, rotation, and flip transforms
//! - Directory browsing (next/prev image navigation)
//! - Slideshow mode with configurable intervals
//! - Image format detection (BMP, PNG, JPEG, GIF)
//! - Image information panel with metadata/EXIF display
//! - Toolbar and status bar
//! - Keyboard shortcuts for all operations
//!
//! Uses the guitk library for UI rendering.

#[allow(unused_imports)]
use appearance::Palette;
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
#[allow(unused_imports)]
use guitk::style::CornerRadii;
use guitk::theme::with_alpha;
use guitk::wheel;

use std::path::{Path, PathBuf};

/// `path`'s file name as the window shows it: the name itself when it is
/// text, its bytes as escapes (`quoting::escape_unprintable`) when it is not
/// -- never a lossy decode, which shows two such names alike.
fn shown_file_name(path: &std::path::Path) -> Option<String> {
    let name = path.file_name()?;
    Some(name.to_str().map_or_else(
        || quoting::escape_unprintable(name.as_encoded_bytes()),
        str::to_owned,
    ))
}
use std::process::ExitCode;

// ============================================================================
// Constants
// ============================================================================

const TOOLBAR_HEIGHT: f32 = 40.0;

/// Every key this program answers, and what it does.
///
/// Twenty-one bindings and, until this list existed, no way to learn one but
/// reading the source. `B` and `S` are the worst of them: each is the only way
/// to bring back the bar it hides, so pressing one once removes the thing that
/// would have said how to undo it. A toolbar cannot advertise the key that
/// hides the toolbar.
///
/// **Each row is a key this program actually answers**, which is not a
/// property the list has on its own: `every_advertised_key_does_something`
/// walks it, reads each label with `guitk::shortcut` and presses every key it
/// names. `apps/rssreader` shipped an overlay of twenty-one shortcuts of which
/// about four worked.
const SHORTCUTS: &[(&str, &str)] = &[
    ("Ctrl+O", "Open a picture"),
    ("Left / Right", "Previous / next image"),
    ("Home / End", "First / last image"),
    ("Ctrl+= / Ctrl+-", "Zoom in / out"),
    ("Ctrl+0", "Fit the image to the window"),
    ("Ctrl+1", "Actual size"),
    ("Ctrl+R / Ctrl+Shift+R", "Rotate right / left"),
    ("Ctrl+H / Ctrl+V", "Flip across / down"),
    ("I", "Show or hide the image details"),
    ("T", "Show or hide the thumbnails"),
    ("B", "Show or hide the toolbar"),
    ("S", "Show or hide the status bar"),
    ("D", "How long each slide stays up"),
    ("F5", "Start or stop the slideshow"),
    ("Space", "Pause or resume the slideshow"),
    ("F11", "Full screen"),
    ("Delete", "Delete this image"),
    ("Escape", "Leave full screen or the slideshow"),
    ("F1 / ?", "This list"),
];
const STATUS_BAR_HEIGHT: f32 = 28.0;
const INFO_PANEL_WIDTH: f32 = 280.0;
const THUMBNAIL_STRIP_HEIGHT: f32 = 80.0;
/// One thumbnail's side, and the gap before each.
const THUMB_SIZE: f32 = 60.0;
const THUMB_PAD: f32 = 4.0;
/// From one thumbnail's left edge to the next one's.
const THUMB_PITCH: f32 = THUMB_SIZE + THUMB_PAD;

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;
const ZOOM_STEP: f32 = 0.25;

// The colours live in the user's palette, not here.
//
// Ten constants used to sit here: a private grey ladder, so a light desktop
// got a dark image viewer (design-decisions 822). Mapped by the palette's own
// documented meanings rather than by nearest grey, which is why the info
// panel is `mantle` -- 'a sidebar beside a content pane' -- even though that
// is a step *down* from `base` where the old constant was a step up.
//
// One collapse: the toolbar and the status bar were 48 and 38, a contrast
// ratio of 1.1 apart, and both are now `surface0`. They are never adjacent
// except across the thumbnail strip, where a `border` hairline separates
// them and does the work the 10/255 never did.

/// Supported image file extensions for directory browsing.
const IMAGE_EXTENSIONS: &[&str] = &[
    "bmp", "png", "jpg", "jpeg", "gif", "webp", "ico", "tiff", "tif", "svg",
];

// ============================================================================
// Image format detection
// ============================================================================

/// Detected image format from magic bytes.
///
/// Every format `imagecodec` decodes, so a file that fails is named for what
/// it claims to be. WebP, ICO and TIFF were missing -- they decode, and a
/// broken one was reported as a file of no known format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Bmp,
    Png,
    Jpeg,
    Gif,
    WebP,
    Ico,
    Tiff,
    Unknown,
}

impl ImageFormat {
    /// Detect image format from the first bytes of a file.
    pub fn detect(data: &[u8]) -> Self {
        if data.len() < 8 {
            return Self::Unknown;
        }

        // BMP: starts with "BM"
        if byteread::starts_with(data, b"BM") {
            return Self::Bmp;
        }

        // PNG: 8-byte signature
        if byteread::starts_with(data, &[137, 80, 78, 71, 13, 10, 26, 10]) {
            return Self::Png;
        }

        // JPEG: starts with FF D8
        if byteread::starts_with(data, &[0xFF, 0xD8]) {
            return Self::Jpeg;
        }

        // GIF: starts with "GIF87a" or "GIF89a"
        if byteread::starts_with(data, b"GIF87a") || byteread::starts_with(data, b"GIF89a") {
            return Self::Gif;
        }

        // WebP: a RIFF container whose form type is "WEBP".
        if byteread::starts_with(data, b"RIFF") && data.get(8..12) == Some(b"WEBP") {
            return Self::WebP;
        }

        // ICO: a reserved zero, then type 1 -- or 2, a cursor, which is an
        // icon with a hot spot and which `imagecodec` reads the same way.
        if byteread::starts_with(data, &[0, 0, 1, 0]) || byteread::starts_with(data, &[0, 0, 2, 0])
        {
            return Self::Ico;
        }

        // TIFF: the byte order, then 42 -- or 43 for BigTIFF -- in that order.
        if [b"II*\0", b"MM\0*", b"II+\0", b"MM\0+"]
            .iter()
            .any(|magic| byteread::starts_with(data, *magic))
        {
            return Self::Tiff;
        }

        Self::Unknown
    }

    /// Human-readable name for the format.
    pub fn name(self) -> &'static str {
        match self {
            Self::Bmp => "BMP",
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Gif => "GIF",
            Self::WebP => "WebP",
            Self::Ico => "ICO",
            Self::Tiff => "TIFF",
            Self::Unknown => "Unknown",
        }
    }
}

// ============================================================================
// Image data
// ============================================================================

/// The picture currently on screen, as the compositor holds it.
///
/// This used to carry a grey checkerboard: `display_image` called
/// `ImageData::placeholder` under a comment reading "in a real implementation,
/// this would decode the image", so every photograph in the system opened as
/// the same 16-pixel check pattern at the right *size*. The size came from the
/// header, which is why nothing looked obviously broken and why the whole test
/// suite passed over it.
#[derive(Clone, Debug)]
pub struct ImageData {
    /// Width in pixels, as decoded — not as the header claimed.
    pub width: u32,
    /// Height in pixels, as decoded.
    pub height: u32,
    /// The number [`RenderCommand::Image`] names it by.
    ///
    /// The pixels themselves are deliberately *not* here. They are moved
    /// straight into the upload queue and thence to the compositor, which is
    /// the only place anything draws from; a retained copy would double the
    /// viewer's memory for a 20-megapixel photograph — 80 MB in this form — to
    /// serve a reader that does not exist.
    pub image_id: u64,
}

/// The image id this viewer uses for whatever picture is on screen.
///
/// One fixed number rather than a per-file hash, and the difference is not
/// cosmetic. A viewer that derived an id from the path would leave every
/// picture it had ever shown resident in the compositor: nothing drops them,
/// and paging through a directory of photographs would climb the connection's
/// image budget until the compositor refused — at which point the *next*
/// picture is the one that fails, for a reason that has nothing to do with it.
///
/// Re-registering an id replaces what is under it, and the budget is measured
/// against what the link would hold *after* the upload, so one id also means
/// the viewer never holds two full-size pictures at once. That is the same
/// property `design-decisions.md` §557 achieves for the wallpaper by dropping
/// before uploading, arrived at more cheaply because a viewer, unlike a
/// slideshow of wallpapers, shows exactly one picture.
const VIEWER_IMAGE_ID: u64 = 1;

// ============================================================================
// Transform state
// ============================================================================

/// Rotation angle in 90-degree increments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    None,
    Cw90,
    Cw180,
    Cw270,
}

impl Rotation {
    /// Rotate clockwise by 90 degrees.
    pub fn rotate_cw(self) -> Self {
        match self {
            Self::None => Self::Cw90,
            Self::Cw90 => Self::Cw180,
            Self::Cw180 => Self::Cw270,
            Self::Cw270 => Self::None,
        }
    }

    /// Rotate counter-clockwise by 90 degrees.
    pub fn rotate_ccw(self) -> Self {
        match self {
            Self::None => Self::Cw270,
            Self::Cw90 => Self::None,
            Self::Cw180 => Self::Cw90,
            Self::Cw270 => Self::Cw180,
        }
    }

    /// Angle in degrees for display purposes.
    pub fn degrees(self) -> u16 {
        match self {
            Self::None => 0,
            Self::Cw90 => 90,
            Self::Cw180 => 180,
            Self::Cw270 => 270,
        }
    }
}

/// Complete transform state for the viewed image.
#[derive(Clone, Debug)]
pub struct Transform {
    pub zoom: f32,
    pub pan_x: f32,
    pub pan_y: f32,
    pub rotation: Rotation,
    pub flip_h: bool,
    pub flip_v: bool,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
            rotation: Rotation::None,
            flip_h: false,
            flip_v: false,
        }
    }
}

impl Transform {
    /// Reset all transforms to default.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Zoom in by one step, clamping to MAX_ZOOM.
    pub fn zoom_in(&mut self) {
        self.zoom = (self.zoom + ZOOM_STEP).min(MAX_ZOOM);
    }

    /// Zoom out by one step, clamping to MIN_ZOOM.
    pub fn zoom_out(&mut self) {
        self.zoom = (self.zoom - ZOOM_STEP).max(MIN_ZOOM);
    }
}

// ============================================================================
// Slideshow state
// ============================================================================

/// Slideshow interval options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlideshowInterval {
    ThreeSeconds,
    FiveSeconds,
    TenSeconds,
    ThirtySeconds,
}

impl SlideshowInterval {
    /// Duration in milliseconds.
    pub fn millis(self) -> u64 {
        match self {
            Self::ThreeSeconds => 3000,
            Self::FiveSeconds => 5000,
            Self::TenSeconds => 10000,
            Self::ThirtySeconds => 30000,
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::ThreeSeconds => "3s",
            Self::FiveSeconds => "5s",
            Self::TenSeconds => "10s",
            Self::ThirtySeconds => "30s",
        }
    }

    /// Cycle to the next interval option.
    pub fn next(self) -> Self {
        match self {
            Self::ThreeSeconds => Self::FiveSeconds,
            Self::FiveSeconds => Self::TenSeconds,
            Self::TenSeconds => Self::ThirtySeconds,
            Self::ThirtySeconds => Self::ThreeSeconds,
        }
    }
}

/// Slideshow mode state.
#[derive(Clone, Debug)]
pub struct SlideshowState {
    pub active: bool,
    pub interval: SlideshowInterval,
    pub elapsed_ms: u64,
    pub paused: bool,
    pub random_order: bool,
}

impl Default for SlideshowState {
    fn default() -> Self {
        Self {
            active: false,
            interval: SlideshowInterval::FiveSeconds,
            elapsed_ms: 0,
            paused: false,
            random_order: false,
        }
    }
}

// ============================================================================
// Image metadata / EXIF
// ============================================================================

/// Image metadata and EXIF information.
#[derive(Clone, Debug, Default)]
pub struct ImageInfo {
    pub filename: String,
    pub file_size: u64,
    pub format: Option<ImageFormat>,
    pub width: u32,
    pub height: u32,
    pub color_depth: Option<u8>,
    pub dpi: Option<(u32, u32)>,
    pub date_modified: Option<String>,
    // EXIF fields (populated if available)
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub exposure_time: Option<String>,
    pub iso: Option<u32>,
    pub aperture: Option<String>,
    pub focal_length: Option<String>,
}

impl ImageInfo {
    /// Format the file size for display.
    pub fn file_size_display(&self) -> String {
        guitk::bytes::iec(self.file_size)
    }

    /// Dimensions as a display string.
    pub fn dimensions_display(&self) -> String {
        format!("{} x {}", self.width, self.height)
    }
}

// ============================================================================
// Directory entry for browsing
// ============================================================================

/// A single image file entry in the current directory listing.
#[derive(Clone, Debug)]
pub struct DirectoryEntry {
    pub path: PathBuf,
    pub filename: String,
    pub file_size: u64,
}

// ============================================================================
// Toolbar button
// ============================================================================

/// Toolbar button definition.
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct ToolbarButton {
    label: &'static str,
    tooltip: &'static str,
    action: ViewerAction,
    x: f32,
    width: f32,
}

/// Actions triggered by toolbar buttons or keyboard shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerAction {
    Open,
    PrevImage,
    NextImage,
    ZoomIn,
    ZoomOut,
    FitToWindow,
    ActualSize,
    RotateCw,
    RotateCcw,
    FlipHorizontal,
    FlipVertical,
    ToggleSlideshow,
    ToggleInfo,
    ToggleThumbnails,
    ToggleToolbar,
    ToggleStatusBar,
    ToggleFullscreen,
    FirstImage,
    LastImage,
    DeleteImage,
    PauseSlideshow,
}

// ============================================================================
// Main viewer state
// ============================================================================

/// Complete state for the image viewer application.
pub struct ViewerState {
    /// The user's colours, handed over by the framework (§822).
    pub palette: Palette,
    // Window dimensions
    pub window_width: f32,
    pub window_height: f32,
    pub fullscreen: bool,

    // Current image
    pub current_image: Option<ImageData>,
    pub image_info: ImageInfo,
    pub transform: Transform,
    /// Why the last load attempt produced no image, if it produced none.
    /// Rendered in place of the picture, because a viewer that shows a
    /// filename with an empty canvas and no explanation looks broken rather
    /// than informative.
    pub load_error: Option<String>,

    // Directory browsing
    pub directory: Option<PathBuf>,
    pub entries: Vec<DirectoryEntry>,
    pub current_index: usize,

    // UI panels
    pub show_info_panel: bool,
    pub show_thumbnails: bool,
    pub show_toolbar: bool,
    /// Whether the shortcut list is up.
    pub show_help: bool,
    pub show_status_bar: bool,

    // Slideshow
    pub slideshow: SlideshowState,

    // Mouse interaction state
    pub dragging: bool,
    pub drag_start_x: f32,
    pub drag_start_y: f32,
    pub drag_start_pan_x: f32,
    pub drag_start_pan_y: f32,

    // Toolbar hover state
    pub hovered_button: Option<usize>,

    /// The fraction of a zoom step the wheel has earned but not yet spent.
    ///
    /// The zoom is a *stepped* quantity -- `zoom_in` adds a fixed `ZOOM_STEP`
    /// -- so this is the accumulator case, not the continuous one. Without it
    /// the handler read only the sign of `dy` and took a full step per event,
    /// which on a precision trackpad (a stream of 0.05-notch events) ran the
    /// zoom from minimum to maximum on a single flick of two fingers.
    zoom_wheel: wheel::Accumulator,

    /// Pictures the compositor should start or stop holding, in order, not yet
    /// sent.
    ///
    /// Queued rather than sent at the point of decode because `display_image`
    /// has no connection and must not need one: every test in this file drives
    /// the viewer with no compositor anywhere, and a loader that dialled a
    /// display would make the whole suite either offline-and-untested or
    /// online-and-fragile. `oswindow::app::drive` drains this through
    /// [`App::take_images`] between the render and the frame, which is what
    /// puts the pixels up before the frame that names them.
    pending_images: Vec<oswindow::app::ImageChange>,

    /// The Open dialog, while it is up.
    ///
    /// It takes every key and click until it closes, so a letter typed into a
    /// file name is not also a shortcut acting on the picture behind it --
    /// `B` in `beach.jpg` would hide the toolbar.
    picker: guitk::dialog::FilePicker,
}

/// Where each part of the window is, for the frame about to be drawn.
///
/// The one reading of the window's geometry. `render` draws each part where
/// this says it is and the pointer handler hit-tests the same rectangles, so a
/// click cannot land on a button that is drawn somewhere else -- the fault
/// that left this viewer's toolbar and thumbnail strip drawn and unclickable:
/// the renderer knew where they were and the mouse handler never asked.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    /// The toolbar's top, when it is shown.
    toolbar: Option<f32>,
    /// The picture's area: left, top, width, height.
    image: (f32, f32, f32, f32),
    /// The info panel's left edge, when it is shown. It runs down the
    /// picture's area, beside it.
    info: Option<f32>,
    /// The thumbnail strip's top, when it is shown.
    thumbs: Option<f32>,
    /// The status bar's top, when it is shown.
    status: Option<f32>,
}

impl Layout {
    /// Whether (`x`, `y`) is over the picture's area.
    fn in_image(&self, x: f32, y: f32) -> bool {
        let (left, top, width, height) = self.image;
        x >= left && x < left + width && y >= top && y < top + height
    }
}

impl ViewerState {
    /// Create a new viewer state with default settings.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            window_width: width,
            window_height: height,
            fullscreen: false,
            current_image: None,
            image_info: ImageInfo::default(),
            load_error: None,
            transform: Transform::default(),
            directory: None,
            entries: Vec::new(),
            current_index: 0,
            show_info_panel: false,
            show_thumbnails: false,
            show_toolbar: true,
            show_help: false,
            show_status_bar: true,
            slideshow: SlideshowState::default(),
            dragging: false,
            drag_start_x: 0.0,
            drag_start_y: 0.0,
            drag_start_pan_x: 0.0,
            drag_start_pan_y: 0.0,
            hovered_button: None,
            zoom_wheel: wheel::Accumulator::default(),
            pending_images: Vec::new(),
            picker: guitk::dialog::FilePicker::new(),
        }
    }

    /// Where each part of the window is. See [`Layout`].
    ///
    /// Full screen hides the two bars and nothing else: the thumbnail strip
    /// and the info panel are the user's to show there, and `T` and `I`
    /// still answer.
    fn layout(&self) -> Layout {
        let bars = !self.fullscreen;
        let toolbar = (self.show_toolbar && bars).then_some(0.0);
        let top = if toolbar.is_some() {
            TOOLBAR_HEIGHT
        } else {
            0.0
        };
        let status =
            (self.show_status_bar && bars).then_some(self.window_height - STATUS_BAR_HEIGHT);
        let bottom = status.unwrap_or(self.window_height);
        let thumbs = self
            .show_thumbnails
            .then_some(bottom - THUMBNAIL_STRIP_HEIGHT);
        let image_bottom = thumbs.unwrap_or(bottom);
        let image_width = if self.show_info_panel {
            self.window_width - INFO_PANEL_WIDTH
        } else {
            self.window_width
        };
        Layout {
            toolbar,
            image: (0.0, top, image_width, image_bottom - top),
            info: self.show_info_panel.then_some(image_width),
            thumbs,
            status,
        }
    }

    /// The toolbar button under (`x`, `y`), by its place in
    /// [`toolbar_buttons`].
    fn toolbar_button_at(&self, x: f32, y: f32) -> Option<usize> {
        let (top, height) = toolbar_button_band(self.layout().toolbar?);
        if y < top || y >= top + height {
            return None;
        }
        toolbar_buttons()
            .iter()
            .position(|b| x >= b.x && x < b.x + b.width)
    }

    /// The thumbnails the strip has room for: each one's place in
    /// [`Self::entries`] and its left edge, centred on the current picture.
    ///
    /// Read by the strip's drawing and by its hit-test alike, so a click lands
    /// on the picture drawn under the pointer.
    fn thumbnail_slots(&self) -> Vec<(usize, f32)> {
        // Truncated on purpose: a thumbnail that does not fit whole is left
        // out rather than drawn cut off.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let room = (self.window_width / THUMB_PITCH).max(0.0) as usize;
        let start = self.current_index.saturating_sub(room / 2);
        let end = start.saturating_add(room).min(self.entries.len());
        (start..end)
            .zip(0_u16..)
            .map(|(entry, slot)| (entry, f32::from(slot) * THUMB_PITCH + THUMB_PAD))
            .collect()
    }

    /// The thumbnail under (`x`, `y`), by its place in [`Self::entries`].
    fn thumbnail_at(&self, x: f32, y: f32) -> Option<usize> {
        let top = thumbnail_top(self.layout().thumbs?);
        if y < top || y >= top + THUMB_SIZE {
            return None;
        }
        self.thumbnail_slots()
            .into_iter()
            .find(|&(_, left)| x >= left && x < left + THUMB_SIZE)
            .map(|(entry, _)| entry)
    }

    /// Show the picture at `index` in the directory's listing.
    fn go_to_entry(&mut self, index: usize) {
        if index < self.entries.len() {
            self.current_index = index;
            self.load_current_entry();
        }
    }

    /// Put the Open dialog up, in the current picture's directory when there
    /// is one -- the next picture wanted is most often beside the last.
    fn ask_for_a_picture(&mut self) {
        let start = self
            .directory
            .clone()
            .filter(|dir| dir.is_dir())
            .unwrap_or_else(guitk::dialog::FilePicker::default_start);
        let patterns: Vec<String> = IMAGE_EXTENSIONS.iter().map(|e| format!("*.{e}")).collect();
        let patterns: Vec<&str> = patterns.iter().map(String::as_str).collect();
        self.picker.put_up(
            guitk::dialog::FileDialog::open()
                .with_initial_path(start)
                .with_filter("Pictures", &patterns),
            false,
        );
    }

    /// Give the Open dialog first refusal on `event` while it is up.
    fn picker_took(&mut self, event: &Event) -> bool {
        match self
            .picker
            .handle(event, self.window_width, self.window_height)
        {
            guitk::dialog::Picked::Chose(path) => {
                // What it came to is on screen either way: the picture, or
                // why not.
                let _ = self.open_file(&path);
                true
            }
            guitk::dialog::Picked::Handled | guitk::dialog::Picked::Cancelled => true,
            guitk::dialog::Picked::Ignored => false,
        }
    }

    /// Open an image file by path, returning whether it could be loaded.
    ///
    /// The directory listing is rebuilt either way: a file that will not open
    /// is still a place in a directory the user can browse away from, and
    /// leaving them with no next/previous is a second failure on top of the
    /// first.
    pub fn open_file(&mut self, path: &Path) -> bool {
        let loaded = self.display_image(path);

        // Update directory listing. This is only done when opening a file
        // directly (e.g. from the file picker), NOT when navigating within an
        // already-loaded directory — see load_current_entry — so that next/prev
        // don't re-scan and re-sort the directory on every step.
        if let Some(parent) = path.parent() {
            self.load_directory(parent);
            // Find our index in the listing
            self.current_index = self
                .entries
                .iter()
                .position(|e| e.path == path)
                .unwrap_or(0);
        }
        loaded
    }

    /// The most bytes of picture file to read.
    ///
    /// Generous: a lossless photograph at the largest size the compositor can
    /// store runs to tens of megabytes, and a caller has no way to ask for
    /// more. The point is not the number but that there is one --
    /// `std::fs::read` had no bound at all, so a file larger than memory was
    /// read whole before `imagecodec::Limits` was consulted, and those limits
    /// exist precisely to be checked "before any buffer the header describes
    /// is allocated".
    const MAX_PICTURE_BYTES: usize = 256 * 1024 * 1024;

    /// Load and display the image at `path` without touching the directory
    /// listing or `current_index`. Used both by `open_file` (which then
    /// (re)builds the listing) and by `load_current_entry` (which navigates
    /// within the existing listing).
    ///
    /// Returns whether the image could be loaded.
    ///
    /// **Every field is replaced, not updated.** This used to assign into
    /// `self.image_info` field by field and keep the previously-displayed
    /// picture when the read failed, so a file the viewer could not open left
    /// a mixture: the status bar and info panel named the *new* file, while
    /// the canvas still showed the *old* image and the panel still listed the
    /// old dimensions, format and EXIF. "This picture is `holiday.jpg`" is the
    /// one claim an image viewer makes, and it was false in exactly the case
    /// the user most needed to be told about.
    fn display_image(&mut self, path: &Path) -> bool {
        let filename = shown_file_name(path).unwrap_or_else(|| String::from("(unknown)"));

        // Built fresh so nothing can survive from the last image.
        let mut info = ImageInfo {
            filename,
            ..ImageInfo::default()
        };

        let data = match safeio::read_capped(path, Self::MAX_PICTURE_BYTES) {
            Ok(read) if read.truncated => {
                // Refused rather than decoded: the tail of a picture is not
                // optional. A JPEG's scan runs to the end of the file and a
                // PNG's `IEND` is the last chunk, so a cut file decodes to
                // something that is not what the photographer took -- and
                // would be shown without a word about it.
                self.image_info = info;
                self.fail_with(format!(
                    "{} is larger than {} MiB",
                    path.display(),
                    Self::MAX_PICTURE_BYTES / (1024 * 1024)
                ));
                return false;
            }
            Ok(read) => read.bytes,
            Err(e) => {
                // Committed anyway: the user asked for *this* file, so the UI
                // must name this file — but with no image and no borrowed
                // metadata beside it.
                self.image_info = info;
                self.fail_with(format!("{}: {}", path.display(), e));
                return false;
            }
        };

        // `read` already succeeded, so a failing `metadata` is a genuine
        // oddity rather than the ordinary missing-file case; 0 is the honest
        // answer for "unknown" here and the file itself is still displayable.
        info.file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        info.date_modified = std::fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok())
            .map(|_t| String::from("(available)"));

        let format = ImageFormat::detect(&data);
        info.format = Some(format);
        // Read before the decode and from the header alone, because it is the
        // only size available for a picture that will not decode -- a TIFF
        // compressed in a way this system does not read, a file cut short. It
        // is what tells the user the file is the picture they meant.
        //
        // `imagecodec`'s reading, for every format it knows. This app had its
        // own for BMP, JPEG and GIF, which took whatever bytes sat at the
        // offsets a size would be at -- the fault the PNG one had and lost --
        // and gave a JPEG's size as stored rather than as shown, so a portrait
        // photograph's panel said landscape until it had decoded.
        if let Ok((w, h)) = imagecodec::dimensions(&data) {
            info.width = w;
            info.height = h;
        }

        let decoded = imagecodec::decode(&data, imagecodec::Limits::default());
        let image = match decoded {
            Ok(image) => image,
            Err(why) => {
                self.image_info = info;
                self.fail_with(format!(
                    "{}: {}",
                    path.display(),
                    decode_failure(format, &why)
                ));
                return false;
            }
        };

        // The decoder's answer overrides the header's. They agree for any file
        // that decoded at all — `imagecodec` allocates from the header — but
        // saying so once here means nothing downstream has to know which of the
        // two `fit_zoom` and the info panel are reading.
        info.width = image.width;
        info.height = image.height;

        // Cleared, not appended to. Paging through a directory faster than the
        // loop draws — holding an arrow key down, or a slideshow with a short
        // interval — would otherwise queue every picture passed over and upload
        // all of them before one frame, at a full photograph's worth of wire
        // traffic per file nobody sees. Only the last one is ever drawn.
        self.pending_images.clear();
        self.pending_images
            .push(oswindow::app::ImageChange::Upload {
                id: VIEWER_IMAGE_ID,
                width: image.width,
                height: image.height,
                stride: image.stride(),
                format: oswindow::PixelFormat::Argb8888,
                // Moved, not cloned. A 20-megapixel photograph is 80 MB in this
                // form; a copy retained "in case something wants it" would
                // double the viewer's footprint for a reader that does not
                // exist. The compositor is where the pixels live once they are
                // sent, and re-reading the file is how they would come back.
                // Typed rather than `Image::to_argb_bytes`'s bare `Vec<u8>`:
                // the other ARGB byte order cannot reach an upload.
                bytes: guitk::canvas::WireBytes::from_le_argb(&image.pixels),
            });

        self.image_info = info;
        self.current_image = Some(ImageData {
            width: image.width,
            height: image.height,
            image_id: VIEWER_IMAGE_ID,
        });
        self.load_error = None;

        // Reset transform for new image
        self.transform.reset();
        true
    }

    /// Record that there is no picture, and stop paying for the last one.
    ///
    /// The drop is the half that is easy to leave out and expensive to leave
    /// out: with no picture, `render_image` emits no `Image` command at all, so
    /// the pixels the compositor still holds under [`VIEWER_IMAGE_ID`] are
    /// unreachable — and a viewer parked on an unopenable file would go on
    /// charging the connection's image budget for a full-screen photograph
    /// nobody can see.
    fn fail_with(&mut self, reason: String) {
        if self.current_image.take().is_some() {
            self.pending_images.clear();
            self.pending_images
                .push(oswindow::app::ImageChange::Drop(VIEWER_IMAGE_ID));
        }
        self.load_error = Some(reason);
        self.transform.reset();
    }

    /// Load the image file listing for a directory.
    pub fn load_directory(&mut self, dir: &Path) {
        self.directory = Some(dir.to_path_buf());
        self.entries.clear();

        if let Ok(read_dir) = std::fs::read_dir(dir) {
            for entry_result in read_dir {
                let Ok(entry) = entry_result else { continue };
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
                    continue;
                }
                let filename = shown_file_name(&path).unwrap_or_default();
                let file_size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                self.entries.push(DirectoryEntry {
                    path,
                    filename,
                    file_size,
                });
            }
        }

        // Sort alphabetically by filename
        self.entries.sort_by(|a, b| a.filename.cmp(&b.filename));
    }

    /// Navigate to the next image in the directory.
    pub fn next_image(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        // `is_empty` returned false above, so the modulus is non-zero.
        self.current_index = self
            .current_index
            .saturating_add(1)
            .checked_rem(self.entries.len())
            .unwrap_or(0);
        self.load_current_entry();
    }

    /// Navigate to the previous image in the directory.
    pub fn prev_image(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        if self.current_index == 0 {
            self.current_index = self.entries.len().saturating_sub(1);
        } else {
            self.current_index = self.current_index.saturating_sub(1);
        }
        self.load_current_entry();
    }

    /// Navigate to the first image in the directory.
    pub fn first_image(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.current_index = 0;
        self.load_current_entry();
    }

    /// Navigate to the last image in the directory.
    pub fn last_image(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.current_index = self.entries.len().saturating_sub(1);
        self.load_current_entry();
    }

    /// Reload the image at the current index.
    fn load_current_entry(&mut self) {
        if let Some(entry) = self.entries.get(self.current_index) {
            let path = entry.path.clone();
            // Display only — do NOT reload the directory listing (that would
            // wipe the listing and reset current_index on every navigation).
            // The result is dropped on purpose: a file that fails to load is
            // already reported through `load_error`, and navigation must not
            // stop at it — the user's way out of a broken file is the arrow
            // key that got them there.
            let _ = self.display_image(&path);
        }
    }

    /// Compute the fit-to-window zoom level for the current image.
    pub fn fit_zoom(&self) -> f32 {
        let Some(img) = &self.current_image else {
            return 1.0;
        };
        let available_width = self.image_area_width();
        let available_height = self.image_area_height();
        if img.width == 0 || img.height == 0 {
            return 1.0;
        }
        let zoom_x = available_width / img.width as f32;
        let zoom_y = available_height / img.height as f32;
        zoom_x.min(zoom_y).clamp(MIN_ZOOM, MAX_ZOOM)
    }

    /// Apply fit-to-window zoom.
    pub fn fit_to_window(&mut self) {
        self.transform.zoom = self.fit_zoom();
        self.transform.pan_x = 0.0;
        self.transform.pan_y = 0.0;
    }

    /// Set zoom to actual size (1:1 pixels).
    pub fn actual_size(&mut self) {
        self.transform.zoom = 1.0;
        self.transform.pan_x = 0.0;
        self.transform.pan_y = 0.0;
    }

    /// Width of the image display area.
    fn image_area_width(&self) -> f32 {
        self.layout().image.2.max(1.0)
    }

    /// Height of the image display area.
    fn image_area_height(&self) -> f32 {
        self.layout().image.3.max(1.0)
    }

    /// Execute a viewer action.
    pub fn execute_action(&mut self, action: ViewerAction) {
        match action {
            // It was a placeholder -- a comment reading "in a real
            // implementation, this would open a file dialog" under a toolbar
            // button and a tooltip naming Ctrl+O, neither of which did
            // anything. A viewer started with no file had no way to be given
            // one.
            ViewerAction::Open => self.ask_for_a_picture(),
            ViewerAction::PrevImage => self.prev_image(),
            ViewerAction::NextImage => self.next_image(),
            ViewerAction::ZoomIn => self.transform.zoom_in(),
            ViewerAction::ZoomOut => self.transform.zoom_out(),
            ViewerAction::FitToWindow => self.fit_to_window(),
            ViewerAction::ActualSize => self.actual_size(),
            ViewerAction::RotateCw => {
                self.transform.rotation = self.transform.rotation.rotate_cw();
            }
            ViewerAction::RotateCcw => {
                self.transform.rotation = self.transform.rotation.rotate_ccw();
            }
            ViewerAction::FlipHorizontal => {
                self.transform.flip_h = !self.transform.flip_h;
            }
            ViewerAction::FlipVertical => {
                self.transform.flip_v = !self.transform.flip_v;
            }
            ViewerAction::ToggleSlideshow => {
                self.slideshow.active = !self.slideshow.active;
                self.slideshow.elapsed_ms = 0;
                self.slideshow.paused = false;
            }
            ViewerAction::PauseSlideshow => {
                if self.slideshow.active {
                    self.slideshow.paused = !self.slideshow.paused;
                }
            }
            ViewerAction::ToggleInfo => {
                self.show_info_panel = !self.show_info_panel;
            }
            ViewerAction::ToggleThumbnails => {
                self.show_thumbnails = !self.show_thumbnails;
            }
            // Both of these gate a draw and had no writer, so the two bars
            // could not be got out of the way of the picture -- which is the
            // one thing a viewer is for.
            ViewerAction::ToggleToolbar => {
                self.show_toolbar = !self.show_toolbar;
            }
            ViewerAction::ToggleStatusBar => {
                self.show_status_bar = !self.show_status_bar;
            }
            ViewerAction::ToggleFullscreen => {
                self.fullscreen = !self.fullscreen;
            }
            ViewerAction::FirstImage => self.first_image(),
            ViewerAction::LastImage => self.last_image(),
            ViewerAction::DeleteImage => {
                // Would move to trash via OS recycle bin integration
            }
        }
    }

    /// Handle a tick event for slideshow progression.
    pub fn handle_tick(&mut self, elapsed_ms: u64) {
        if !self.slideshow.active || self.slideshow.paused {
            return;
        }
        // The only unbounded accumulator here: it is reset every interval in
        // practice, but a caller is free to pass any elapsed time at all, and
        // a slideshow that stops advancing beats one that aborts.
        self.slideshow.elapsed_ms = self.slideshow.elapsed_ms.saturating_add(elapsed_ms);
        if self.slideshow.elapsed_ms >= self.slideshow.interval.millis() {
            self.slideshow.elapsed_ms = 0;
            self.next_image();
        }
    }

    /// Handle a keyboard event. Returns true if the event was consumed.
    pub fn handle_key_event(&mut self, event: &KeyEvent) -> bool {
        if !event.pressed {
            return false;
        }

        let ctrl = event.modifiers.ctrl;
        let shift = event.modifiers.shift;

        match event.key {
            Key::O if ctrl => {
                self.execute_action(ViewerAction::Open);
                true
            }

            // Navigation
            Key::Left if !ctrl => {
                self.execute_action(ViewerAction::PrevImage);
                true
            }
            Key::Right if !ctrl => {
                self.execute_action(ViewerAction::NextImage);
                true
            }
            Key::Home => {
                self.execute_action(ViewerAction::FirstImage);
                true
            }
            Key::End => {
                self.execute_action(ViewerAction::LastImage);
                true
            }

            // Zoom
            Key::Equals if ctrl => {
                self.execute_action(ViewerAction::ZoomIn);
                true
            }
            Key::Minus if ctrl => {
                self.execute_action(ViewerAction::ZoomOut);
                true
            }
            Key::Num0 if ctrl => {
                self.execute_action(ViewerAction::FitToWindow);
                true
            }
            Key::Num1 if ctrl => {
                self.execute_action(ViewerAction::ActualSize);
                true
            }

            // Rotation / flip
            Key::R if ctrl && !shift => {
                self.execute_action(ViewerAction::RotateCw);
                true
            }
            Key::R if ctrl && shift => {
                self.execute_action(ViewerAction::RotateCcw);
                true
            }
            Key::H if ctrl => {
                self.execute_action(ViewerAction::FlipHorizontal);
                true
            }
            Key::V if ctrl => {
                self.execute_action(ViewerAction::FlipVertical);
                true
            }

            // Panels
            Key::I if !ctrl => {
                self.execute_action(ViewerAction::ToggleInfo);
                true
            }
            Key::T if !ctrl => {
                self.execute_action(ViewerAction::ToggleThumbnails);
                true
            }
            Key::B if !ctrl => {
                self.execute_action(ViewerAction::ToggleToolbar);
                true
            }
            // How long each picture stays up. `SlideshowInterval::next`
            // was written with four options and had no caller, so the module
            // doc's "configurable intervals" described a constant: five
            // seconds, for everybody, with no way to ask for anything else.
            Key::D if !ctrl => {
                self.slideshow.interval = self.slideshow.interval.next();
                // The clock restarts, or shortening the interval can change
                // the picture at once -- which reads as the key advancing the
                // slideshow rather than setting its pace.
                self.slideshow.elapsed_ms = 0;
                true
            }
            Key::S if !ctrl => {
                self.execute_action(ViewerAction::ToggleStatusBar);
                true
            }

            // Slideshow
            Key::F5 => {
                self.execute_action(ViewerAction::ToggleSlideshow);
                true
            }
            Key::Space => {
                self.execute_action(ViewerAction::PauseSlideshow);
                true
            }

            // Fullscreen
            Key::F11 => {
                self.execute_action(ViewerAction::ToggleFullscreen);
                true
            }

            // Delete
            Key::Delete => {
                self.execute_action(ViewerAction::DeleteImage);
                true
            }

            // `?`, which is Shift and the slash key.
            // The shortcut list. `F1` raises it in every app in this tree,
            // including `apps/spreadsheet`, where `?` is a character the
            // program has to be able to type into a cell -- so somebody who
            // has learned one key is never stuck. `?` as well, wherever the
            // program is not obliged to type one.
            Key::F1 => {
                self.show_help = !self.show_help;
                true
            }
            Key::Slash if shift => {
                self.show_help = !self.show_help;
                true
            }
            // Before the plain `Escape` arm below, which would otherwise take
            // this and leave the list up while exiting fullscreen.
            Key::Escape if self.show_help => {
                self.show_help = false;
                true
            }

            // Escape exits fullscreen or slideshow
            Key::Escape => {
                if self.slideshow.active {
                    self.slideshow.active = false;
                    true
                } else if self.fullscreen {
                    self.fullscreen = false;
                    true
                } else {
                    false
                }
            }

            _ => false,
        }
    }

    /// Handle a mouse event. Returns true if the event was consumed.
    pub fn handle_mouse_event(&mut self, event: &MouseEvent) -> bool {
        match &event.kind {
            MouseEventKind::Scroll { dx: _, dy } => {
                // One notch is one zoom step -- `rows_at(.., 1.0)` rather than
                // the default three, because a step is already a coarse move
                // and three of them per detent would overshoot every time.
                //
                // The sign is inverted against the accumulator's list
                // convention on purpose: `dy` positive is away from the user,
                // which scrolls a list *up* (a negative row delta) but is the
                // near-universal gesture for zooming *in*.
                let steps = self.zoom_wheel.rows_at(*dy, 1.0);
                for _ in 0..steps.unsigned_abs() {
                    if steps < 0 {
                        self.transform.zoom_in();
                    } else {
                        self.transform.zoom_out();
                    }
                }
                true
            }
            MouseEventKind::Press(MouseButton::Left) => {
                if let Some(button) = self.toolbar_button_at(event.x, event.y) {
                    if let Some(action) = toolbar_buttons().get(button).map(|b| b.action) {
                        self.execute_action(action);
                    }
                    return true;
                }
                if let Some(entry) = self.thumbnail_at(event.x, event.y) {
                    self.go_to_entry(entry);
                    return true;
                }
                // A drag pans the picture only when it starts on the
                // picture. It started anywhere below the toolbar, so a click
                // on the thumbnail strip or the info panel began a pan.
                if self.layout().in_image(event.x, event.y) {
                    self.dragging = true;
                    self.drag_start_x = event.x;
                    self.drag_start_y = event.y;
                    self.drag_start_pan_x = self.transform.pan_x;
                    self.drag_start_pan_y = self.transform.pan_y;
                    true
                } else {
                    false
                }
            }
            MouseEventKind::Release(MouseButton::Left) => {
                self.dragging = false;
                true
            }
            MouseEventKind::Move if self.dragging => {
                let dx = event.x - self.drag_start_x;
                let dy = event.y - self.drag_start_y;
                self.transform.pan_x = self.drag_start_pan_x + dx;
                self.transform.pan_y = self.drag_start_pan_y + dy;
                true
            }
            // The button under the pointer is lit. `hovered_button` was read
            // by the toolbar's drawing and written by nothing.
            MouseEventKind::Move => {
                let hovered = self.toolbar_button_at(event.x, event.y);
                let changed = hovered != self.hovered_button;
                self.hovered_button = hovered;
                changed
            }
            // Over the picture only: a double click arrives after its second
            // press, so double-clicking a toolbar button pressed it twice and
            // then changed the zoom as well.
            MouseEventKind::DoubleClick(MouseButton::Left)
                if !self.layout().in_image(event.x, event.y) =>
            {
                false
            }
            MouseEventKind::DoubleClick(MouseButton::Left) => {
                // Double-click toggles between fit and actual size
                if (self.transform.zoom - 1.0).abs() < 0.01 {
                    self.fit_to_window();
                } else {
                    self.actual_size();
                }
                true
            }
            _ => false,
        }
    }

    /// Handle any event type dispatched to the viewer.
    pub fn handle_event(&mut self, event: &Event) -> bool {
        if self.picker.is_open() && self.picker_took(event) {
            return true;
        }
        match event {
            Event::Key(key_event) => self.handle_key_event(key_event),
            Event::Mouse(mouse_event) => self.handle_mouse_event(mouse_event),
            Event::Resize { width, height } => {
                self.window_width = *width as f32;
                self.window_height = *height as f32;
                true
            }
            Event::Tick { elapsed_ms } => {
                self.handle_tick(*elapsed_ms);
                true
            }
            _ => false,
        }
    }
}

// ============================================================================
// Rendering
// ============================================================================

/// Render the complete viewer UI into a RenderTree.
pub fn render(state: &ViewerState) -> RenderTree {
    let mut tree = RenderTree::new();

    // Background
    tree.fill_rect(
        0.0,
        0.0,
        state.window_width,
        state.window_height,
        state.palette.base,
    );

    let layout = state.layout();

    // Toolbar (hidden in fullscreen)
    if let Some(y) = layout.toolbar {
        render_toolbar(state, &mut tree, y);
    }

    // Clip to image area and render image
    let (image_x, image_y, image_w, image_h) = layout.image;
    tree.clip(image_x, image_y, image_w, image_h);
    render_image(state, &mut tree, image_x, image_y, image_w, image_h);
    tree.unclip();

    if let Some(x) = layout.info {
        render_info_panel(state, &mut tree, x, image_y, image_h);
    }
    if let Some(y) = layout.thumbs {
        render_thumbnail_strip(state, &mut tree, y);
    }
    // Status bar (hidden in fullscreen)
    if let Some(y) = layout.status {
        render_status_bar(state, &mut tree, y);
    }

    // The Open dialog over the viewer, and under the shortcut list.
    if state.picker.is_open() {
        tree.commands.extend(state.picker.render(
            &state.palette,
            state.window_width,
            state.window_height,
        ));
    }

    // The shortcut list over everything, because it is the one thing a reader
    // asked for explicitly -- and because two of the keys it names hide the
    // bars it would otherwise have to fit between.
    if state.show_help {
        guitk::shortcut::render_card(
            &mut tree,
            &state.palette,
            (state.window_width, state.window_height),
            0.0,
            SHORTCUTS,
            "F1 or ? closes this",
        );
    }

    tree
}

/// Render the toolbar with action buttons.
fn render_toolbar(state: &ViewerState, tree: &mut RenderTree, y: f32) {
    // Toolbar background
    tree.fill_rect(
        0.0,
        y,
        state.window_width,
        TOOLBAR_HEIGHT,
        state.palette.surface0,
    );
    // Bottom border
    tree.fill_rect(
        0.0,
        y + TOOLBAR_HEIGHT - 1.0,
        state.window_width,
        1.0,
        state.palette.border,
    );

    let buttons = toolbar_buttons();
    let (button_y, button_h) = toolbar_button_band(y);

    for (idx, btn) in buttons.iter().enumerate() {
        let bg = if state.hovered_button == Some(idx) {
            state.palette.surface2
        } else {
            state.palette.surface1
        };

        tree.push(RenderCommand::FillRect {
            x: btn.x,
            y: button_y,
            width: btn.width,
            height: button_h,
            color: bg,
            corner_radii: CornerRadii {
                top_left: 3.0,
                top_right: 3.0,
                bottom_right: 3.0,
                bottom_left: 3.0,
            },
        });

        // Button label (centered)
        tree.push(RenderCommand::Text {
            x: btn.x + 4.0,
            y: button_y + 7.0,
            text: btn.label.to_string(),
            color: state.palette.text,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(btn.width - 8.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

/// Render the image in the display area with current transforms.
fn render_image(
    state: &ViewerState,
    tree: &mut RenderTree,
    area_x: f32,
    area_y: f32,
    area_w: f32,
    area_h: f32,
) {
    let Some(img) = &state.current_image else {
        // Two different empty states, and they must not be confused: "you
        // haven't opened anything" versus "the thing you opened would not
        // open". The second used to render as the first, so a corrupt or
        // unreadable file looked exactly like a freshly-started viewer.
        let (headline, detail) = match &state.load_error {
            Some(err) => (String::from("Cannot display this image"), err.clone()),
            None => (
                String::from("No image loaded"),
                // It said "or drag an image here", and no window receives a
                // drop: there is no such event to deliver one.
                String::from("Open a picture with Ctrl+O or the Open button"),
            ),
        };
        tree.push(RenderCommand::Text {
            x: area_x + area_w / 2.0 - 80.0,
            y: area_y + area_h / 2.0 - 8.0,
            text: headline,
            color: state.palette.subtext0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        tree.push(RenderCommand::Text {
            x: area_x + area_w / 2.0 - 100.0,
            y: area_y + area_h / 2.0 + 12.0,
            text: detail,
            color: state.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            // The reason is the whole point of this state; it is worth the
            // width, and elided rather than clipped so a cut is visible.
            max_width: Some(area_w - 32.0),
            overflow: TextOverflow::Ellipsis,
        });
        return;
    };

    let zoom = state.transform.zoom;
    let display_w = img.width as f32 * zoom;
    let display_h = img.height as f32 * zoom;

    // Center the image in the available area, then apply pan offset
    let center_x = area_x + (area_w - display_w) / 2.0 + state.transform.pan_x;
    let center_y = area_y + (area_h - display_h) / 2.0 + state.transform.pan_y;

    // Apply translation for pan
    tree.translate(center_x, center_y);

    // Render the image command
    tree.push(RenderCommand::Image {
        x: 0.0,
        y: 0.0,
        width: display_w,
        height: display_h,
        image_id: img.image_id,
    });

    tree.untranslate();

    // Slideshow overlay indicator
    if state.slideshow.active {
        // The interval is on the badge because `D` changes it, and a
        // setting that can be changed and not seen is a key that appears to
        // do nothing: the next picture is three seconds or thirty away, and
        // either way nothing happens at the moment of pressing.
        let indicator_text = if state.slideshow.paused {
            format!("PAUSED {}", state.slideshow.interval.label())
        } else {
            format!("SLIDE {}", state.slideshow.interval.label())
        };
        let indicator_color = if state.slideshow.paused {
            with_alpha(state.palette.yellow, 200)
        } else {
            with_alpha(state.palette.green, 200)
        };

        // Small badge in top-right of image area
        tree.push(RenderCommand::FillRect {
            x: area_x + area_w - 100.0,
            y: area_y + 8.0,
            width: 92.0,
            height: 24.0,
            color: Color::rgba(0, 0, 0, 160),
            corner_radii: CornerRadii {
                top_left: 4.0,
                top_right: 4.0,
                bottom_right: 4.0,
                bottom_left: 4.0,
            },
        });
        tree.push(RenderCommand::Text {
            x: area_x + area_w - 92.0,
            y: area_y + 14.0,
            text: indicator_text.clone(),
            color: indicator_color,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

/// Render the image information panel on the right side.
fn render_info_panel(state: &ViewerState, tree: &mut RenderTree, x: f32, y: f32, height: f32) {
    // Panel background
    tree.fill_rect(x, y, INFO_PANEL_WIDTH, height, state.palette.mantle);
    // Left border
    tree.fill_rect(x, y, 1.0, height, state.palette.border);

    let pad = 12.0;
    let mut text_y = y + pad;
    let label_x = x + pad;
    let value_x = x + pad + 80.0;
    let line_height = 20.0;

    // Panel title
    tree.push(RenderCommand::Text {
        x: label_x,
        y: text_y,
        text: String::from("Image Information"),
        color: state.palette.text,
        font_size: 13.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(INFO_PANEL_WIDTH - pad * 2.0),
        overflow: TextOverflow::Ellipsis,
    });
    text_y += line_height + 8.0;

    // Separator
    tree.fill_rect(
        label_x,
        text_y,
        INFO_PANEL_WIDTH - pad * 2.0,
        1.0,
        state.palette.border,
    );
    text_y += 8.0;

    let info = &state.image_info;

    // File info section
    let fields: Vec<(&str, String)> = vec![
        ("File:", info.filename.clone()),
        ("Size:", info.file_size_display()),
        ("Dimensions:", info.dimensions_display()),
        (
            "Format:",
            info.format
                .map(|f| f.name().to_string())
                .unwrap_or_else(|| String::from("—")),
        ),
        (
            "Depth:",
            info.color_depth
                .map(|d| format!("{} bpp", d))
                .unwrap_or_else(|| String::from("—")),
        ),
        (
            "DPI:",
            info.dpi
                .map(|(x, y)| format!("{} x {}", x, y))
                .unwrap_or_else(|| String::from("—")),
        ),
        (
            "Modified:",
            info.date_modified
                .clone()
                .unwrap_or_else(|| String::from("—")),
        ),
    ];

    for (label, value) in &fields {
        tree.push(RenderCommand::Text {
            x: label_x,
            y: text_y,
            text: String::from(*label),
            color: state.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        tree.push(RenderCommand::Text {
            x: value_x,
            y: text_y,
            text: value.clone(),
            color: state.palette.text,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(INFO_PANEL_WIDTH - 80.0 - pad * 2.0),
            overflow: TextOverflow::Ellipsis,
        });
        text_y += line_height;
    }

    // EXIF section (if any data available)
    let has_exif = info.camera_make.is_some()
        || info.camera_model.is_some()
        || info.exposure_time.is_some()
        || info.iso.is_some()
        || info.aperture.is_some()
        || info.focal_length.is_some();

    if has_exif {
        text_y += 8.0;
        tree.fill_rect(
            label_x,
            text_y,
            INFO_PANEL_WIDTH - pad * 2.0,
            1.0,
            state.palette.border,
        );
        text_y += 8.0;

        tree.push(RenderCommand::Text {
            x: label_x,
            y: text_y,
            text: String::from("EXIF Data"),
            color: state.palette.text,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        text_y += line_height + 4.0;

        let exif_fields: Vec<(&str, Option<String>)> = vec![
            ("Camera:", info.camera_make.clone()),
            ("Model:", info.camera_model.clone()),
            ("Exposure:", info.exposure_time.clone()),
            ("ISO:", info.iso.map(|v| format!("{}", v))),
            ("Aperture:", info.aperture.clone()),
            ("Focal:", info.focal_length.clone()),
        ];

        for (label, value_opt) in &exif_fields {
            if let Some(value) = value_opt {
                tree.push(RenderCommand::Text {
                    x: label_x,
                    y: text_y,
                    text: String::from(*label),
                    color: state.palette.subtext0,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                tree.push(RenderCommand::Text {
                    x: value_x,
                    y: text_y,
                    text: value.clone(),
                    color: state.palette.text,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(INFO_PANEL_WIDTH - 80.0 - pad * 2.0),
                    overflow: TextOverflow::Ellipsis,
                });
                text_y += line_height;
            }
        }
    }

    // Transform info
    text_y += 8.0;
    tree.fill_rect(
        label_x,
        text_y,
        INFO_PANEL_WIDTH - pad * 2.0,
        1.0,
        state.palette.border,
    );
    text_y += 8.0;

    tree.push(RenderCommand::Text {
        x: label_x,
        y: text_y,
        text: String::from("View"),
        color: state.palette.text,
        font_size: 12.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    text_y += line_height + 4.0;

    let zoom_pct = (state.transform.zoom * 100.0) as u32;
    let view_fields: Vec<(&str, String)> = vec![
        ("Zoom:", format!("{}%", zoom_pct)),
        (
            "Rotation:",
            format!("{}deg", state.transform.rotation.degrees()),
        ),
        (
            "Flip:",
            match (state.transform.flip_h, state.transform.flip_v) {
                (false, false) => String::from("None"),
                (true, false) => String::from("Horizontal"),
                (false, true) => String::from("Vertical"),
                (true, true) => String::from("Both"),
            },
        ),
    ];

    for (label, value) in &view_fields {
        tree.push(RenderCommand::Text {
            x: label_x,
            y: text_y,
            text: String::from(*label),
            color: state.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        tree.push(RenderCommand::Text {
            x: value_x,
            y: text_y,
            text: value.clone(),
            color: state.palette.text,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(INFO_PANEL_WIDTH - 80.0 - pad * 2.0),
            overflow: TextOverflow::Ellipsis,
        });
        text_y += line_height;
    }
}

/// Render the thumbnail strip at the bottom.
fn render_thumbnail_strip(state: &ViewerState, tree: &mut RenderTree, y: f32) {
    // Background
    tree.fill_rect(
        0.0,
        y,
        state.window_width,
        THUMBNAIL_STRIP_HEIGHT,
        state.palette.surface0,
    );
    // Top border
    tree.fill_rect(0.0, y, state.window_width, 1.0, state.palette.border);

    let thumb_size = THUMB_SIZE;
    let thumb_y = thumbnail_top(y);

    for (abs_idx, thumb_x) in state.thumbnail_slots() {
        let is_current = abs_idx == state.current_index;

        // Thumbnail border (highlight current)
        let border_color = if is_current {
            state.palette.accent
        } else {
            state.palette.border
        };
        tree.push(RenderCommand::StrokeRect {
            x: thumb_x,
            y: thumb_y,
            width: thumb_size,
            height: thumb_size,
            color: border_color,
            line_width: if is_current { 2.0 } else { 1.0 },
            corner_radii: CornerRadii::ZERO,
        });

        // Thumbnail placeholder (would use actual thumbnails)
        tree.push(RenderCommand::FillRect {
            x: thumb_x + 1.0,
            y: thumb_y + 1.0,
            width: thumb_size - 2.0,
            height: thumb_size - 2.0,
            color: state.palette.surface1,
            corner_radii: CornerRadii::ZERO,
        });

        // Filename label below (truncated)
        if let Some(entry) = state.entries.get(abs_idx) {
            let display_name = if entry.filename.len() > 8 {
                let truncated: String = entry.filename.chars().take(7).collect();
                format!("{}~", truncated)
            } else {
                entry.filename.clone()
            };
            tree.push(RenderCommand::Text {
                x: thumb_x + 2.0,
                y: thumb_y + thumb_size - 12.0,
                text: display_name,
                color: if is_current {
                    state.palette.text
                } else {
                    state.palette.subtext0
                },
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(thumb_size - 4.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }
}

/// Render the status bar at the bottom.
fn render_status_bar(state: &ViewerState, tree: &mut RenderTree, y: f32) {
    // Background
    tree.fill_rect(
        0.0,
        y,
        state.window_width,
        STATUS_BAR_HEIGHT,
        state.palette.surface0,
    );
    // Top border
    tree.fill_rect(0.0, y, state.window_width, 1.0, state.palette.border);

    let text_y = y + 8.0;
    let pad = 10.0;

    // Left: filename
    tree.push(RenderCommand::Text {
        x: pad,
        y: text_y,
        text: state.image_info.filename.clone(),
        color: state.palette.text,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(state.window_width * 0.4),
        overflow: TextOverflow::Ellipsis,
    });

    // Center: dimensions
    let dims = state.image_info.dimensions_display();
    tree.push(RenderCommand::Text {
        x: state.window_width * 0.4,
        y: text_y,
        text: dims,
        color: state.palette.subtext0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Right side: zoom level + image position
    let zoom_pct = (state.transform.zoom * 100.0) as u32;
    let zoom_text = format!("{}%", zoom_pct);
    tree.push(RenderCommand::Text {
        x: state.window_width - 160.0,
        y: text_y,
        text: zoom_text,
        color: state.palette.subtext0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Image N of M
    if !state.entries.is_empty() {
        let pos_text = format!(
            "{} / {}",
            state.current_index.saturating_add(1),
            state.entries.len()
        );
        tree.push(RenderCommand::Text {
            x: state.window_width - 80.0,
            y: text_y,
            text: pos_text,
            color: state.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

/// The toolbar's buttons' top and height, for a toolbar whose top is `top`.
fn toolbar_button_band(top: f32) -> (f32, f32) {
    (top + 6.0, TOOLBAR_HEIGHT - 12.0)
}

/// The top of the thumbnails in a strip whose top is `strip_top`.
fn thumbnail_top(strip_top: f32) -> f32 {
    strip_top + (THUMBNAIL_STRIP_HEIGHT - THUMB_SIZE) / 2.0
}

/// Build the toolbar button definitions with positions.
fn toolbar_buttons() -> Vec<ToolbarButton> {
    let mut buttons = Vec::new();
    let mut x = 8.0;
    let gap = 4.0;

    let defs: &[(&str, &str, ViewerAction, f32)] = &[
        ("Open", "Open file (Ctrl+O)", ViewerAction::Open, 44.0),
        ("|<", "Previous (Left)", ViewerAction::PrevImage, 28.0),
        (">|", "Next (Right)", ViewerAction::NextImage, 28.0),
        ("+", "Zoom in (Ctrl++)", ViewerAction::ZoomIn, 24.0),
        ("-", "Zoom out (Ctrl+-)", ViewerAction::ZoomOut, 24.0),
        (
            "Fit",
            "Fit to window (Ctrl+0)",
            ViewerAction::FitToWindow,
            32.0,
        ),
        (
            "1:1",
            "Actual size (Ctrl+1)",
            ViewerAction::ActualSize,
            32.0,
        ),
        ("CW", "Rotate CW (Ctrl+R)", ViewerAction::RotateCw, 30.0),
        (
            "CCW",
            "Rotate CCW (Ctrl+Shift+R)",
            ViewerAction::RotateCcw,
            36.0,
        ),
        ("H", "Flip H (Ctrl+H)", ViewerAction::FlipHorizontal, 24.0),
        ("V", "Flip V (Ctrl+V)", ViewerAction::FlipVertical, 24.0),
        (
            "Show",
            "Slideshow (F5)",
            ViewerAction::ToggleSlideshow,
            42.0,
        ),
        ("Info", "Info panel (I)", ViewerAction::ToggleInfo, 36.0),
    ];

    for &(label, tooltip, action, width) in defs {
        buttons.push(ToolbarButton {
            label,
            tooltip,
            action,
            x,
            width,
        });
        x += width + gap;
    }

    buttons
}

// ============================================================================
// Utility functions
// ============================================================================

/// Why a file that was read could not be shown, in the user's terms.
///
/// The decoder's own message is written for whoever has to fix the *file* —
/// "not a picture format this system reads" — and is the wrong sentence for the
/// commonest case by far, which is a perfectly ordinary JPEG that this system
/// cannot decode yet. Those two are opposite diagnoses: one says the file is
/// wrong, the other says the viewer is, and telling a user their holiday
/// photograph is not a picture is the sort of confident wrong answer that sends
/// people looking for a corrupt disk.
///
/// So the format that [`ImageFormat::detect`] recognised is folded in. It reads
/// the signature independently of `imagecodec`, which is what lets the two
/// disagree usefully: "these bytes begin like a JPEG" and "no decoder here
/// claims them" together mean *unsupported*, not *unrecognised*.
fn decode_failure(format: ImageFormat, why: &imagecodec::ImageError) -> String {
    match (format, why) {
        // It begins as a format does and the decoder did not take it: the
        // file is wrong, not this program.
        //
        // This said "BMP images cannot be displayed yet" (and GIF), a sentence
        // about this program that stopped being true when `imagecodec` learned
        // them -- as it did for JPEG before, which left this list for the same
        // reason. Every format named here now decodes, so a named file the
        // decoder does not claim is one whose first bytes are all it has.
        (named, imagecodec::ImageError::UnknownFormat) if named != ImageFormat::Unknown => {
            format!(
                "it begins as a {} file does, but is not one this system can read",
                named.name()
            )
        }
        _ => why.to_string(),
    }
}

/// Check if a file extension represents a supported image format.
pub fn is_image_extension(ext: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

// ============================================================================
// The window
// ============================================================================

impl oswindow::app::App for ViewerState {
    /// Adopt the user's colours (§822).
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    /// The file's name first, then the application's.
    ///
    /// That order is what a task bar full of windows needs: the strip of
    /// buttons is elided from the right, so a viewer that led with its own name
    /// would give every open picture the same visible label.
    fn title(&self) -> String {
        if self.image_info.filename.is_empty() {
            String::from("Image Viewer")
        } else {
            format!("{} — Image Viewer", self.image_info.filename)
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        // Truncating a size that was built from two integers a moment ago, and
        // clamped so a negative or absurd float cannot become a nonsense
        // request. The render vocabulary is `f32` throughout, so the cast has
        // to happen somewhere.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (
            self.window_width.clamp(1.0, 16384.0) as u32,
            self.window_height.clamp(1.0, 16384.0) as u32,
        )
    }

    /// A clock only while a slideshow is running.
    ///
    /// Consulted after every event, so starting a slideshow with F5 arms the
    /// clock and stopping it disarms one — which is what stops a viewer left
    /// open on one photograph from holding the whole desktop awake. The
    /// interval is the slideshow's own, not a fixed frame rate: a five-second
    /// slideshow that woke sixty times a second to discover that four seconds
    /// remained would be 299 wake-ups spent on arithmetic.
    fn tick_interval(&self) -> Option<std::time::Duration> {
        (self.slideshow.active && !self.slideshow.paused)
            .then(|| std::time::Duration::from_millis(self.slideshow.interval.millis()))
    }

    fn on_event(&mut self, event: &Event) -> oswindow::app::Response {
        if matches!(event, Event::CloseRequested) {
            return oswindow::app::Response::Exit;
        }
        if self.handle_event(event) {
            oswindow::app::Response::Redraw
        } else {
            oswindow::app::Response::Idle
        }
    }

    fn take_images(&mut self) -> Vec<oswindow::app::ImageChange> {
        std::mem::take(&mut self.pending_images)
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The size the compositor last reported wins over the one the viewer
        // remembers. They agree whenever a `Resize` event was delivered, and
        // the case where they do not — the very first frame, drawn before any
        // event has arrived — is exactly the one that would otherwise be drawn
        // at the size this viewer *asked* for rather than the size it got.
        self.window_width = width;
        self.window_height = height;
        render(self)
    }
}

// ============================================================================
// Application entry point
// ============================================================================

fn main() -> ExitCode {
    // Parsed rather than indexed, so `--display` reaches the connection and
    // does not get mistaken for a file called `--display`. `rest` is this
    // viewer's own argument list; `launch_with` is the entry point for an
    // application that has taken its own arguments, `launch` the one for an
    // application with none.
    let args = match oswindow::app::Args::from_env() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("imageviewer: {e}");
            return ExitCode::from(2);
        }
    };
    if args.rest.len() > 1 {
        // One window, one picture. A second file would silently be the one not
        // shown, which is worse than saying so: the user would conclude the
        // viewer had opened it and simply drawn the wrong one.
        eprintln!(
            "imageviewer: only one file at a time; open a directory's other images with the arrow keys"
        );
        return ExitCode::from(2);
    }

    let mut state = ViewerState::new(1024.0, 768.0);
    if let Some(file_path) = args.rest.first() {
        let path = PathBuf::from(file_path);
        // Existence is not the question; openability is, and only trying
        // answers it. A file can disappear between a check and an open, and a
        // file that exists can still be unreadable or in a format this system
        // cannot yet decode.
        //
        // A failure is reported here *and* shown in the window, rather than
        // ending the process. It used to exit(1), which was right when there
        // was no window at all — but a graphical viewer that refuses to appear
        // leaves nothing to act on, and `open_file` rebuilds the directory
        // listing whether or not the file opened, precisely so that the arrow
        // key which reached a broken file can carry the user off it again. The
        // stderr line is what a terminal user gets immediately; the canvas is
        // what everyone else gets. See design-decisions.md §558.
        if !state.open_file(&path) {
            let reason = state
                .load_error
                .as_deref()
                .unwrap_or("the file could not be read");
            eprintln!("imageviewer: {reason}");
        }
        // Auto-fit the first image
        state.fit_to_window();
    }

    oswindow::app::launch_with("imageviewer", args.display.as_deref(), &mut state)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp
    )]

    use super::*;

    /// A file name that is text is shown as it is; one that is not, by its
    /// bytes -- two such names never look the same.
    #[test]
    fn a_file_name_that_is_not_text_is_shown_by_its_bytes() {
        use std::path::Path;
        assert_eq!(
            shown_file_name(Path::new("dir/notes.txt")).as_deref(),
            Some("notes.txt")
        );
        assert_eq!(shown_file_name(Path::new("/")), None);
        #[cfg(windows)]
        let (a, b) = {
            use std::os::windows::ffi::OsStringExt;
            (
                std::ffi::OsString::from_wide(&[0x0066, 0xD800]),
                std::ffi::OsString::from_wide(&[0x0066, 0xD801]),
            )
        };
        #[cfg(not(windows))]
        let (a, b) = {
            use std::os::unix::ffi::OsStringExt;
            (
                std::ffi::OsString::from_vec(vec![b'f', 0xFE]),
                std::ffi::OsString::from_vec(vec![b'f', 0xFF]),
            )
        };
        let shown_a = shown_file_name(Path::new(&a)).unwrap();
        let shown_b = shown_file_name(Path::new(&b)).unwrap();
        assert!(!shown_a.contains('\u{FFFD}'), "{shown_a:?}");
        assert_ne!(shown_a, shown_b, "two names became one");
    }

    /// **Every key the shortcut list advertises is one this program answers.**
    ///
    /// A list on screen and the handler behind it are two copies of one fact,
    /// and they drift: `apps/rssreader` shipped an overlay of twenty-one
    /// shortcuts of which about four worked. The label is read by
    /// `guitk::shortcut` rather than matched against a table written beside it
    /// here -- that table would be a third copy, drifting from both.
    ///
    /// The property is "some reachable state answers this key", not "this key
    /// is taken right now": `Escape` means nothing until there is a full
    /// screen or a slideshow to leave, and declining from its own arm is
    /// answering.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let answered = help_states()
                    .iter_mut()
                    .any(|state| state.handle_key_event(&stroke));
                assert!(
                    answered,
                    "the list advertises {label:?} for {what:?}, and no viewer answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// Viewers chosen so that between them every advertised key has work.
    fn help_states() -> Vec<ViewerState> {
        let plain = ViewerState::new(1024.0, 768.0);

        // Full screen, which is the one state `Escape` has anything to leave.
        let mut full = ViewerState::new(1024.0, 768.0);
        full.fullscreen = true;

        vec![plain, full]
    }

    /// `D` changes how long each slide stays up, and the badge says so.
    ///
    /// `SlideshowInterval` has four options and a `next()` that cycles them,
    /// and `SlideshowState::interval` was `FiveSeconds` at construction with
    /// no writer in the crate. So the module doc's "Slideshow mode with
    /// configurable intervals" described a constant, and `next()` -- written
    /// and tested -- had no caller.
    #[test]
    fn d_changes_how_long_each_slide_stays_up() {
        let mut state = ViewerState::new(1024.0, 768.0);
        state.slideshow.active = true;
        state.current_image = Some(ImageData {
            width: 640,
            height: 480,
            image_id: VIEWER_IMAGE_ID,
        });

        let mut seen = vec![state.slideshow.interval];
        for _ in 0..3 {
            assert!(state.handle_key_event(&plain(Key::D)), "D was ignored");
            seen.push(state.slideshow.interval);
        }
        for want in [
            SlideshowInterval::ThreeSeconds,
            SlideshowInterval::FiveSeconds,
            SlideshowInterval::TenSeconds,
            SlideshowInterval::ThirtySeconds,
        ] {
            assert!(
                seen.contains(&want),
                "cycling never reached {}",
                want.label()
            );
        }
        state.handle_key_event(&plain(Key::D));
        assert_eq!(
            state.slideshow.interval, seen[0],
            "the intervals do not come back round"
        );

        // And the badge names the one in force. Without that, pressing `D`
        // changes when the *next* picture arrives -- three seconds or thirty
        // away -- and nothing happens at the moment of pressing.
        let shown = help_text(&state);
        assert!(
            shown.contains(state.slideshow.interval.label()),
            "the slideshow badge does not name the interval in force: {shown:?}"
        );
    }

    /// The clock restarts when the interval changes.
    #[test]
    fn changing_the_interval_restarts_the_clock() {
        let mut state = ViewerState::new(1024.0, 768.0);
        state.slideshow.active = true;
        state.slideshow.elapsed_ms = 4000;
        state.handle_key_event(&plain(Key::D));
        assert_eq!(
            state.slideshow.elapsed_ms, 0,
            "a shorter interval with the old clock still running can change \
the picture at once, which reads as D advancing the slideshow"
        );
    }

    fn plain(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    /// **The shortcut list reaches the window.**
    ///
    /// The guard above reads the list against the handler; this reads it
    /// against the screen. `apps/netscan`'s `wol_note` was written by the
    /// model and drawn by nothing for three commits with every model-level
    /// test passing.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let mut state = ViewerState::new(1024.0, 768.0);
        assert!(
            !help_text(&state).contains("? closes this"),
            "the list is up before anybody asked for it"
        );

        let mut ask = KeyEvent {
            key: Key::Slash,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        ask.modifiers.shift = true;
        assert!(state.handle_key_event(&ask));

        let shown = help_text(&state);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} never reached the window");
            assert!(shown.contains(what), "{what:?} never reached the window");
        }

        assert!(state.handle_key_event(&KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }));
        assert!(
            !help_text(&state).contains("? closes this"),
            "Escape did not close it"
        );
    }

    /// Every string the window is drawing, joined.
    fn help_text(state: &ViewerState) -> String {
        render(state)
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// Every colour the viewer's chrome draws comes from the user's palette.
    ///
    /// Nothing is declared derived here, which is the point: an image viewer
    /// has no content colour of its own. The picture is the content, and it
    /// arrives as pixels, not as a `RenderCommand`. The scrim over the
    /// slideshow badge is black at an alpha, which the check exempts.
    #[test]
    fn every_colour_the_viewer_draws_comes_from_its_palette() {
        for light in [false, true] {
            let mut state = ViewerState::new(1100.0, 800.0);
            state.palette = Palette::for_mode(light);
            state.show_info_panel = true;
            state.show_thumbnails = true;
            state.slideshow.active = true;
            let tree = render(&state);
            assert!(
                tree.commands.len() > 10,
                "the sweep examined {} commands, which is not a render",
                tree.commands.len()
            );
            appearance::palette_check::assert_drawn_from(
                &state.palette,
                &tree.commands,
                &[],
                &format!("imageviewer (light={light})"),
            );
        }
    }

    use scratchdir::ScratchDir;

    /// One detent is one zoom step, and the direction is the one every other
    /// viewer uses: wheel away from the user zooms in.
    #[test]
    fn one_wheel_notch_is_one_zoom_step() {
        let mut state = ViewerState::new(800.0, 600.0);
        let start = state.transform.zoom;
        state.handle_mouse_event(&MouseEvent {
            x: 100.0,
            y: 100.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: 1.0 },
        });
        assert_eq!(state.transform.zoom, start + ZOOM_STEP);
        state.handle_mouse_event(&MouseEvent {
            x: 100.0,
            y: 100.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
        });
        assert_eq!(state.transform.zoom, start);
    }

    /// The bug the accumulator exists to stop. A trackpad reports a two-finger
    /// flick as a stream of small fractions; reading only the sign of each took
    /// a whole zoom step per event, so one gesture ran the zoom from end to end.
    #[test]
    fn a_trackpad_flick_does_not_zoom_from_end_to_end() {
        let mut state = ViewerState::new(800.0, 600.0);
        let start = state.transform.zoom;
        // Forty events at a twentieth of a notch is two notches of real
        // movement -- and used to be forty steps, i.e. the entire range twice
        // over, pinned at MAX_ZOOM.
        for _ in 0..40 {
            state.handle_mouse_event(&MouseEvent {
                x: 100.0,
                y: 100.0,
                kind: MouseEventKind::Scroll { dx: 0.0, dy: 0.05 },
            });
        }
        assert_eq!(state.transform.zoom, start + 2.0 * ZOOM_STEP);
        assert!(
            state.transform.zoom < MAX_ZOOM,
            "a two-notch gesture must not reach the limit"
        );
    }

    #[test]
    fn test_image_format_detection_bmp() {
        let data = b"BM\x00\x00\x00\x00\x00\x00\x00\x00";
        assert_eq!(ImageFormat::detect(data), ImageFormat::Bmp);
    }

    #[test]
    fn test_image_format_detection_png() {
        let data: &[u8] = &[137, 80, 78, 71, 13, 10, 26, 10, 0, 0];
        assert_eq!(ImageFormat::detect(data), ImageFormat::Png);
    }

    #[test]
    fn test_image_format_detection_jpeg() {
        let data: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0, 0, 0];
        assert_eq!(ImageFormat::detect(data), ImageFormat::Jpeg);
    }

    #[test]
    fn test_image_format_detection_gif87a() {
        let data = b"GIF87a\x00\x00\x00\x00";
        assert_eq!(ImageFormat::detect(data), ImageFormat::Gif);
    }

    #[test]
    fn test_image_format_detection_gif89a() {
        let data = b"GIF89a\x00\x00\x00\x00";
        assert_eq!(ImageFormat::detect(data), ImageFormat::Gif);
    }

    #[test]
    fn test_image_format_detection_unknown() {
        let data = b"RIFF\x00\x00\x00\x00\x00\x00";
        assert_eq!(ImageFormat::detect(data), ImageFormat::Unknown);
    }

    #[test]
    fn test_image_format_detection_too_short() {
        let data = b"BM";
        assert_eq!(ImageFormat::detect(data), ImageFormat::Unknown);
    }

    /// A real 24-bit BMP of `w` by `h.abs()` pixels, top-down when `h` is
    /// negative -- a picture a decoder takes, not a header-shaped stub.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "a fixture's sizes, a few thousand pixels at most"
    )]
    fn bmp_24(w: u32, h: i32) -> Vec<u8> {
        let row = (w * 3).div_ceil(4) * 4;
        let pixels = row * h.unsigned_abs();
        let mut out = Vec::new();
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&(54 + pixels).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&54u32.to_le_bytes());
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&i32::try_from(w).unwrap().to_le_bytes());
        out.extend_from_slice(&h.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&24u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&pixels.to_le_bytes());
        out.extend_from_slice(&2835i32.to_le_bytes());
        out.extend_from_slice(&2835i32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.resize(out.len() + usize::try_from(pixels).unwrap(), 0x80);
        out
    }

    /// A real GIF: a 320x240 screen with one 1x1 frame on it.
    fn gif_320x240() -> Vec<u8> {
        let mut out = b"GIF89a".to_vec();
        out.extend_from_slice(&320u16.to_le_bytes());
        out.extend_from_slice(&240u16.to_le_bytes());
        // A global table of two colours, then the colours.
        out.extend_from_slice(&[0x80, 0, 0, 0, 0, 0, 255, 255, 255]);
        // A frame at 0,0, 1 by 1, with no table of its own.
        out.extend_from_slice(&[0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
        // Its pixels: two-bit LZW codes clear, 0, end, in one sub-block.
        out.extend_from_slice(&[2, 2, 0x44, 0x01, 0]);
        out.push(0x3B);
        out
    }

    /// The size shown before a picture decodes is `imagecodec`'s reading of
    /// its header, for every format. This app read BMP, JPEG and GIF sizes
    /// itself, from whatever bytes sat at the offsets a size would be at.
    #[test]
    fn a_header_is_read_by_the_decoder_that_reads_the_picture() {
        let limits = imagecodec::Limits::default();
        let bmp = bmp_24(2, 3);
        assert_eq!(imagecodec::dimensions(&bmp).ok(), Some((2, 3)));
        assert!(
            imagecodec::decode(&bmp, limits).is_ok(),
            "control: the BMP is a picture"
        );
        // Top-down: the height is stored negative.
        assert_eq!(
            imagecodec::dimensions(&bmp_24(640, -480)).ok(),
            Some((640, 480))
        );
        assert_eq!(
            imagecodec::dimensions(&png_bytes(800, 600)).ok(),
            Some((800, 600))
        );
        let gif = gif_320x240();
        assert_eq!(imagecodec::dimensions(&gif).ok(), Some((320, 240)));
        assert!(
            imagecodec::decode(&gif, limits).is_ok(),
            "control: the GIF is a picture"
        );

        // The stub the old BMP test used -- "BM" and two numbers where a size
        // would be, and no header -- was given a size of 100 by 200.
        let mut stub = vec![0u8; 30];
        stub[..2].copy_from_slice(b"BM");
        stub[18..22].copy_from_slice(&100u32.to_le_bytes());
        stub[22..26].copy_from_slice(&200i32.to_le_bytes());
        assert_eq!(
            imagecodec::dimensions(&stub).ok(),
            None,
            "a file with no header was given a size"
        );
    }

    /// Every format the decoder reads is named, so a broken one is reported
    /// as what it claims to be.
    #[test]
    fn every_format_the_decoder_reads_is_named() {
        assert_eq!(
            ImageFormat::detect(b"RIFF\x1a\x00\x00\x00WEBPVP8L"),
            ImageFormat::WebP
        );
        // A RIFF of another form -- an AVI -- is not a WebP.
        assert_eq!(
            ImageFormat::detect(b"RIFF\x1a\x00\x00\x00AVI LIST"),
            ImageFormat::Unknown
        );
        assert_eq!(
            ImageFormat::detect(&[0, 0, 1, 0, 1, 0, 16, 16]),
            ImageFormat::Ico
        );
        assert_eq!(
            ImageFormat::detect(&[0, 0, 2, 0, 1, 0, 32, 32]),
            ImageFormat::Ico,
            "a cursor is not named, though it decodes"
        );
        for magic in [b"II*\0", b"MM\0*", b"II+\0", b"MM\0+"] {
            let mut data = magic.to_vec();
            data.extend_from_slice(&[8, 0, 0, 0]);
            assert_eq!(ImageFormat::detect(&data), ImageFormat::Tiff, "{magic:?}");
        }
        for format in [ImageFormat::WebP, ImageFormat::Ico, ImageFormat::Tiff] {
            assert_ne!(format.name(), ImageFormat::Unknown.name());
        }
    }

    #[test]
    fn test_rotation() {
        let r = Rotation::None;
        assert_eq!(r.rotate_cw(), Rotation::Cw90);
        assert_eq!(r.rotate_cw().rotate_cw(), Rotation::Cw180);
        assert_eq!(r.rotate_cw().rotate_cw().rotate_cw(), Rotation::Cw270);
        assert_eq!(
            r.rotate_cw().rotate_cw().rotate_cw().rotate_cw(),
            Rotation::None
        );
    }

    #[test]
    fn test_rotation_ccw() {
        let r = Rotation::None;
        assert_eq!(r.rotate_ccw(), Rotation::Cw270);
        assert_eq!(r.rotate_ccw().rotate_ccw(), Rotation::Cw180);
    }

    #[test]
    fn test_rotation_degrees() {
        assert_eq!(Rotation::None.degrees(), 0);
        assert_eq!(Rotation::Cw90.degrees(), 90);
        assert_eq!(Rotation::Cw180.degrees(), 180);
        assert_eq!(Rotation::Cw270.degrees(), 270);
    }

    #[test]
    fn test_transform_zoom_clamp() {
        let mut t = Transform::default();
        // Zoom in repeatedly — should clamp at MAX_ZOOM
        for _ in 0..100 {
            t.zoom_in();
        }
        assert!((t.zoom - MAX_ZOOM).abs() < f32::EPSILON);

        // Zoom out repeatedly — should clamp at MIN_ZOOM
        for _ in 0..100 {
            t.zoom_out();
        }
        assert!((t.zoom - MIN_ZOOM).abs() < f32::EPSILON);
    }

    #[test]
    fn test_transform_reset() {
        let mut t = Transform {
            zoom: 2.5,
            pan_x: 100.0,
            pan_y: -50.0,
            rotation: Rotation::Cw180,
            flip_h: true,
            flip_v: true,
        };
        t.reset();
        assert!((t.zoom - 1.0).abs() < f32::EPSILON);
        assert!((t.pan_x).abs() < f32::EPSILON);
        assert!((t.pan_y).abs() < f32::EPSILON);
        assert_eq!(t.rotation, Rotation::None);
        assert!(!t.flip_h);
        assert!(!t.flip_v);
    }

    #[test]
    fn test_slideshow_interval_cycle() {
        let i = SlideshowInterval::ThreeSeconds;
        assert_eq!(i.next(), SlideshowInterval::FiveSeconds);
        assert_eq!(i.next().next(), SlideshowInterval::TenSeconds);
        assert_eq!(i.next().next().next(), SlideshowInterval::ThirtySeconds);
        assert_eq!(
            i.next().next().next().next(),
            SlideshowInterval::ThreeSeconds
        );
    }

    #[test]
    fn test_slideshow_interval_millis() {
        assert_eq!(SlideshowInterval::ThreeSeconds.millis(), 3000);
        assert_eq!(SlideshowInterval::FiveSeconds.millis(), 5000);
        assert_eq!(SlideshowInterval::TenSeconds.millis(), 10000);
        assert_eq!(SlideshowInterval::ThirtySeconds.millis(), 30000);
    }

    #[test]
    fn test_viewer_state_default() {
        let state = ViewerState::new(1024.0, 768.0);
        assert!((state.window_width - 1024.0).abs() < f32::EPSILON);
        assert!((state.window_height - 768.0).abs() < f32::EPSILON);
        assert!(state.current_image.is_none());
        assert!(!state.fullscreen);
        assert!(!state.slideshow.active);
        assert!(state.show_toolbar);
        assert!(state.show_status_bar);
        assert!(!state.show_info_panel);
    }

    #[test]
    fn test_viewer_fit_zoom_no_image() {
        let state = ViewerState::new(800.0, 600.0);
        assert!((state.fit_zoom() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_is_image_extension() {
        assert!(is_image_extension("png"));
        assert!(is_image_extension("PNG"));
        assert!(is_image_extension("jpg"));
        assert!(is_image_extension("jpeg"));
        assert!(is_image_extension("bmp"));
        assert!(is_image_extension("gif"));
        assert!(!is_image_extension("txt"));
        assert!(!is_image_extension("rs"));
        assert!(!is_image_extension("exe"));
    }

    #[test]
    fn test_image_info_file_size_display() {
        let mut info = ImageInfo {
            file_size: 500,
            ..ImageInfo::default()
        };
        assert_eq!(info.file_size_display(), "500 B");

        info.file_size = 2048;
        assert_eq!(info.file_size_display(), "2.0 KiB");

        info.file_size = 1_500_000;
        assert_eq!(info.file_size_display(), "1.4 MiB");
    }

    #[test]
    fn test_render_produces_commands() {
        let state = ViewerState::new(800.0, 600.0);
        let tree = render(&state);
        // Should have background + toolbar + status bar at minimum
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_image() {
        let mut state = ViewerState::new(800.0, 600.0);
        state.current_image = Some(ImageData {
            width: 200,
            height: 150,
            image_id: VIEWER_IMAGE_ID,
        });
        state.image_info.width = 200;
        state.image_info.height = 150;
        state.image_info.filename = String::from("test.png");

        let tree = render(&state);
        assert!(!tree.is_empty());
        // Should contain an Image command
        let has_image = tree
            .commands
            .iter()
            .any(|cmd| matches!(cmd, RenderCommand::Image { .. }));
        assert!(has_image);
    }

    #[test]
    fn test_handle_tick_advances_slideshow() {
        let mut state = ViewerState::new(800.0, 600.0);
        state.entries = vec![
            DirectoryEntry {
                path: PathBuf::from("/a.png"),
                filename: String::from("a.png"),
                file_size: 100,
            },
            DirectoryEntry {
                path: PathBuf::from("/b.png"),
                filename: String::from("b.png"),
                file_size: 200,
            },
            DirectoryEntry {
                path: PathBuf::from("/c.png"),
                filename: String::from("c.png"),
                file_size: 300,
            },
        ];
        state.current_index = 0;
        state.slideshow.active = true;
        state.slideshow.interval = SlideshowInterval::ThreeSeconds;

        // Not enough time elapsed
        state.handle_tick(2000);
        assert_eq!(state.current_index, 0);

        // Now enough time
        state.handle_tick(1500);
        assert_eq!(state.current_index, 1);
    }

    #[test]
    fn test_handle_tick_paused() {
        let mut state = ViewerState::new(800.0, 600.0);
        state.entries = vec![
            DirectoryEntry {
                path: PathBuf::from("/a.png"),
                filename: String::from("a.png"),
                file_size: 100,
            },
            DirectoryEntry {
                path: PathBuf::from("/b.png"),
                filename: String::from("b.png"),
                file_size: 200,
            },
        ];
        state.current_index = 0;
        state.slideshow.active = true;
        state.slideshow.paused = true;
        state.slideshow.interval = SlideshowInterval::ThreeSeconds;

        state.handle_tick(10000);
        // Should not advance when paused
        assert_eq!(state.current_index, 0);
    }

    // -----------------------------------------------------------------------
    // Getting the picture to the compositor
    // -----------------------------------------------------------------------

    /// Every image request the viewer has queued but not yet sent, as
    /// `("up"|"down", id, width, height, bytes)`.
    fn queued(state: &ViewerState) -> Vec<(&'static str, u64, u32, u32, usize)> {
        state
            .pending_images
            .iter()
            .map(|c| match c {
                oswindow::app::ImageChange::Upload {
                    id,
                    width,
                    height,
                    bytes,
                    ..
                } => ("up", *id, *width, *height, bytes.len()),
                oswindow::app::ImageChange::Drop(id) => ("down", *id, 0, 0, 0),
            })
            .collect()
    }

    /// The defect this whole change exists to close: a viewer that named an
    /// image id and never put pixels under it drew *nothing*, and the id it
    /// named came from a checkerboard it had synthesised itself.
    #[test]
    fn opening_a_picture_queues_its_pixels_for_the_compositor() {
        let guard = scratch("queues-pixels");
        let dir = guard.dir().to_path_buf();
        let file = dir.join("photo.png");
        std::fs::write(&file, png_bytes(9, 7)).expect("write png");

        let mut state = ViewerState::new(800.0, 600.0);
        assert!(state.open_file(&file), "the fixture PNG must decode");

        assert_eq!(
            queued(&state),
            [("up", VIEWER_IMAGE_ID, 9, 7, 9 * 7 * 4)],
            "the pixels must be queued, under the id the render tree names"
        );
        let image = state.current_image.as_ref().expect("a picture");
        assert_eq!(image.image_id, VIEWER_IMAGE_ID);
    }

    /// The bytes are the file's, not a pattern the viewer invented.
    ///
    /// `png_bytes` writes a gradient, so a decoder that silently produced a
    /// flat buffer — or the old checkerboard — fails here. Asserting on the
    /// *content* rather than only the length is the point: the previous code
    /// produced exactly the right number of bytes.
    #[test]
    fn the_pixels_are_the_files_own_and_not_a_pattern() {
        let guard = scratch("real-pixels");
        let dir = guard.dir().to_path_buf();
        let file = dir.join("gradient.png");
        std::fs::write(&file, png_bytes(4, 3)).expect("write png");

        let mut state = ViewerState::new(800.0, 600.0);
        assert!(state.open_file(&file));

        let oswindow::app::ImageChange::Upload { bytes, stride, .. } = &state.pending_images[0]
        else {
            panic!("expected an upload");
        };
        assert_eq!(*stride, 4 * 4, "this decoder never pads");
        // Row 1, pixel 2, as little-endian 0xAARRGGBB: the fixture writes
        // (r, g, b, a) = (x, y, 0x40, 0xFF), so this is (2, 1, 0x40, 0xFF).
        let (x, y, width) = (2usize, 1usize, 4usize);
        let at = (y * width + x) * 4;
        assert_eq!(
            &bytes.as_slice()[at..at + 4],
            &[0x40, 1, 2, 0xFF],
            "b, g, r, a in memory order — the picture was not decoded"
        );
    }

    /// One id for the picture on screen, not one per file.
    ///
    /// A per-path id would leave every photograph the user had paged through
    /// resident in the compositor, since nothing drops them: the connection's
    /// image budget would climb until some later, innocent picture was refused.
    #[test]
    fn paging_through_a_directory_reuses_one_image_id() {
        let guard = scratch("one-id");
        let dir = guard.dir().to_path_buf();
        std::fs::write(dir.join("a.png"), png_bytes(4, 4)).expect("write a");
        std::fs::write(dir.join("b.png"), png_bytes(6, 5)).expect("write b");

        let mut state = ViewerState::new(800.0, 600.0);
        assert!(state.open_file(&dir.join("a.png")));
        state.pending_images.clear();

        state.next_image();
        assert_eq!(
            queued(&state),
            [("up", VIEWER_IMAGE_ID, 6, 5, 6 * 5 * 4)],
            "the second picture must replace the first rather than join it"
        );
    }

    /// Paging faster than the loop draws must not upload every file passed
    /// over: only the last one is ever seen, and a held-down arrow key across a
    /// directory of photographs would otherwise be tens of megabytes of wire
    /// traffic per frame.
    #[test]
    fn skipping_past_pictures_uploads_only_the_one_that_lands() {
        let guard = scratch("supersede");
        let dir = guard.dir().to_path_buf();
        std::fs::write(dir.join("a.png"), png_bytes(4, 4)).expect("write a");
        std::fs::write(dir.join("b.png"), png_bytes(6, 6)).expect("write b");
        std::fs::write(dir.join("c.png"), png_bytes(8, 8)).expect("write c");

        let mut state = ViewerState::new(800.0, 600.0);
        assert!(state.open_file(&dir.join("a.png")));
        state.next_image();
        state.next_image();

        assert_eq!(
            queued(&state),
            [("up", VIEWER_IMAGE_ID, 8, 8, 8 * 8 * 4)],
            "a, b and c were all queued; only c is drawn"
        );
    }

    /// A viewer parked on a file it cannot show must stop paying for the one it
    /// showed before. With no picture, `render_image` emits no `Image` command
    /// at all, so the pixels the compositor still held would be unreachable and
    /// still charged to the connection's image budget.
    #[test]
    fn a_file_that_will_not_open_gives_the_last_picture_back() {
        let guard = scratch("drops-on-failure");
        let dir = guard.dir().to_path_buf();
        let good = dir.join("real.png");
        std::fs::write(&good, png_bytes(8, 8)).expect("write png");

        let mut state = ViewerState::new(800.0, 600.0);
        assert!(state.open_file(&good));
        state.pending_images.clear();

        assert!(!state.open_file(&dir.join("gone.png")));
        assert_eq!(
            queued(&state),
            [("down", VIEWER_IMAGE_ID, 0, 0, 0)],
            "the unreachable picture must be released"
        );
    }

    /// ...but only if there was one. A failure with nothing on screen has
    /// nothing to give back, and a `Drop` sent regardless would be a round trip
    /// per keystroke through a directory of files none of which open.
    #[test]
    fn a_failure_with_nothing_on_screen_asks_for_nothing() {
        let guard = scratch("no-spurious-drop");
        let dir = guard.dir().to_path_buf();

        let mut state = ViewerState::new(800.0, 600.0);
        assert!(!state.open_file(&dir.join("never-existed.png")));
        assert!(queued(&state).is_empty());
    }

    /// A picture larger than the cap is refused, not read whole.
    ///
    /// `imagecodec::Limits` documents that its bounds are checked against a
    /// file's header, "before any buffer the header describes is allocated --
    /// a limit applied afterwards is not a limit, it is a post-mortem". This
    /// viewer used to read the file with `std::fs::read` and consult those
    /// limits afterwards, so a file larger than memory was already in memory
    /// before anything could object.
    ///
    /// The cap is deliberately far below the real one here, because a test
    /// that had to write 256 MiB to prove a bound is a test nobody runs.
    #[test]
    fn a_picture_past_the_cap_is_refused_before_it_is_decoded() {
        let guard = scratch("capped-read");
        let dir = guard.dir().to_path_buf();
        let path = dir.join("huge.png");
        // Bigger than the cap this test uses, and a valid PNG besides, so the
        // refusal cannot be mistaken for "not a picture".
        let picture = imagecodec::testing::png_gradient(8, 8);
        std::fs::write(&path, &picture).expect("write picture");

        let read = safeio::read_capped(&path, 8).expect("the read itself succeeds");
        assert!(
            read.truncated,
            "the cap did not bite, so this test proves nothing"
        );
        assert!(read.bytes.len() <= 8, "and it stopped where it said");
    }

    /// A file that begins as a format does and does not decode says which
    /// format it claims to be, and blames the file.
    ///
    /// It said "GIF images cannot be displayed yet" -- a sentence about this
    /// program, which stopped being true when `imagecodec` learned GIF
    /// (9a61ec67c), after which this test failed and nothing ran it: the
    /// decoder now takes the file for a GIF and says what is wrong with it.
    /// JPEG went the same way before. Every format the viewer names decodes,
    /// so the two cases left are these: a file the decoder takes for the
    /// format and finds broken, and one whose first bytes are all there is of
    /// the format -- which the decoder does not take at all.
    #[test]
    fn a_file_that_only_begins_as_a_picture_says_what_it_claimed_to_be() {
        let guard = scratch("unsupported-format");
        let dir = guard.dir().to_path_buf();
        let gif = dir.join("holiday.gif");
        // A real GIF signature and a logical screen descriptor: enough for
        // `ImageFormat::detect`, and not a GIF.
        std::fs::write(&gif, b"GIF89a\x10\x00\x10\x00\x00\x00\x00").expect("write gif");

        let mut state = ViewerState::new(800.0, 600.0);
        assert!(!state.open_file(&gif));
        let reason = state.load_error.as_deref().expect("a reason");
        assert!(
            reason.contains("GIF"),
            "the reason must name what the file claimed to be: {reason}"
        );
        assert!(
            !reason.contains("cannot be displayed yet"),
            "a format this system reads was called undisplayable: {reason}"
        );

        // A JPEG's start-of-image and nothing after it that a JPEG has: the
        // decoder, which wants a marker next, does not take it for one at all,
        // and the viewer says what it began as.
        let jpeg = dir.join("scan.jpg");
        std::fs::write(&jpeg, [0xFF, 0xD8, 0, 0, 0, 0, 0, 0, 0, 0]).expect("write jpeg");
        assert!(!state.open_file(&jpeg));
        let reason = state.load_error.as_deref().expect("a reason");
        assert!(
            reason.contains("begins as a JPEG file does, but is not one"),
            "the reason must name what the file began as: {reason}"
        );
    }

    /// A real JPEG now opens, where it used to be named as undecodable.
    #[test]
    fn a_jpeg_opens() {
        let guard = scratch("jpeg-opens");
        let dir = guard.dir().to_path_buf();
        let path = dir.join("photo.jpg");
        std::fs::write(&path, imagecodec::testing::SMALL_JPEG).expect("write jpeg");

        let mut state = ViewerState::new(800.0, 600.0);
        assert!(state.open_file(&path), "{:?}", state.load_error);
    }

    /// A truncated PNG used to report a size: `parse_png_dimensions` read
    /// offsets 16 and 20 with no check that an IHDR was there or intact, so the
    /// info panel showed a confident, invented size for a file that would never
    /// open.
    #[test]
    fn a_png_with_no_real_header_reports_no_size() {
        // The signature, then bytes that would read as 0x01020304 by
        // 0x05060708 at the old offsets.
        let mut data = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        data.extend_from_slice(&[0, 0, 0, 13]);
        data.extend_from_slice(b"IHDR");
        data.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);

        assert_eq!(
            imagecodec::dimensions(&data).ok(),
            None,
            "a chunk that ends mid-header must not yield a size"
        );
    }

    /// `B` and `S` hide the toolbar and the status bar.
    ///
    /// Both flags gate a draw and had no writer, so neither bar could be got
    /// out of the way of the picture -- which is the one thing a viewer is
    /// for.
    #[test]
    fn b_and_s_hide_the_two_bars() {
        let mut state = ViewerState::new(800.0, 600.0);
        assert!(state.show_toolbar, "control: the toolbar starts shown");
        assert!(
            state.show_status_bar,
            "control: the status bar starts shown"
        );

        let press = |k: Key| KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };

        state.handle_key_event(&press(Key::B));
        assert!(!state.show_toolbar, "B did not hide the toolbar");
        assert!(state.show_status_bar, "B hid the status bar as well");

        state.handle_key_event(&press(Key::S));
        assert!(!state.show_status_bar, "S did not hide the status bar");

        state.handle_key_event(&press(Key::B));
        assert!(state.show_toolbar, "B does not bring the toolbar back");
    }

    #[test]
    fn test_key_event_zoom_in() {
        let mut state = ViewerState::new(800.0, 600.0);
        let initial_zoom = state.transform.zoom;
        let event = KeyEvent {
            key: Key::Equals,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: String::new(),
        };
        assert!(state.handle_key_event(&event));
        assert!(state.transform.zoom > initial_zoom);
    }

    #[test]
    fn test_key_event_zoom_out() {
        let mut state = ViewerState::new(800.0, 600.0);
        state.transform.zoom = 2.0;
        let event = KeyEvent {
            key: Key::Minus,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: String::new(),
        };
        assert!(state.handle_key_event(&event));
        assert!(state.transform.zoom < 2.0);
    }

    #[test]
    fn test_key_event_not_consumed_on_release() {
        let mut state = ViewerState::new(800.0, 600.0);
        let event = KeyEvent {
            key: Key::Left,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        assert!(!state.handle_key_event(&event));
    }

    #[test]
    fn test_mouse_scroll_zoom() {
        let mut state = ViewerState::new(800.0, 600.0);
        let initial_zoom = state.transform.zoom;
        let event = MouseEvent {
            x: 400.0,
            y: 300.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: 1.0 },
        };
        assert!(state.handle_mouse_event(&event));
        assert!(state.transform.zoom > initial_zoom);
    }

    #[test]
    fn test_fullscreen_toggle() {
        let mut state = ViewerState::new(800.0, 600.0);
        assert!(!state.fullscreen);
        state.execute_action(ViewerAction::ToggleFullscreen);
        assert!(state.fullscreen);
        state.execute_action(ViewerAction::ToggleFullscreen);
        assert!(!state.fullscreen);
    }

    #[test]
    fn test_escape_exits_slideshow_first() {
        let mut state = ViewerState::new(800.0, 600.0);
        state.slideshow.active = true;
        state.fullscreen = true;
        let event = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        // First escape should stop slideshow
        state.handle_key_event(&event);
        assert!(!state.slideshow.active);
        assert!(state.fullscreen);
        // Second escape should exit fullscreen
        state.handle_key_event(&event);
        assert!(!state.fullscreen);
    }

    #[test]
    fn test_navigation_wraps() {
        let mut state = ViewerState::new(800.0, 600.0);
        state.entries = vec![
            DirectoryEntry {
                path: PathBuf::from("/a.png"),
                filename: String::from("a.png"),
                file_size: 100,
            },
            DirectoryEntry {
                path: PathBuf::from("/b.png"),
                filename: String::from("b.png"),
                file_size: 200,
            },
            DirectoryEntry {
                path: PathBuf::from("/c.png"),
                filename: String::from("c.png"),
                file_size: 300,
            },
        ];
        state.current_index = 2;
        state.next_image();
        assert_eq!(state.current_index, 0); // wraps around

        state.current_index = 0;
        state.prev_image();
        assert_eq!(state.current_index, 2); // wraps around
    }

    // ---- A failed load must not leave the previous image on screen ----

    /// A private temporary directory for one test, removed when the returned
    /// guard drops.
    ///
    /// The name used to be a fixed string per test, which two runs of the suite
    /// at once -- or one run under `cargo test` on a machine where a previous
    /// run was killed -- would collide on; the old code papered over that by
    /// deleting the directory on the way in, which silently destroys the other
    /// run's tree. `ScratchDir` names itself from the process id and a
    /// per-process atomic counter, so no two live tests can ever pick the same
    /// name and nothing has to be deleted on the way in.
    ///
    /// Bind the guard to a named local, never to `_`: a bare `_` drops it
    /// immediately and the directory is gone before the test's first line.
    fn press_at(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    fn move_to(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Move,
        })
    }

    fn ctrl(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: String::new(),
        })
    }

    /// What a toolbar button can change, read back.
    fn what_it_did(state: &ViewerState) -> String {
        format!(
            "zoom {} rot {:?} flip {} {} show {} info {} picker {} at {}",
            state.transform.zoom,
            state.transform.rotation,
            state.transform.flip_h,
            state.transform.flip_v,
            state.slideshow.active,
            state.show_info_panel,
            state.picker.is_open(),
            state.current_index,
        )
    }

    /// A viewer on the second of three pictures.
    fn on_the_second_of_three(guard: &ScratchDir) -> ViewerState {
        let dir = guard.dir();
        for (name, w) in [("a.png", 4), ("b.png", 6), ("c.png", 8)] {
            std::fs::write(dir.join(name), png_bytes(w, 4)).expect("write a picture");
        }
        let mut state = ViewerState::new(1024.0, 768.0);
        assert!(state.open_file(&dir.join("b.png")));
        // Neither the fit nor actual size, so both of those buttons show.
        state.transform.zoom = 2.0;
        state
    }

    /// **Every toolbar button does, when clicked, what its action does.**
    ///
    /// They were drawn, with a hover colour and tooltips naming their keys,
    /// and the mouse handler never looked at them: a press over the toolbar
    /// returned "not mine". Each is clicked at its middle and compared with
    /// the action run directly, so a click routed to the wrong button fails
    /// as surely as one routed to none.
    #[test]
    fn every_toolbar_button_answers_a_click() {
        let guard = scratch("toolbar");
        for (index, button) in toolbar_buttons().iter().enumerate() {
            let mut clicked = on_the_second_of_three(&guard);
            let mut run = on_the_second_of_three(&guard);
            let (top, height) = toolbar_button_band(0.0);
            let (x, y) = (button.x + button.width / 2.0, top + height / 2.0);
            assert_eq!(clicked.toolbar_button_at(x, y), Some(index));
            let before = what_it_did(&clicked);
            assert!(clicked.handle_event(&press_at(x, y)), "{}", button.label);
            run.execute_action(button.action);
            assert_eq!(what_it_did(&clicked), what_it_did(&run), "{}", button.label);
            assert_ne!(
                what_it_did(&clicked),
                before,
                "{} changed nothing",
                button.label
            );
        }
    }

    /// With the toolbar hidden, or in full screen, its place is the picture.
    #[test]
    fn a_hidden_toolbar_takes_no_click() {
        let mut state = ViewerState::new(1024.0, 768.0);
        let (top, height) = toolbar_button_band(0.0);
        let first = &toolbar_buttons()[0];
        let (x, y) = (first.x + 2.0, top + height / 2.0);
        assert_eq!(state.toolbar_button_at(x, y), Some(0));
        state.show_toolbar = false;
        assert_eq!(state.toolbar_button_at(x, y), None);
        state.show_toolbar = true;
        state.fullscreen = true;
        assert_eq!(state.toolbar_button_at(x, y), None);
        assert!(!state.handle_event(&press_at(x, y + TOOLBAR_HEIGHT * 20.0)));
    }

    /// The button under the pointer is lit, and only a change redraws.
    #[test]
    fn the_button_under_the_pointer_is_lit() {
        let mut state = ViewerState::new(1024.0, 768.0);
        let (top, height) = toolbar_button_band(0.0);
        let third = &toolbar_buttons()[2];
        let over = move_to(third.x + 1.0, top + height / 2.0);
        assert!(state.handle_event(&over));
        assert_eq!(state.hovered_button, Some(2));
        assert!(
            !state.handle_event(&over),
            "nothing changed; nothing to draw"
        );
        assert!(state.handle_event(&move_to(500.0, 400.0)));
        assert_eq!(state.hovered_button, None);
    }

    /// **A thumbnail opens its picture.** The strip was drawn with the current
    /// picture outlined and took no click.
    #[test]
    fn a_thumbnail_opens_its_picture() {
        let guard = scratch("strip");
        let mut state = on_the_second_of_three(&guard);
        state.show_thumbnails = true;
        let top = thumbnail_top(state.layout().thumbs.expect("the strip is up"));
        let slots = state.thumbnail_slots();
        assert_eq!(slots.len(), 3, "{slots:?}");
        let (entry, left) = slots[2];
        assert!(state.handle_event(&press_at(left + THUMB_SIZE / 2.0, top - 20.0)));
        assert_eq!(
            state.current_index, 1,
            "the thumbnail answered above itself"
        );
        state.dragging = false;
        assert!(state.handle_event(&press_at(left + THUMB_SIZE / 2.0, top + 1.0)));
        assert_eq!(state.current_index, entry);
        assert_eq!(state.image_info.filename, "c.png");
        assert_eq!(state.current_image.as_ref().map(|i| i.width), Some(8));
        assert!(!state.dragging, "a click on the strip is not a pan");
        // Between two thumbnails is nothing.
        let (_, first) = slots[0];
        assert_eq!(
            state.thumbnail_at(first + THUMB_SIZE + 1.0, top + 1.0),
            None
        );
    }

    /// A drag pans only when it starts on the picture: not on the status bar,
    /// the strip or the info panel.
    #[test]
    fn only_a_press_on_the_picture_starts_a_pan() {
        let mut state = ViewerState::new(1024.0, 768.0);
        state.show_info_panel = true;
        // Straight below the Open button: the picture's, not the button's.
        assert!(state.handle_event(&press_at(toolbar_buttons()[0].x + 2.0, 300.0)));
        assert!(state.dragging);
        assert!(!state.picker.is_open(), "the button answered below itself");
        state.dragging = false;
        let panel = state.layout().info.expect("the panel is up");
        assert!(!state.handle_event(&press_at(panel + 10.0, 300.0)));
        let status = state.layout().status.expect("the bar is up");
        assert!(!state.handle_event(&press_at(200.0, status + 5.0)));
        assert!(!state.dragging);
    }

    /// **Ctrl+O puts the Open dialog up, beside the picture on screen**, and
    /// while it is up the keys are its: a `B` typed there does not hide the
    /// toolbar behind it.
    #[test]
    fn ctrl_o_asks_for_a_picture_beside_the_last() {
        let guard = scratch("open-dialog");
        std::fs::write(guard.dir().join("notes.txt"), b"not a picture").expect("write");
        let mut state = on_the_second_of_three(&guard);
        assert!(state.handle_event(&ctrl(Key::O)));
        let dialog = state.picker.dialog().expect("the dialog is up");
        assert_eq!(dialog.current_path(), guard.dir());
        let listed: Vec<_> = dialog.entries().iter().map(|e| e.name.clone()).collect();
        assert_eq!(listed, ["a.png", "b.png", "c.png"], "pictures only");
        assert!(state.handle_event(&Event::Key(plain(Key::B))));
        assert!(
            state.show_toolbar,
            "the key went to the viewer behind the dialog"
        );
        assert!(state.handle_event(&Event::Key(plain(Key::Escape))));
        assert!(!state.picker.is_open());
    }

    /// Choosing a picture in the dialog opens it, and its directory with it.
    #[test]
    fn a_picture_chosen_in_the_dialog_opens() {
        let guard = scratch("open-choose");
        let dir = guard.dir();
        std::fs::write(dir.join("only.png"), png_bytes(5, 3)).expect("write a picture");
        let mut state = ViewerState::new(1024.0, 768.0);
        state.directory = Some(dir.to_path_buf());
        state.execute_action(ViewerAction::Open);
        let dialog = state.picker.dialog_mut().expect("the dialog is up");
        let at = dialog
            .entries()
            .iter()
            .position(|e| e.name == "only.png")
            .expect("the picture is listed");
        dialog.select_entry(at);
        assert!(state.handle_event(&Event::Key(plain(Key::Enter))));
        assert!(!state.picker.is_open());
        assert_eq!(state.image_info.filename, "only.png");
        assert_eq!(
            state.current_image.as_ref().map(|i| (i.width, i.height)),
            Some((5, 3))
        );
        assert_eq!(state.entries.len(), 1, "its directory is listed");
    }

    /// **The parts of the window tile it**: the toolbar, the picture, the
    /// strip and the status bar stack without a gap or an overlap, the info
    /// panel runs beside the picture, and full screen hides the two bars.
    #[test]
    fn the_layout_tiles_the_window() {
        let mut state = ViewerState::new(1024.0, 768.0);
        state.show_info_panel = true;
        state.show_thumbnails = true;
        let bottom = 768.0 - STATUS_BAR_HEIGHT;
        assert_eq!(
            state.layout(),
            Layout {
                toolbar: Some(0.0),
                image: (
                    0.0,
                    TOOLBAR_HEIGHT,
                    1024.0 - INFO_PANEL_WIDTH,
                    bottom - THUMBNAIL_STRIP_HEIGHT - TOOLBAR_HEIGHT
                ),
                info: Some(1024.0 - INFO_PANEL_WIDTH),
                thumbs: Some(bottom - THUMBNAIL_STRIP_HEIGHT),
                status: Some(bottom),
            }
        );
        state.fullscreen = true;
        assert_eq!(
            state.layout(),
            Layout {
                toolbar: None,
                image: (
                    0.0,
                    0.0,
                    1024.0 - INFO_PANEL_WIDTH,
                    768.0 - THUMBNAIL_STRIP_HEIGHT
                ),
                info: Some(1024.0 - INFO_PANEL_WIDTH),
                thumbs: Some(768.0 - THUMBNAIL_STRIP_HEIGHT),
                status: None,
            }
        );
        assert_eq!(state.image_area_height(), 768.0 - THUMBNAIL_STRIP_HEIGHT);
    }

    /// The strip shows the thumbnails around the current picture, as many as
    /// fit whole, with the current one in the middle.
    #[test]
    fn the_strip_is_centred_on_the_current_picture() {
        let mut state = ViewerState::new(1024.0, 768.0);
        state.entries = (0..100)
            .map(|i| DirectoryEntry {
                path: PathBuf::from(format!("{i}.png")),
                filename: format!("{i}.png"),
                file_size: 0,
            })
            .collect();
        state.current_index = 50;
        let slots = state.thumbnail_slots();
        let room = 16; // 1024 / 64
        assert_eq!(slots.len(), room);
        assert_eq!(slots[0], (50 - room / 2, THUMB_PAD));
        assert_eq!(slots[room / 2].0, 50);
        assert_eq!(slots[room - 1].1, 15.0 * THUMB_PITCH + THUMB_PAD);
        state.current_index = 98;
        assert_eq!(state.thumbnail_slots().last().map(|s| s.0), Some(99));
    }

    /// A double click on a toolbar button is two presses of the button, and
    /// not a change of zoom as well.
    #[test]
    fn a_double_click_on_a_button_does_not_zoom() {
        let mut state = ViewerState::new(1024.0, 768.0);
        state.transform.zoom = 2.0;
        let (top, height) = toolbar_button_band(0.0);
        let info = toolbar_buttons()
            .into_iter()
            .find(|b| b.action == ViewerAction::ToggleInfo)
            .expect("an Info button");
        let (x, y) = (info.x + 2.0, top + height / 2.0);
        state.handle_event(&press_at(x, y));
        state.handle_event(&press_at(x, y));
        let double = Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::DoubleClick(MouseButton::Left),
        });
        assert!(!state.handle_event(&double));
        assert_eq!(state.transform.zoom, 2.0);
        assert!(!state.show_info_panel, "pressed twice: on, then off");
        // On the picture, it is still the zoom's.
        let on_picture = Event::Mouse(MouseEvent {
            x: 300.0,
            y: 300.0,
            kind: MouseEventKind::DoubleClick(MouseButton::Left),
        });
        assert!(state.handle_event(&on_picture));
        assert_ne!(state.transform.zoom, 2.0);
    }

    fn scratch(label: &str) -> ScratchDir {
        ScratchDir::new(&format!("slateos-imageviewer-{label}"))
    }

    /// A genuine, decodable 8-bit RGBA PNG of the given size.
    ///
    /// This used to stop after the IHDR fields — enough for
    /// `ImageFormat::detect` to say "PNG" and for the old
    /// `parse_png_dimensions` to read two integers out of offsets 16 and 20.
    /// That was sufficient exactly because nothing decoded: the viewer took the
    /// header's word for the size and drew a checkerboard. A fixture that a
    /// real decoder rejects would now fail every test that opens a file, and
    /// the *right* repair is to make the fixture a real picture rather than to
    /// weaken the tests — a viewer whose whole test suite runs on files no
    /// decoder accepts is the position this crate is being moved out of.
    ///
    /// The encoder that produces it lives in `imagecodec`, next to the decoder
    /// that has to read it, because `apps/explorer` needed the same thing and
    /// a `#[cfg(test)]` helper cannot be shared across a crate boundary even
    /// in principle. The gradient is the one this module always used — red
    /// follows *x*, green follows *y*, blue a constant `0x40`, opaque — so the
    /// pixel assertions below are unchanged by the move.
    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        imagecodec::testing::png_gradient(w, h)
    }

    /// The bug: `display_image` assigned into `image_info` field by field and
    /// left `current_image` alone when the read failed, so the viewer showed
    /// the *new* filename over the *old* picture and the old dimensions.
    #[test]
    fn a_file_that_will_not_open_does_not_keep_the_last_image_on_screen() {
        let guard = scratch("stale-image");
        let dir = guard.dir().to_path_buf();
        let good = dir.join("real.png");
        std::fs::write(&good, png_bytes(640, 480)).expect("write png");

        let mut state = ViewerState::new(800.0, 600.0);
        assert!(state.open_file(&good), "the fixture PNG must load");
        assert!(state.current_image.is_some());
        assert_eq!(state.image_info.width, 640);
        assert_eq!(state.image_info.height, 480);

        let missing = dir.join("gone.png");
        assert!(
            !state.open_file(&missing),
            "a missing file must report failure"
        );

        assert!(
            state.current_image.is_none(),
            "the previous picture must not survive a failed load"
        );
        assert_eq!(
            state.image_info.filename, "gone.png",
            "the UI must name the file the user actually asked for"
        );
        assert_eq!(
            (state.image_info.width, state.image_info.height),
            (0, 0),
            "the old image's dimensions must not be shown beside the new name"
        );
        assert!(
            state.image_info.format.is_none(),
            "the old image's format must not be shown beside the new name"
        );
        assert!(state.load_error.is_some(), "the failure must be explained");
    }

    /// The two empty states must render differently: "you opened nothing" and
    /// "what you opened would not open" used to be the same screen.
    #[test]
    fn a_failed_load_renders_its_reason_rather_than_the_welcome_text() {
        let mut state = ViewerState::new(800.0, 600.0);

        let fresh = render(&state);
        let fresh_text = collect_text(&fresh);
        assert!(fresh_text.iter().any(|t| t.contains("No image loaded")));

        let guard = scratch("render-error");
        let dir = guard.dir().to_path_buf();
        assert!(!state.open_file(&dir.join("nope.png")));

        let failed = render(&state);
        let failed_text = collect_text(&failed);
        assert!(
            failed_text
                .iter()
                .any(|t| t.contains("Cannot display this image")),
            "the failure state must say so: {failed_text:?}"
        );
        assert!(
            !failed_text.iter().any(|t| t.contains("drag an image here")),
            "the failure state must not look like a freshly-started viewer"
        );
        assert!(
            failed_text.iter().any(|t| t.contains("nope.png")),
            "the reason must name the file: {failed_text:?}"
        );
    }

    /// A successful load after a failed one must clear the failure, or the
    /// viewer keeps apologising for a file the user has moved on from.
    #[test]
    fn a_successful_load_clears_the_previous_failure() {
        let guard = scratch("clears-error");
        let dir = guard.dir().to_path_buf();
        let good = dir.join("real.png");
        std::fs::write(&good, png_bytes(32, 16)).expect("write png");

        let mut state = ViewerState::new(800.0, 600.0);
        assert!(!state.open_file(&dir.join("absent.png")));
        assert!(state.load_error.is_some());

        assert!(state.open_file(&good));
        assert!(state.load_error.is_none());
        assert_eq!(state.image_info.filename, "real.png");
        assert_eq!((state.image_info.width, state.image_info.height), (32, 16));
    }

    /// Navigating onto a file whose dimensions cannot be read must show *no*
    /// dimensions, not the previous image's — and must not strand the user
    /// there: the arrow key that reached it has to carry them off it again.
    #[test]
    fn navigation_onto_an_unreadable_file_shows_no_stale_dimensions() {
        let guard = scratch("nav-broken");
        let dir = guard.dir().to_path_buf();
        std::fs::write(dir.join("a.png"), png_bytes(10, 10)).expect("write a");
        std::fs::write(dir.join("b.png"), b"not a png at all").expect("write b");
        std::fs::write(dir.join("c.png"), png_bytes(30, 30)).expect("write c");

        let mut state = ViewerState::new(800.0, 600.0);
        assert!(state.open_file(&dir.join("a.png")));
        assert_eq!(state.entries.len(), 3, "all three are listed");

        state.next_image();
        assert_eq!(state.image_info.filename, "b.png");
        assert_eq!(
            (state.image_info.width, state.image_info.height),
            (0, 0),
            "a file with no readable dimensions must not borrow a.png's"
        );

        state.next_image();
        assert_eq!(
            state.image_info.filename, "c.png",
            "navigation must not stop at the file it could not display"
        );
        assert_eq!((state.image_info.width, state.image_info.height), (30, 30));
    }

    /// Every `Text` command in a render tree, so a test can assert on what the
    /// user would actually read.
    fn collect_text(tree: &RenderTree) -> Vec<String> {
        tree.commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }
}
