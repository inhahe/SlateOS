//! Pictures of a theme: the desktop drawn in it, by the desktop's own code,
//! and composed by the compositor's own, headless.
//!
//! `roadmap-detailed.md` §4.6 asks for two things this is: the theme
//! repository's "Preview rendering: CI job renders standardized screenshots
//! (taskbar, file manager, settings app, terminal, a dialog box) using the
//! actual theme colors and widget styles, so reviewers don't need to install
//! the theme"; and the theme browser's "Large visual previews as the primary
//! selection mechanism" -- which a theme made on this machine, derived or put
//! together from others (`appearance::themes::authoring`), has no screenshot
//! for until something draws one.
//!
//! # One scene, the same for every theme
//!
//! So that pictures of two themes compare, every picture is of the same
//! [`Scene`]: the desktop's wallpaper -- the theme's own picture where it
//! recommends one -- with a column of icons; the taskbar, the start menu
//! open over it; a window of the toolkit's controls in the theme's frame (a
//! heading, a field, check boxes, choices, a slider, a bar, buttons); and a
//! notification. Nothing in it is drawn here: each part is the desktop's own
//! (`desktop::DesktopShell`) or the toolkit's own (`guitk::widget`), placed on
//! its own surface as the desktop session places it -- the background, the
//! window, then the taskbar, the menus and the notifications above, each
//! with the glass the session asks for behind it -- and composed by the
//! compositor (`compositor::Compositor`) with the theme's settings. A picture
//! is therefore what the desktop would show, not an imitation that could
//! drift from it.
//!
//! What is not drawn: the file manager, the Settings app and the terminal
//! the roadmap's list also names, which are programs of lane E's rather than
//! parts of the desktop; and the pointer. Text is drawn in whatever faces the
//! machine drawing it has -- the built-in bitmap face where it has none.

use std::collections::BTreeSet;
use std::path::PathBuf;

use appearance::{AppearanceSettings, ImageFit, Palette};
use compositor::{BufferFormat, Compositor, WindowId};
use desktop::DesktopShell;
use desktop::icons::{IconAction, IconType};
use desktop::notif_pane::{NotifPriority, Notification};
use desktop::session::glass_of;
use desktop::wallpaper::WallpaperManager;
use guiremote::control::{BlurKind, Layer, WindowSpec};
use guitk::canvas::WireBytes;
use guitk::frame::Rect;
use guitk::layout::{FlexAlign, FlexDirection, FlexJustify};
use guitk::render::RenderTree;
use guitk::widget::{Widget, WidgetTree};

/// The size a picture is drawn at unless asked otherwise: a laptop's screen,
/// large enough for every part of the scene to be drawn at its own size.
pub const DEFAULT_SIZE: (u32, u32) = (1280, 800);

/// The smallest picture drawn: below it the start menu and the sample window
/// overlap, and a picture of parts on top of each other shows neither.
pub const MIN_SIZE: (u32, u32) = (1024, 640);

/// The largest: a 4K screen.
pub const MAX_SIZE: (u32, u32) = (3840, 2160);

/// The time of day the scene's wallpaper is drawn at, in seconds after
/// midnight -- a wallpaper may follow the time of day, and a picture drawn
/// twice should show the same one. Ten past ten, a watchmaker's time. The
/// taskbar's clock reads the machine's own.
const SCENE_TIME: u64 = 10 * 3600 + 10 * 60;

/// The process the scene's surfaces are said to belong to: the compositor
/// keeps windows per client, and the scene is one client.
const SCENE_CLIENT: u64 = 1;

/// The sample window's client area.
const WINDOW_SIZE: (u32, u32) = (420, 300);

/// How far the start menu's glow reaches past its box, and the room the
/// window leaves for it.
const MENU_GLOW: f32 = 48.0;

/// How long a notification is given to slide in before the picture is
/// taken, in milliseconds: its whole entrance, well short of its leaving.
const TOAST_SETTLED_MS: u64 = 1_000;

/// A picture: `width` by `height` pixels, row by row, each `0xAARRGGBB`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
    /// Its pixels, row by row from the top, each `0xAARRGGBB`.
    pub argb: Vec<u32>,
}

impl Picture {
    /// The pixel at `(x, y)`, `None` outside the picture.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<u32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = u64::from(y)
            .checked_mul(u64::from(self.width))?
            .checked_add(u64::from(x))?;
        self.argb.get(usize::try_from(at).ok()?).copied()
    }

    /// The picture as a PNG file's bytes.
    ///
    /// # Errors
    ///
    /// [`PreviewError::Png`] when the encoder refuses it.
    pub fn png(&self) -> Result<Vec<u8>, PreviewError> {
        imagecodec::encode_png(self.width, self.height, &self.argb)
            .map_err(|err| PreviewError::Png(format!("{err:?}")))
    }
}

/// Why a picture could not be drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewError {
    /// The size asked for is outside [`MIN_SIZE`] to [`MAX_SIZE`].
    Size(u32, u32),
    /// The compositor refused something the scene asked of it.
    Compositor(String),
    /// The picture could not be encoded.
    Png(String),
}

impl std::fmt::Display for PreviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Size(w, h) => write!(
                f,
                "a picture is drawn from {}x{} to {}x{}, and {w}x{h} is not",
                MIN_SIZE.0, MIN_SIZE.1, MAX_SIZE.0, MAX_SIZE.1
            ),
            Self::Compositor(why) => write!(f, "the compositor refused the scene: {why}"),
            Self::Png(why) => write!(f, "the picture could not be written as a PNG: {why}"),
        }
    }
}

impl std::error::Error for PreviewError {}

/// The parts of the scene, each in screen coordinates, as the desktop and the
/// toolkit draw them in a theme -- before any is composed.
pub struct Scene {
    /// The size it is drawn at.
    pub width: u32,
    /// The size it is drawn at.
    pub height: u32,
    /// The wallpaper and the icons on it.
    pub background: RenderTree,
    /// The wallpaper's picture, decoded, under the id the background draws
    /// it by -- or `None` for a background in a colour.
    pub wallpaper: Option<(u64, imagecodec::Image)>,
    /// The sample window's contents, in its own coordinates, and where its
    /// client area is on the screen.
    pub window: (RenderTree, Rect),
    /// The taskbar, and where it is.
    pub taskbar: (RenderTree, Rect),
    /// The start menu, open, and where its glass is: the box round the
    /// panels it draws, as the session puts it.
    pub menus: (RenderTree, Rect),
    /// The notification, and where the stack of them is.
    pub toasts: Option<(RenderTree, Rect)>,
    /// The shell the parts were drawn by: what each icon id in them names
    /// is its to say.
    shell: DesktopShell,
}

impl Scene {
    /// The scene in `settings`, `width` by `height`.
    ///
    /// # Errors
    ///
    /// [`PreviewError::Size`] for a size outside [`MIN_SIZE`] to
    /// [`MAX_SIZE`].
    pub fn new(
        settings: &AppearanceSettings,
        width: u32,
        height: u32,
    ) -> Result<Self, PreviewError> {
        if width < MIN_SIZE.0 || height < MIN_SIZE.1 || width > MAX_SIZE.0 || height > MAX_SIZE.1 {
            return Err(PreviewError::Size(width, height));
        }
        let mut shell = DesktopShell::new(width, height);
        shell.set_appearance(settings.clone());
        let palette = Palette::from_settings(settings);
        let (w, h) = (to_f32(width), to_f32(height));

        // The wallpaper: the theme's own picture in this mode where it
        // recommends one, as the desktop would show it; else the colour the
        // desktop draws when nothing is chosen.
        let mut wallpaper = WallpaperManager::with_seed(0);
        let light = settings.is_light();
        let mut decoded = None;
        if let Some(path) = settings.wallpaper_theme.picture(light) {
            wallpaper.set_image(path, ImageFit::Fill);
            if let Some(image) = decode_picture(path, width, height) {
                let id = wallpaper.current_image_id();
                wallpaper.picture_ready(id, to_f32(image.width), to_f32(image.height));
                decoded = Some((id, image));
            } else {
                wallpaper.picture_gone();
            }
        } else {
            wallpaper.follow_desktop_base();
        }
        let mut background = RenderTree::new();
        background
            .commands
            .extend(wallpaper.get_render_commands(&palette, w, h, SCENE_TIME));
        add_icons(&mut shell);
        background.commands.extend(shell.render_icons());

        let _toast = shell.notify(Notification {
            id: 0,
            app_name: "Calendar".to_owned(),
            title: "Lunch with Sam".to_owned(),
            body: "In fifteen minutes, at the café on the corner".to_owned(),
            timestamp: 0,
            priority: NotifPriority::Normal,
            read: false,
            action: None,
            silent: false,
        });
        // A notification slides in; the picture is of it arrived.
        shell.advance_toasts(TOAST_SETTLED_MS);
        let toasts = match (shell.toast_extent(), shell.render_toasts()) {
            (Some(rect), Some(tree)) => Some((tree, rect)),
            _ => None,
        };

        shell.toggle_start_menu();
        let menu = shell.render_start_menu().unwrap_or_default();
        // Its glass where the session puts it: behind the panels it draws.
        let glass = glass_of(&menu, shell.screen()).unwrap_or_else(|| shell.start_menu_rect());
        let menus = (menu, glass);
        let taskbar = (shell.render_taskbar(), shell.taskbar_rect());

        // The window: towards the right, clear of the start menu and the
        // glow round it, above the notification -- moved over the glow only
        // where the screen is too narrow for both.
        let menu_right = menus.1.x + menus.1.w + MENU_GLOW;
        let window_w = to_f32(WINDOW_SIZE.0);
        let x = (w - window_w - w * 0.06).max(menu_right).min(w - window_w);
        let at = Rect::new(x, h * 0.1, window_w, to_f32(WINDOW_SIZE.1));
        let window = (sample_window(&palette), at);

        Ok(Self {
            width,
            height,
            background,
            wallpaper: decoded,
            window,
            taskbar,
            menus,
            toasts,
            shell,
        })
    }

    /// The scene composed: each part on a surface of its own, as the desktop
    /// session places it, and the whole composed by the compositor in
    /// `settings`.
    ///
    /// # Errors
    ///
    /// [`PreviewError::Compositor`] when the compositor refuses a surface,
    /// a picture or a frame.
    pub fn compose(&self, settings: &AppearanceSettings) -> Result<Picture, PreviewError> {
        let refused = |err: compositor::CompositorError| PreviewError::Compositor(err.to_string());
        let mut c = Compositor::new(self.width, self.height, 60).map_err(refused)?;
        c.set_appearance(settings.clone());
        let screen = Rect::new(0.0, 0.0, to_f32(self.width), to_f32(self.height));

        // Bottom to top, as the session makes them: within a layer, a
        // surface made later is drawn over one made earlier.
        let background = self.surface(&mut c, "Desktop", screen, Layer::Background, BlurKind::None);
        if let Some((id, image)) = &self.wallpaper {
            let bytes = WireBytes::from_le_argb(&image.pixels);
            c.register_image(
                background,
                *id,
                image.width,
                image.height,
                image.stride(),
                BufferFormat::Argb8888,
                bytes.as_slice(),
            )
            .map_err(refused)?;
        }
        self.submit(&mut c, background, &self.background, screen)?;

        let (contents, at) = &self.window;
        let window = c.create_window_from_spec(
            &WindowSpec {
                decorations: true,
                position: Some((to_i32(at.x), to_i32(at.y))),
                layer: Layer::Normal,
                ..WindowSpec::new("Controls".to_owned(), to_u32(at.w), to_u32(at.h))
            },
            SCENE_CLIENT,
        );
        c.submit_render(window, contents.commands.clone())
            .map_err(refused)?;

        let (bar, bar_at) = &self.taskbar;
        let panel = self.surface(
            &mut c,
            "Taskbar",
            *bar_at,
            Layer::Overlay,
            BlurKind::Taskbar,
        );
        self.submit(&mut c, panel, bar, *bar_at)?;
        // As the session draws its menus: their glass on an empty surface
        // behind the panels they draw (`desktop::session::glass_of`), and the
        // menus -- with the shadows and glows that fall outside their panels
        // -- over it on a clear surface the size of the screen.
        let (menu, menu_at) = &self.menus;
        let _glass = self.surface(
            &mut c,
            "Shell menu glass",
            *menu_at,
            Layer::Overlay,
            BlurKind::Menu,
        );
        let menus = self.surface(
            &mut c,
            "Shell menus",
            screen,
            Layer::Overlay,
            BlurKind::None,
        );
        self.submit(&mut c, menus, menu, screen)?;
        if let Some((tree, at)) = &self.toasts {
            let toasts = self.surface(
                &mut c,
                "Notifications",
                *at,
                Layer::Overlay,
                BlurKind::Notification,
            );
            self.submit(&mut c, toasts, tree, *at)?;
        }

        // Whether anything was drawn is not a question: every part of the
        // scene draws.
        let _ = c.compose_frame();
        Ok(Picture {
            width: self.width,
            height: self.height,
            argb: c.present_pixels().to_vec(),
        })
    }

    /// A surface of the shell's, undecorated, at `at` on `layer`.
    fn surface(
        &self,
        c: &mut Compositor,
        title: &str,
        at: Rect,
        layer: Layer,
        blur_behind: BlurKind,
    ) -> WindowId {
        c.create_window_from_spec(
            &WindowSpec {
                app_id: "slateos-shell".to_owned(),
                position: Some((to_i32(at.x), to_i32(at.y))),
                resizable: false,
                decorations: false,
                transparent: true,
                layer,
                blur_behind,
                ..WindowSpec::new(title.to_owned(), to_u32(at.w), to_u32(at.h))
            },
            SCENE_CLIENT,
        )
    }

    /// Submit the screen-space `tree` to the surface `window` at `at` --
    /// moved into the surface's own coordinates, as the session does, with
    /// every icon it names drawn and handed over first.
    fn submit(
        &self,
        c: &mut Compositor,
        window: WindowId,
        tree: &RenderTree,
        at: Rect,
    ) -> Result<(), PreviewError> {
        let refused = |err: compositor::CompositorError| PreviewError::Compositor(err.to_string());
        let mut sent: BTreeSet<u64> = BTreeSet::new();
        let settings = &self.shell.appearance;
        appearance::icons::upload_missing(
            &tree.commands,
            &settings.icon_theme,
            |id| self.shell.icon_request(id),
            |id| sent.insert(id),
            |id, icon| {
                let bytes = WireBytes::from_le_argb(&icon.argb);
                c.register_image(
                    window,
                    id,
                    icon.size,
                    icon.size,
                    icon.size.saturating_mul(4),
                    BufferFormat::Argb8888,
                    bytes.as_slice(),
                )
                .map_err(refused)
            },
        )?;
        let mut local = RenderTree::new();
        local.translate(-at.x, -at.y);
        local.commands.extend(tree.commands.iter().cloned());
        local.untranslate();
        c.submit_render(window, local.commands).map_err(refused)
    }
}

/// Draw the standard scene in `settings`, `width` by `height`.
///
/// # Errors
///
/// As [`Scene::new`] and [`Scene::compose`].
pub fn render(
    settings: &AppearanceSettings,
    width: u32,
    height: u32,
) -> Result<Picture, PreviewError> {
    Scene::new(settings, width, height)?.compose(settings)
}

/// The scene's icons: a column of the kinds a desktop holds, down its left
/// edge as a fresh desktop places them.
fn add_icons(shell: &mut DesktopShell) {
    let home = PathBuf::from("/home/you");
    for (i, (label, kind, path)) in [
        ("Documents", IconType::Folder, home.join("Documents")),
        ("Pictures", IconType::Folder, home.join("Pictures")),
        (
            "Notes.txt",
            IconType::Document,
            home.join("Desktop/Notes.txt"),
        ),
        (
            "Holiday.png",
            IconType::Image,
            home.join("Desktop/Holiday.png"),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let row = i32::try_from(i).unwrap_or(0);
        shell.icons.add_icon(
            label,
            kind,
            IconAction::OpenPath(path),
            16,
            16_i32.saturating_add(row.saturating_mul(96)),
        );
    }
}

/// The sample window's contents: the toolkit's controls, each in the
/// theme's colours and in the shapes its widget style gives them.
fn sample_window(palette: &Palette) -> RenderTree {
    // A field, a slider and a bar fill the row they are in -- `grow` -- so
    // each is in a row of its own, after a label of a set width, as a
    // program lays them out: in the column itself they would grow downwards.
    let row = |children: Vec<Widget>| {
        Widget::container()
            .with_flex_direction(FlexDirection::Row)
            .with_align(FlexAlign::Center)
            .with_gap(10.0)
            .with_children(children)
    };
    let label = |text: &str| Widget::label(text).css("width: 72px");
    let heading = Widget::label("Display").css("font-weight: bold; font-size: 1.3em");
    let name = row(vec![label("Name"), Widget::text_input("Ocean", "Name")]);
    let seconds = Widget::checkbox("Show seconds on the clock", true);
    let weekday = Widget::checkbox("Show the day of the week", false);
    let modes = row(vec![
        label("Mode"),
        Widget::radio("Light", false),
        Widget::radio("Dark", true),
    ]);
    let size = row(vec![
        label("Text size"),
        Widget::slider(0.0, 100.0, 60.0).labelled("Text size"),
    ]);
    let copying = row(vec![label("Copying"), Widget::progress_bar(40.0, 100.0)]);
    let buttons =
        row(vec![Widget::button("Cancel"), Widget::button("Apply")]).with_justify(FlexJustify::End);
    let root = Widget::container()
        .with_flex_direction(FlexDirection::Column)
        .with_gap(12.0)
        .css("padding: 16px 18px; background-color: var(--base)")
        .with_children(vec![
            heading, name, seconds, weekday, modes, size, copying, buttons,
        ]);
    let mut tree = WidgetTree::new(root, to_f32(WINDOW_SIZE.0), to_f32(WINDOW_SIZE.1));
    tree.set_palette(*palette);
    tree.layout();
    tree.render()
}

/// The picture at `path`, decoded no larger than the screen -- `None` for
/// one that cannot be read or decoded, which leaves the desktop's colour, as
/// the desktop itself would.
fn decode_picture(path: &std::path::Path, width: u32, height: u32) -> Option<imagecodec::Image> {
    let bytes = std::fs::read(path).ok()?;
    imagecodec::decode_scaled(&bytes, imagecodec::Limits::default(), width, height).ok()
}

/// A screen size, as a coordinate: exact for every size a screen has.
#[expect(
    clippy::cast_precision_loss,
    reason = "a size within MAX_SIZE is exact in f32"
)]
fn to_f32(n: u32) -> f32 {
    n as f32
}

/// A coordinate, as a whole pixel: the nearest, held to `i32`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "rounded and clamped to i32's range first"
)]
fn to_i32(v: f32) -> i32 {
    v.round().clamp(-2_147_483_648.0, 2_147_483_520.0) as i32
}

/// A length, as whole pixels: the nearest, at least one.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "rounded and clamped to a size a surface can have first"
)]
fn to_u32(v: f32) -> u32 {
    v.round().clamp(1.0, 65_536.0) as u32
}

#[cfg(test)]
mod tests;
