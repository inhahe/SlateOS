//! Icon themes: the pictures a desktop draws beside a name, found by name in
//! the theme the user chose.
//!
//! # Where an icon comes from
//!
//! A theme's icons are SVG files in an `icons` directory inside the theme's own
//! folder -- beside the `theme.yaml` of [`crate::themes`], under the same two
//! roots, the user's first -- one file per icon, `<name>.svg`. The names are
//! the freedesktop Icon Naming Specification's (`folder`, `user-home`,
//! `utilities-terminal`, `preferences-system`, ...), so an icon set drawn for
//! another desktop can be dropped in, and a program asking for an icon uses a
//! name every theme author already knows.
//!
//! A name the theme lacks is asked for again with its last `-part` dropped --
//! `folder-documents` falls back to `folder` -- which is the specification's
//! own rule, and then the built-in theme's icon of that name. The built-in
//! icons are compiled in, so a desktop with no theme installed, or a theme
//! that draws only a few icons, still draws every one.
//!
//! # Colour
//!
//! An icon drawn in `currentColor` takes the colour it is drawn in -- the text
//! beside it, the accent -- so a monochrome set follows the theme without a
//! file per colour (`roadmap-detailed.md` §3.4 Tier 1, "colour token
//! substitution"). An icon with colours of its own keeps them.
//!
//! # Trust
//!
//! A theme's files are the user's, or whoever installed the theme's: a file
//! larger than [`MAX_ICON_BYTES`], one that is not an SVG this renderer can
//! read, or one that draws nothing is passed over for the next place to look,
//! and an icon is never rendered larger than [`MAX_ICON_PX`] square.
//!
//! # Drawing one in a frame
//!
//! A render tree is commands, not pixels, so a program names an icon in it by
//! an image id: [`IconRegistry::icon`] gives the id of a name drawn at a size
//! in a colour -- the same id every time it is asked the same, under the
//! [`ICON_ID_TAG`] that marks it an icon -- and remembers what it stands for.
//! Before sending a frame, [`upload_missing`] draws and uploads each icon the
//! frame names that its window does not hold yet, since a compositor draws
//! nothing, silently, for an image id it has no pixels for. The desktop draws
//! every one of its pictures this way, and any program can.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use guitk::color::Color;
use guitk::render::RenderCommand;
use guitk::svg::SvgDocument;

use crate::themes::{self, ThemeDirs};

/// The directory inside a theme's folder that holds its icons.
pub const ICONS_DIR: &str = "icons";

/// The largest icon file read. An icon is a few hundred bytes of path data; a
/// file of megabytes is not an icon, and is not parsed to find out.
pub const MAX_ICON_BYTES: u64 = 256 * 1024;

/// The largest size an icon is drawn at, in pixels square.
pub const MAX_ICON_PX: u32 = 512;

/// The built-in theme's icons: `(name, SVG)`. Several names share one picture
/// -- a place is drawn as what it holds, so `folder-documents` is the document.
const BUILT_IN: &[(&str, &str)] = &[
    ("folder", include_str!("../themes/aero/icons/folder.svg")),
    (
        "text-x-generic",
        include_str!("../themes/aero/icons/text-x-generic.svg"),
    ),
    (
        "folder-documents",
        include_str!("../themes/aero/icons/text-x-generic.svg"),
    ),
    (
        "image-x-generic",
        include_str!("../themes/aero/icons/image-x-generic.svg"),
    ),
    (
        "folder-pictures",
        include_str!("../themes/aero/icons/image-x-generic.svg"),
    ),
    (
        "audio-x-generic",
        include_str!("../themes/aero/icons/audio-x-generic.svg"),
    ),
    (
        "folder-music",
        include_str!("../themes/aero/icons/audio-x-generic.svg"),
    ),
    (
        "folder-download",
        include_str!("../themes/aero/icons/folder-download.svg"),
    ),
    (
        "user-home",
        include_str!("../themes/aero/icons/user-home.svg"),
    ),
    (
        "preferences-system",
        include_str!("../themes/aero/icons/preferences-system.svg"),
    ),
    (
        "utilities-terminal",
        include_str!("../themes/aero/icons/utilities-terminal.svg"),
    ),
    (
        "system-shutdown",
        include_str!("../themes/aero/icons/system-shutdown.svg"),
    ),
    (
        "application-x-executable",
        include_str!("../themes/aero/icons/application-x-executable.svg"),
    ),
    (
        "computer",
        include_str!("../themes/aero/icons/computer.svg"),
    ),
    (
        "user-trash",
        include_str!("../themes/aero/icons/user-trash.svg"),
    ),
    (
        "system-search",
        include_str!("../themes/aero/icons/system-search.svg"),
    ),
    (
        "drive-harddisk",
        include_str!("../themes/aero/icons/drive-harddisk.svg"),
    ),
    (
        "emblem-symbolic-link",
        include_str!("../themes/aero/icons/emblem-symbolic-link.svg"),
    ),
    (
        "x-office-document",
        include_str!("../themes/aero/icons/text-x-generic.svg"),
    ),
    // The shell's own pictograms, drawn in place of emoji the fonts do not
    // have (design-decisions.md §881).
    (
        "audio-volume-muted",
        include_str!("../themes/aero/icons/audio-volume-muted.svg"),
    ),
    (
        "audio-volume-low",
        include_str!("../themes/aero/icons/audio-volume-low.svg"),
    ),
    (
        "audio-volume-medium",
        include_str!("../themes/aero/icons/audio-volume-medium.svg"),
    ),
    (
        "audio-volume-high",
        include_str!("../themes/aero/icons/audio-volume-high.svg"),
    ),
    (
        "audio-input-microphone",
        include_str!("../themes/aero/icons/audio-input-microphone.svg"),
    ),
    (
        "audio-input-microphone-muted",
        include_str!("../themes/aero/icons/audio-input-microphone-muted.svg"),
    ),
    (
        "display-brightness-low",
        include_str!("../themes/aero/icons/display-brightness-low.svg"),
    ),
    (
        "display-brightness",
        include_str!("../themes/aero/icons/display-brightness.svg"),
    ),
    (
        "display-brightness-high",
        include_str!("../themes/aero/icons/display-brightness-high.svg"),
    ),
    (
        "media-playback-start",
        include_str!("../themes/aero/icons/media-playback-start.svg"),
    ),
    (
        "media-playback-pause",
        include_str!("../themes/aero/icons/media-playback-pause.svg"),
    ),
    (
        "media-eject",
        include_str!("../themes/aero/icons/media-eject.svg"),
    ),
    (
        "camera-photo",
        include_str!("../themes/aero/icons/camera-photo.svg"),
    ),
    (
        "input-keyboard",
        include_str!("../themes/aero/icons/input-keyboard.svg"),
    ),
    (
        "network-idle",
        include_str!("../themes/aero/icons/network-idle.svg"),
    ),
    (
        "network-offline",
        include_str!("../themes/aero/icons/network-offline.svg"),
    ),
    ("battery", include_str!("../themes/aero/icons/battery.svg")),
    (
        "battery-caution",
        include_str!("../themes/aero/icons/battery-caution.svg"),
    ),
    (
        "battery-charging",
        include_str!("../themes/aero/icons/battery-charging.svg"),
    ),
    (
        "battery-missing",
        include_str!("../themes/aero/icons/battery-missing.svg"),
    ),
    (
        "system-reboot",
        include_str!("../themes/aero/icons/system-reboot.svg"),
    ),
    (
        "system-suspend",
        include_str!("../themes/aero/icons/system-suspend.svg"),
    ),
    (
        "system-suspend-hibernate",
        include_str!("../themes/aero/icons/system-suspend-hibernate.svg"),
    ),
    (
        "system-lock-screen",
        include_str!("../themes/aero/icons/system-lock-screen.svg"),
    ),
    (
        "avatar-default",
        include_str!("../themes/aero/icons/avatar-default.svg"),
    ),
    (
        "preferences-desktop-accessibility",
        include_str!("../themes/aero/icons/preferences-desktop-accessibility.svg"),
    ),
    (
        "view-reveal",
        include_str!("../themes/aero/icons/view-reveal.svg"),
    ),
    (
        "view-conceal",
        include_str!("../themes/aero/icons/view-conceal.svg"),
    ),
    (
        "emblem-ok",
        include_str!("../themes/aero/icons/emblem-ok.svg"),
    ),
    (
        "dialog-warning",
        include_str!("../themes/aero/icons/dialog-warning.svg"),
    ),
    (
        "dialog-error",
        include_str!("../themes/aero/icons/dialog-error.svg"),
    ),
    (
        "notifications",
        include_str!("../themes/aero/icons/notifications.svg"),
    ),
    (
        "notifications-disabled",
        include_str!("../themes/aero/icons/notifications-disabled.svg"),
    ),
    ("alarm", include_str!("../themes/aero/icons/alarm.svg")),
    (
        "action-unavailable",
        include_str!("../themes/aero/icons/action-unavailable.svg"),
    ),
    (
        "start-here",
        include_str!("../themes/aero/icons/start-here.svg"),
    ),
    (
        "pan-start",
        include_str!("../themes/aero/icons/pan-start.svg"),
    ),
    (
        "dialog-information",
        include_str!("../themes/aero/icons/dialog-information.svg"),
    ),
    (
        "drive-removable-media",
        include_str!("../themes/aero/icons/drive-removable-media.svg"),
    ),
    (
        "weather-clear",
        include_str!("../themes/aero/icons/weather-clear.svg"),
    ),
    (
        "preferences-system-time",
        include_str!("../themes/aero/icons/preferences-system-time.svg"),
    ),
    (
        "utilities-system-monitor",
        include_str!("../themes/aero/icons/utilities-system-monitor.svg"),
    ),
    (
        "x-office-calendar",
        include_str!("../themes/aero/icons/x-office-calendar.svg"),
    ),
    (
        "internet-news-reader",
        include_str!("../themes/aero/icons/internet-news-reader.svg"),
    ),
    // A note is written in a text editor, and drawn as the page it is.
    (
        "accessories-text-editor",
        include_str!("../themes/aero/icons/text-x-generic.svg"),
    ),
    // The start menu's folders: whether one is open, and the pictures of the
    // shell's own programs that the set did not already draw.
    ("pan-end", include_str!("../themes/aero/icons/pan-end.svg")),
    (
        "pan-down",
        include_str!("../themes/aero/icons/pan-down.svg"),
    ),
    // The start menu power button's caret: its choices open upwards.
    ("pan-up", include_str!("../themes/aero/icons/pan-up.svg")),
    // The power choices' log out, beside the set's shut down, restart,
    // sleep, hibernate and lock.
    (
        "system-log-out",
        include_str!("../themes/aero/icons/system-log-out.svg"),
    ),
    (
        "accessories-calculator",
        include_str!("../themes/aero/icons/accessories-calculator.svg"),
    ),
    (
        "system-file-manager",
        include_str!("../themes/aero/icons/system-file-manager.svg"),
    ),
    (
        "applets-screenshooter",
        include_str!("../themes/aero/icons/applets-screenshooter.svg"),
    ),
];

/// The names the built-in theme draws, for a caller that lists them.
#[must_use]
pub fn built_in_names() -> Vec<&'static str> {
    BUILT_IN.iter().map(|(name, _)| *name).collect()
}

/// Whether `name` can name an icon: lower-case letters, digits and `-` (and
/// `_`, which some sets use), not empty and not starting with `-`. Anything
/// else -- a `/`, a `.`, a `..` -- would be a path rather than a name.
#[must_use]
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// `name`, then each shorter name the specification's fallback rule gives:
/// `folder-documents` then `folder`.
fn candidates(name: &str) -> impl Iterator<Item = &str> {
    let mut next = Some(name);
    std::iter::from_fn(move || {
        let this = next?;
        next = this
            .rfind('-')
            .and_then(|cut| this.get(..cut))
            .filter(|s| !s.is_empty());
        Some(this)
    })
}

/// An icon, drawn: `size` pixels square, straight-alpha `0xAARRGGBB`, row by
/// row -- the form `imagecodec` decodes to and the compositor uploads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon {
    /// The side of the square, in pixels.
    pub size: u32,
    /// The pixels, `size * size` of them.
    pub argb: Vec<u32>,
}

/// A theme's icons.
///
/// The theme is named by its folder, which need not be text
/// (`design-decisions.md` §426) -- an `OsString`, as a colour theme's is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconTheme {
    id: OsString,
    /// Where to look, or `None` for the standard directories -- read when an
    /// icon is looked up, not when the theme is named, so that the setting
    /// is the name alone and two readings of one file compare equal.
    dirs: Option<ThemeDirs>,
    /// The XDG data directories whose `icons/hicolor` programs install their
    /// own icons into, or `None` for the environment's -- read at lookup, as
    /// `dirs` is.
    data_dirs: Option<Vec<PathBuf>>,
}

impl Default for IconTheme {
    fn default() -> Self {
        Self::built_in()
    }
}

impl IconTheme {
    /// The built-in theme, whose icons are compiled in -- read from the
    /// standard theme directories first, so an installed copy that has been
    /// edited is the one drawn.
    #[must_use]
    pub fn built_in() -> Self {
        Self::load(OsStr::new(themes::BUILT_IN))
    }

    /// The theme `id`, looked for in `dirs`.
    #[must_use]
    pub fn named(id: &OsStr, dirs: ThemeDirs) -> Self {
        Self {
            id: id.to_os_string(),
            dirs: Some(dirs),
            data_dirs: None,
        }
    }

    /// Look for programs' own icons (`hicolor`, `pixmaps`) under these data
    /// directories, in this order, rather than the environment's.
    #[must_use]
    pub fn with_data_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.data_dirs = Some(dirs);
        self
    }

    /// The theme `id`, looked for in the standard theme directories.
    #[must_use]
    pub fn load(id: &OsStr) -> Self {
        Self {
            id: id.to_os_string(),
            dirs: None,
            data_dirs: None,
        }
    }

    /// The theme's name -- its folder's.
    #[must_use]
    pub fn id(&self) -> &OsStr {
        &self.id
    }

    /// The SVG for `name`: the theme's own file, the user's copy before the
    /// system's, trying each shorter name in turn; then the built-in icon of
    /// the name or a shorter one. `None` for a name nothing draws, or one that
    /// is not a name at all.
    #[must_use]
    pub fn source(&self, name: &str) -> Option<Cow<'static, str>> {
        if !is_valid_name(name) {
            return None;
        }
        for candidate in candidates(name) {
            if let Some(text) = self.theme_file(candidate) {
                return Some(Cow::Owned(text));
            }
        }
        candidates(name).find_map(|candidate| {
            BUILT_IN
                .iter()
                .find(|(built, _)| *built == candidate)
                .map(|(_, svg)| Cow::Borrowed(*svg))
        })
    }

    /// The first readable, parseable file for exactly `name` in the theme's
    /// folders, user's first.
    fn theme_file(&self, name: &str) -> Option<String> {
        if !themes::is_valid_id(&self.id) {
            return None;
        }
        let file = format!("{name}.svg");
        let dirs = self.dirs.clone().unwrap_or_else(ThemeDirs::standard);
        dirs.roots().into_iter().find_map(|(root, _)| {
            let path: PathBuf = root.join(&self.id).join(ICONS_DIR).join(&file);
            read_icon(&path)
        })
    }

    /// `name` drawn `size` pixels square, `currentColor` as `color`. `None`
    /// when nothing draws the name, or `size` is zero; a size past
    /// [`MAX_ICON_PX`] is drawn at that.
    ///
    /// Where it is looked for, in order:
    ///
    /// 1. A file, when `name` is an absolute path -- as a program's desktop
    ///    entry may give its icon.
    /// 2. The theme's own icons, with the shorter names: a theme that draws
    ///    only `folder` gives that for every folder, its look kept together.
    /// 3. Name by name, the exact name first: the built-in set, then the icons
    ///    programs install into the `hicolor` theme (and the older `pixmaps`).
    ///    So a program's own icon is drawn under its own name before a
    ///    shorter, generic one is, as the Icon Theme Specification has it.
    ///
    /// SVG or PNG; a PNG is scaled to the size asked, keeping its shape.
    #[must_use]
    pub fn render(&self, name: &str, size: u32, color: Color) -> Option<Icon> {
        let size = size.min(MAX_ICON_PX);
        if size == 0 {
            return None;
        }
        if Path::new(name).is_absolute() {
            return render_file(Path::new(name), size, color);
        }
        if is_valid_name(name) {
            for candidate in candidates(name) {
                if let Some(text) = self.theme_file(candidate) {
                    return render_svg(&text, size, color);
                }
            }
        }
        let name = strip_icon_extension(name);
        if !is_program_icon_name(name) {
            return None;
        }
        for candidate in candidates(name) {
            if let Some((_, svg)) = BUILT_IN.iter().find(|(built, _)| *built == candidate) {
                return render_svg(svg, size, color);
            }
            if let Some(icon) = self.installed(candidate, size, color) {
                return Some(icon);
            }
        }
        None
    }

    /// A program's own icon `name`, as programs install them: in `hicolor`,
    /// the theme every icon theme falls back to -- scalable first, then the
    /// PNG whose size suits best -- and then in `pixmaps`, the older place.
    /// The first data directory that has it wins, the user's before the
    /// system's.
    fn installed(&self, name: &str, size: u32, color: Color) -> Option<Icon> {
        let dirs = match &self.data_dirs {
            Some(dirs) => dirs.clone(),
            None => desktopentry::scan::DataDirs::from_env(|n| std::env::var_os(n))
                .dirs()
                .to_vec(),
        };
        for dir in dirs {
            let hicolor = dir.join("icons").join("hicolor");
            for context in HICOLOR_CONTEXTS {
                let svg = hicolor
                    .join("scalable")
                    .join(context)
                    .join(format!("{name}.svg"));
                if let Some(icon) = read_icon(&svg).and_then(|text| render_svg(&text, size, color))
                {
                    return Some(icon);
                }
                if let Some(icon) = best_png(&hicolor, context, name, size)
                    .and_then(|path| render_raster(&path, size, color))
                {
                    return Some(icon);
                }
            }
            let pixmaps = dir.join("pixmaps");
            if let Some(icon) = render_file(&pixmaps.join(format!("{name}.svg")), size, color)
                .or_else(|| render_file(&pixmaps.join(format!("{name}.png")), size, color))
            {
                return Some(icon);
            }
        }
        None
    }
}

/// The sizes `hicolor` keeps its PNGs at, as `<n>x<n>` directories.
const HICOLOR_SIZES: [u32; 11] = [16, 22, 24, 32, 48, 64, 96, 128, 192, 256, 512];

/// The kinds of icon `hicolor` sorts them into, most likely first: a
/// program's own icon is under `apps`.
const HICOLOR_CONTEXTS: [&str; 8] = [
    "apps",
    "places",
    "mimetypes",
    "devices",
    "categories",
    "status",
    "actions",
    "emblems",
];

/// The largest PNG icon file read: a 512-pixel icon compresses to well under
/// a megabyte, so four is room for any icon and no room for a photograph
/// named by mistake.
pub const MAX_RASTER_ICON_BYTES: u64 = 4 * 1024 * 1024;

/// Whether `name` can name a program's icon: what a desktop entry's `Icon`
/// gives when it is not a path -- `firefox`, `org.gnome.Calculator`,
/// `accessories-text-editor`. Looser than [`is_valid_name`], which is for the
/// icon theme's own names; still never a path out of the directory looked in.
fn is_program_icon_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['.', '-'])
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
}

/// `name` without a `.png`, `.svg` or `.xpm` its entry wrote on the end, which
/// the specification says to leave off and many entries do not.
fn strip_icon_extension(name: &str) -> &str {
    for ext in [".png", ".svg", ".xpm"] {
        if let Some(stem) = name.strip_suffix(ext) {
            return stem;
        }
    }
    name
}

/// The `hicolor` PNG of `name` whose size suits `size` best: the smallest at
/// least as large, so it is only ever scaled down, else the largest there is.
fn best_png(hicolor: &Path, context: &str, name: &str, size: u32) -> Option<PathBuf> {
    let file = format!("{name}.png");
    let found: Vec<(u32, PathBuf)> = HICOLOR_SIZES
        .iter()
        .map(|s| {
            (
                *s,
                hicolor.join(format!("{s}x{s}")).join(context).join(&file),
            )
        })
        .filter(|(_, path)| path.is_file())
        .collect();
    found
        .iter()
        .find(|(s, _)| *s >= size)
        .or_else(|| found.last())
        .map(|(_, path)| path.clone())
}

/// An icon file named by its path: an SVG, or a PNG scaled to `size`.
fn render_file(path: &Path, size: u32, color: Color) -> Option<Icon> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "svg" => read_icon(path).and_then(|text| render_svg(&text, size, color)),
        "png" => render_raster(path, size, color),
        _ => None,
    }
}

/// A PNG icon, decoded and fitted into a `size`-pixel square, faded by
/// `color`'s alpha as an SVG icon is. A raster picture has no `currentColor`,
/// so its own colours are kept.
fn render_raster(path: &Path, size: u32, color: Color) -> Option<Icon> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_RASTER_ICON_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let image = imagecodec::decode(&bytes, imagecodec::Limits::default()).ok()?;
    let argb: Vec<u32> = fit_square(&image, size)
        .into_iter()
        .map(|px| {
            let [a, r, g, b] = px.to_be_bytes();
            let faded = u16::from(a).saturating_mul(u16::from(color.a));
            let alpha = u8::try_from(faded.saturating_add(127) / 255).unwrap_or(u8::MAX);
            u32::from_be_bytes([alpha, r, g, b])
        })
        .collect();
    if argb.iter().all(|px| px & 0xFF00_0000 == 0) {
        return None;
    }
    Some(Icon { size, argb })
}

/// `image` fitted into a `size`-pixel square: scaled to the largest size that
/// fits, keeping its proportions, centred, the rest clear.
///
/// Each pixel drawn is the average of the part of the picture it covers,
/// weighted by how much of each source pixel it covers, in premultiplied
/// alpha -- so a 256-pixel icon drawn at 20 keeps its thin lines as thin grey
/// lines rather than losing them between the samples, and a transparent edge
/// does not bleed its hidden colour into the picture.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "icon sizes are at most MAX_ICON_PX and images are bounded by imagecodec's limits; every float is a pixel coordinate or a colour channel well inside f32's exact range"
)]
fn fit_square(image: &imagecodec::Image, size: u32) -> Vec<u32> {
    let side = size as usize;
    let mut out = vec![0_u32; side.saturating_mul(side)];
    let (w, h) = (image.width as f32, image.height as f32);
    if image.width == 0 || image.height == 0 || side == 0 {
        return out;
    }
    let target = size as f32;
    let scale = (target / w).min(target / h);
    let dw = (w * scale).round().clamp(1.0, target);
    let dh = (h * scale).round().clamp(1.0, target);
    let ox = ((target - dw) / 2.0).floor() as usize;
    let oy = ((target - dh) / 2.0).floor() as usize;
    let (sx, sy) = (w / dw, h / dh);
    let stride = image.width as usize;
    for dy in 0..dh as usize {
        let y0 = dy as f32 * sy;
        let y1 = y0 + sy;
        for dx in 0..dw as usize {
            let x0 = dx as f32 * sx;
            let x1 = x0 + sx;
            let mut sum = [0.0_f32; 4];
            let mut area = 0.0_f32;
            for py in y0.floor() as usize..(y1.ceil() as usize).min(image.height as usize) {
                let wy = (y1.min(py as f32 + 1.0) - y0.max(py as f32)).max(0.0);
                for px in x0.floor() as usize..(x1.ceil() as usize).min(stride) {
                    let wx = (x1.min(px as f32 + 1.0) - x0.max(px as f32)).max(0.0);
                    let weight = wx * wy;
                    let pixel = py
                        .checked_mul(stride)
                        .and_then(|row| row.checked_add(px))
                        .and_then(|at| image.pixels.get(at))
                        .copied()
                        .unwrap_or(0);
                    let [a, r, g, b] = pixel.to_be_bytes();
                    let alpha = f32::from(a) / 255.0;
                    sum[0] += alpha * weight;
                    sum[1] += f32::from(r) * alpha * weight;
                    sum[2] += f32::from(g) * alpha * weight;
                    sum[3] += f32::from(b) * alpha * weight;
                    area += weight;
                }
            }
            if area <= 0.0 || sum[0] <= 0.0 {
                continue;
            }
            let alpha = sum[0] / area;
            let channel = |v: f32| (v / sum[0]).round().clamp(0.0, 255.0) as u8;
            let packed = u32::from_be_bytes([
                (alpha * 255.0).round().clamp(0.0, 255.0) as u8,
                channel(sum[1]),
                channel(sum[2]),
                channel(sum[3]),
            ]);
            if let Some(slot) = oy
                .saturating_add(dy)
                .checked_mul(side)
                .and_then(|row| row.checked_add(ox.saturating_add(dx)))
                .and_then(|at| out.get_mut(at))
            {
                *slot = packed;
            }
        }
    }
    out
}

/// A theme's icon file, if it is one: small enough, text, and an SVG this
/// renderer reads. Anything else is passed over for the next place to look.
fn read_icon(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_ICON_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    SvgDocument::parse(&text).ok().map(|_| text)
}

/// `svg` drawn `size` pixels square, `currentColor` as `color` -- and the
/// whole icon faded by `color`'s alpha, so a translucent colour draws a
/// translucent icon (the ghost of a dragged icon, say) whatever colours the
/// icon's own shapes are in.
pub(crate) fn render_svg(svg: &str, size: u32, color: Color) -> Option<Icon> {
    let hex = format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b);
    let tinted = svg
        .replace("currentColor", &hex)
        .replace("currentcolor", &hex);
    let doc = SvgDocument::parse(&tinted).ok()?;
    let bytes = doc.render(size, size);
    // The renderer's pixels are `[r, g, b, a]`, straight alpha.
    let argb: Vec<u32> = bytes
        .chunks_exact(4)
        .map(|p| match p {
            [r, g, b, a] => {
                // `a * color.a / 255`, rounded: both at most 255, so the
                // product fits and the quotient is a byte again.
                let faded = u16::from(*a).saturating_mul(u16::from(color.a));
                let alpha = u8::try_from(faded.saturating_add(127) / 255).unwrap_or(u8::MAX);
                u32::from_be_bytes([alpha, *r, *g, *b])
            }
            _ => 0,
        })
        .collect();
    let expected = usize::try_from(size)
        .ok()?
        .checked_mul(usize::try_from(size).ok()?)?;
    if argb.len() != expected || argb.iter().all(|px| px & 0xFF00_0000 == 0) {
        // Nothing drawn: an SVG of no visible shapes is not an icon.
        return None;
    }
    Some(Icon { size, argb })
}

// ============================================================================
// Naming icons in a frame
// ============================================================================

/// The bit that marks an image id an icon's: set in every id
/// [`IconRegistry::icon`] gives, so a program's own pictures -- numbered from
/// one upwards -- are never mistaken for one, and [`upload_missing`] leaves
/// them alone.
pub const ICON_ID_TAG: u64 = 1 << 62;
/// The bits of an icon's id below the tag.
const ICON_ID_MASK: u64 = ICON_ID_TAG - 1;

/// An icon a render tree names by its image id: which, how many pixels
/// square, and in what colour -- everything needed to draw and upload it
/// before the frame that names it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IconRequest {
    /// The icon's name, as an icon theme knows it (`folder`, `user-home`).
    ///
    /// Owned or borrowed: the shell's own pictures are names in its source,
    /// but a program's is whatever its desktop entry says.
    pub name: Cow<'static, str>,
    /// Drawn instead when nothing draws `name` -- a program whose entry
    /// names an icon the theme does not have still gets a picture, the
    /// generic one, rather than a gap in its row.
    pub fallback: Option<&'static str>,
    /// Its side, in pixels.
    pub px: u32,
    /// The colour a `currentColor` icon is drawn in; its alpha fades the
    /// whole icon.
    pub color: Color,
}

/// The icons a part of a program has drawn, by the image id each was given.
///
/// Each part that draws icons may keep one -- the desktop keeps one for its
/// menus, one for its desktop icons, one for its overlays -- as long as
/// whatever uploads can ask them all.
#[derive(Debug, Default)]
pub struct IconRegistry {
    requests: RefCell<BTreeMap<u64, IconRequest>>,
}

impl IconRegistry {
    /// The image id of `name` drawn `px` square in `color`, remembering the
    /// request under it.
    ///
    /// The id is the request's hash under [`ICON_ID_TAG`]: the same icon asked
    /// for again, in any frame, is the same id, so it is uploaded once.
    pub fn icon(&self, name: impl Into<Cow<'static, str>>, px: u32, color: Color) -> u64 {
        self.remember(IconRequest {
            name: name.into(),
            fallback: None,
            px,
            color,
        })
    }

    /// [`Self::icon`], drawing `fallback` when nothing draws `name`: for a
    /// name that comes from outside -- a program's desktop entry -- which the
    /// theme may not have.
    pub fn icon_or(
        &self,
        name: impl Into<Cow<'static, str>>,
        fallback: &'static str,
        px: u32,
        color: Color,
    ) -> u64 {
        self.remember(IconRequest {
            name: name.into(),
            fallback: Some(fallback),
            px,
            color,
        })
    }

    /// File `request` under its id, and answer the id.
    fn remember(&self, request: IconRequest) -> u64 {
        let mut hasher = std::hash::DefaultHasher::new();
        request.hash(&mut hasher);
        let id = ICON_ID_TAG | (hasher.finish() & ICON_ID_MASK);
        self.requests.borrow_mut().insert(id, request);
        id
    }

    /// What was drawn under `id`, if anything was.
    #[must_use]
    pub fn request(&self, id: u64) -> Option<IconRequest> {
        self.requests.borrow().get(&id).cloned()
    }

    /// Forget every request: the appearance changed, and every icon is drawn
    /// again in new colours under new ids.
    pub fn clear(&self) {
        self.requests.borrow_mut().clear();
    }
}

/// Upload each icon `commands` name that has not been sent yet: draw it from
/// `theme` as `lookup` says it was asked for, and hand its pixels to `upload`.
///
/// `first_time(id)` answers whether `id` still needs sending -- and records
/// that it has been, which is the caller's to keep per window, since each
/// window holds its own images. An id that is not an icon's (no
/// [`ICON_ID_TAG`]) is a picture the program uploads itself and is left alone.
/// An icon `lookup` does not know, or one nothing draws, is recorded as sent
/// all the same: the frame still names it and draws the rest, and asking
/// again every frame would find nothing again every frame. So is one `upload`
/// refuses -- that is the caller's to decide, by what it returns: an error
/// stops here and is handed back.
///
/// # Errors
///
/// Whatever `upload` returns.
pub fn upload_missing<E>(
    commands: &[RenderCommand],
    theme: &IconTheme,
    lookup: impl Fn(u64) -> Option<IconRequest>,
    mut first_time: impl FnMut(u64) -> bool,
    mut upload: impl FnMut(u64, &Icon) -> Result<(), E>,
) -> Result<(), E> {
    for command in commands {
        let RenderCommand::Image { image_id, .. } = command else {
            continue;
        };
        let id = *image_id;
        if id & ICON_ID_TAG == 0 || !first_time(id) {
            continue;
        }
        let Some(request) = lookup(id) else {
            continue;
        };
        let icon = theme
            .render(&request.name, request.px, request.color)
            .or_else(|| {
                request
                    .fallback
                    .and_then(|fallback| theme.render(fallback, request.px, request.color))
            });
        if let Some(icon) = icon {
            upload(id, &icon)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "icons_tests.rs"]
mod tests;
