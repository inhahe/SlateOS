//! The shell's event loop — the thing that was missing.
//!
//! Everything else in this crate produces values: [`DesktopShell`] answers
//! "what is under this point", "what should the taskbar look like", "what does
//! this click mean". Nothing carried those values anywhere. The render methods
//! had one caller each — the crate's own demo — and a taskbar click produced a
//! [`ShellAction`] that was printed rather than sent. See `known-issues.md`
//! `TD-C-THE-SHELL-CAN-DRAW-ITSELF-AND-NOBODY-CAN-ASK-IT-TO`.
//!
//! [`ShellSession`] is the carrier. It owns an [`EventLoop`] and a
//! [`DesktopShell`], creates the surfaces the shell draws on, feeds the
//! compositor's window list into [`DesktopShell::apply_window_list`], turns
//! pointer and key events into shell calls, and sends the resulting intents
//! back out as protocol requests.
//!
//! # The four surfaces, and why four
//!
//! A shell is not one window. Its parts sit in different bands of the stacking
//! order, and the band is not advisory — a taskbar demoted to [`Layer::Normal`]
//! disappears behind the first window the user opens.
//!
//! | Surface | Layer | Covers | Draws |
//! |---|---|---|---|
//! | background | [`Layer::Background`] | the whole screen | the wallpaper |
//! | panel | [`Layer::Overlay`] | [`DesktopShell::taskbar_rect`] | the taskbar |
//! | popups | [`Layer::Overlay`] | the whole screen | start menu, power menu, calendar, Alt-Tab |
//! | osd | [`Layer::Overlay`], click-through | the whole screen | the volume and brightness overlays |
//!
//! The popup surface is full-screen rather than menu-sized, and that is the
//! whole mechanism behind click-outside-to-dismiss: a press on bare desktop
//! while a menu is open has to reach the shell in order to close the menu, and
//! it can only do that if some surface of the shell's is under it. It is
//! unmapped while nothing is open, so an idle desktop is not covered by an
//! invisible sheet that eats every click.
//!
//! The overlay surface is full-screen for a quite different reason — an OSD is
//! centred on the display and is *not* there to be clicked — and it is the one
//! surface created `input_transparent`, so a press lands on whatever is
//! underneath instead of on a volume indicator that happens to be fading over
//! it. That is also why it cannot share the popup surface: those two want
//! opposite answers to the same question, and they come and go on unrelated
//! schedules, so one surface would have to be mapped whenever either wanted it.
//!
//! # Two coordinate spaces, one origin
//!
//! [`DesktopShell`] hit-tests and draws in **screen** coordinates:
//! [`taskbar_rect`](DesktopShell::taskbar_rect) puts the bar at
//! `screen_height - thickness`, and [`hit_test`](DesktopShell::hit_test)
//! expects a point in the same space. A compositor client is never told screen
//! coordinates — a press is delivered relative to the window it landed on, and
//! a submitted picture is painted relative to the window's own top-left corner.
//!
//! So every surface needs a translation, in **both** directions, and they are
//! opposites:
//!
//! - input: `screen = local + origin`, before [`hit_test`](DesktopShell::hit_test)
//! - output: `local = screen - origin`, before [`EventLoop::submit`]
//!
//! Getting one right and the other wrong yields a desktop that draws correctly
//! and responds in the wrong place — a taskbar whose buttons work forty pixels
//! above themselves. [`Surface`] is the one place either direction is written,
//! and `a_point_that_hits_an_element_is_a_point_that_element_was_drawn_at`
//! below is the test that ties them together.
//!
//! Not done by re-basing the shell's rectangles per surface, which would be the
//! obvious alternative: [`hit_test`](DesktopShell::hit_test) compares the popup
//! rectangles against the taskbar rectangle to decide which of two overlapping
//! things a point belongs to, and that comparison is only meaningful while they
//! are all in one space.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use appearance::Palette;
use guitk::event::{Event, MouseEvent};
use guitk::render::RenderTree;
use oswindow::{
    ConnectionError, ConnectionTransport as Transport, Error, EventLoop, Layer, PixelFormat, Spec,
};

use crate::animations::{AnimationManager, WindowAnimation};
use crate::notif_pane;
use crate::wallpaper::WallpaperManager;
use crate::{DesktopShell, ShellAction, ShellRequest, WindowRequest};

/// How long the shell asks to be woken for the next animation frame.
///
/// A target, not a promise: the wake-up is a *deadline*, so a loaded machine
/// delivers the frame late and the animation takes a correspondingly bigger
/// step. Nothing in the shell counts frames, so a late one costs smoothness and
/// never correctness.
///
/// 16 ms rather than 7 (144 Hz) because the shell is not composited in step with
/// the display: a client-side clock cannot align with vsync however fast it
/// runs, so a faster one buys extra wake-ups and no extra smoothness. The
/// vsync-locked version is a compositor frame callback and stays open — see
/// `design-decisions.md` §521 §1.
const FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// One window the shell draws on, and where it sits on screen.
///
/// Deliberately not a `Window`: [`oswindow::Window`] is what the compositor
/// last said about a window, and this is what the *shell* needs to know about
/// one, which is only its id and its origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    window: u64,
    origin: (f32, f32),
}

impl Surface {
    /// The compositor's id for this surface.
    #[must_use]
    pub const fn window(&self) -> u64 {
        self.window
    }

    /// Where the surface's top-left corner is, in screen coordinates.
    #[must_use]
    pub const fn origin(&self) -> (f32, f32) {
        self.origin
    }

    /// A pointer event as the shell understands it: window-local in, screen
    /// out.
    #[must_use]
    pub fn to_screen(&self, event: &MouseEvent) -> MouseEvent {
        MouseEvent {
            x: event.x + self.origin.0,
            y: event.y + self.origin.1,
            kind: event.kind.clone(),
        }
    }

    /// A screen-space picture as this surface must submit it: screen in,
    /// window-local out.
    ///
    /// Done with a `PushTranslate`/`PopTranslate` pair rather than by rewriting
    /// the coordinate in every command. Not merely because it is shorter:
    /// [`RenderCommand`](guitk::render::RenderCommand) has a dozen variants
    /// carrying positions in different shapes — points, rectangles, glyph runs,
    /// path data — and a rewriter would have to be extended for each new one,
    /// silently drawing at the wrong place until somebody noticed. The
    /// transform stack is a single fact the renderer already applies to
    /// everything inside it.
    ///
    /// Applied unconditionally, even for an origin of `(0, 0)` where it is a
    /// no-op. A "skip it when it does not matter" branch would mean the surface
    /// at the origin — the wallpaper, and the popups — went down a code path
    /// the translated one never took, which is how the two directions get to
    /// disagree in the first place.
    #[must_use]
    pub fn localize(&self, tree: &RenderTree) -> RenderTree {
        let mut out = RenderTree::new();
        out.translate(-self.origin.0, -self.origin.1);
        out.commands.extend(tree.commands.iter().cloned());
        out.untranslate();
        out
    }
}

/// A running desktop shell: the compositor on one side, [`DesktopShell`] on the
/// other.
///
/// ```no_run
/// use desktop::session::ShellSession;
/// use oswindow::EventLoop;
///
/// let events = EventLoop::new(oswindow::connect().expect("no compositor"));
/// let mut session = ShellSession::start(events).expect("the compositor refused a surface");
/// session.shell_mut().load_appearance();
/// session.repaint().expect("could not paint");
/// session.run().expect("the connection failed");
/// ```
pub struct ShellSession<T: Transport> {
    events: EventLoop<T>,
    shell: DesktopShell,
    wallpaper: WallpaperManager,
    background: Surface,
    panel: Surface,
    popups: Surface,
    /// Whether the popup surface is currently mapped. Tracked so that
    /// `set_visible` is a round trip only when the answer changes, rather than
    /// on every repaint.
    popups_shown: bool,
    /// The full-screen, click-through surface the heads-up overlays are drawn
    /// on.
    ///
    /// Separate from `popups` rather than sharing it, because the two differ in
    /// the one property neither can compromise on: a menu exists to be clicked
    /// and an overlay must never be. They also come and go on unrelated
    /// schedules — an OSD can appear over an open start menu — so one surface
    /// would have to be mapped whenever *either* wanted it, which would leave
    /// the menu's full-screen click-catcher up during a volume fade and swallow
    /// the next click on bare desktop.
    osd: Surface,
    /// Whether the overlay surface is currently mapped, tracked as
    /// `popups_shown` is and reconciled from `shell.osd.has_visible()`.
    osd_shown: bool,
    /// Whether the shell currently holds the Escape key.
    ///
    /// Tracked for the same reason `popups_shown` is, and reconciled by
    /// [`reconcile_escape_grab`](Self::reconcile_escape_grab) — see there for
    /// why Escape alone is held conditionally.
    escape_held: bool,
    /// The last window-list revision folded into `shell`.
    revision: u64,
    /// Whether the chrome needs repainting before the next block.
    dirty: bool,
    running: bool,
    launches: Vec<PathBuf>,
    /// Everything currently moving. Empty means no wake-up is registered and
    /// the loop parks with no bound at all, which is what keeps an idle desktop
    /// idle.
    animations: AnimationManager,
    /// The picture the background surface was last *asked* to hold, as the
    /// wallpaper's image id and the path it was read from.
    ///
    /// Both halves are needed. The id alone would not notice a user who edited
    /// the file in place and asked for the same wallpaper again; the path alone
    /// would not notice that [`WallpaperManager`] has issued a fresh id, which
    /// it does on every `set_image` and every slideshow step, and an upload
    /// under the *old* id would leave the new one naming nothing.
    ///
    /// "Asked to hold" rather than "holds", because a failed attempt is
    /// recorded here too — paired with a `wallpaper_error` that says so. That
    /// is deliberate: `paint_background` runs on every repaint, so a pair that
    /// was *not* remembered on failure would re-read and re-inflate a corrupt
    /// full-screen `.png` on every mouse click.
    wallpaper_image: Option<(u64, String)>,
    /// Why the wallpaper file could not be shown, if it could not.
    ///
    /// Kept rather than returned, because failing to show a wallpaper is not a
    /// reason to fail a repaint: the background mode still paints its colour
    /// underlay, and a shell that refused to draw its taskbar because a `.png`
    /// was corrupt would be a worse outcome than a plain background. See
    /// [`wallpaper_error`](Self::wallpaper_error).
    wallpaper_error: Option<String>,
}

impl<T: Transport> ShellSession<T> {
    /// Take over a connection: ask how big the display is, create the three
    /// surfaces, subscribe to the window list, and paint once.
    ///
    /// Does **not** read the user's appearance settings from disk. A session
    /// has to be constructible where there is no filesystem — a test, a
    /// recovery shell — and a constructor that quietly opened a file would make
    /// every such caller depend on the contents of the developer's home
    /// directory. The caller does it, and repaints:
    /// `session.shell_mut().load_appearance()` then
    /// [`repaint`](Self::repaint).
    ///
    /// # Errors
    ///
    /// As [`EventLoop::display_info`], [`EventLoop::create`] and
    /// [`EventLoop::watch_desktop`]. A refused surface is fatal here rather
    /// than survivable: a shell with no taskbar is not a degraded shell, it is
    /// a desktop the user cannot switch windows from.
    pub fn start(mut events: EventLoop<T>) -> Result<Self, Error<T>> {
        let display = events.display_info()?;
        let shell = DesktopShell::new(display.width, display.height);
        let bar = shell.taskbar_rect();

        // Order matters: the panel is created before the popup surface, so
        // within `Layer::Overlay` the menus stack above the bar they rise from.
        let background = Surface {
            window: events.create(chrome(
                "Desktop",
                display.width,
                display.height,
                (0, 0),
                Layer::Background,
            ))?,
            origin: (0.0, 0.0),
        };
        let panel = Surface {
            window: events.create(chrome(
                "Taskbar",
                px(bar.w),
                px(bar.h),
                (pos(bar.x), pos(bar.y)),
                Layer::Overlay,
            ))?,
            origin: (bar.x, bar.y),
        };
        let popups = Surface {
            window: events.create(chrome(
                "Shell menus",
                display.width,
                display.height,
                (0, 0),
                Layer::Overlay,
            ))?,
            origin: (0.0, 0.0),
        };
        // Last, so it is above the menus: an overlay is a heads-up report and
        // has to be readable over whatever is on screen, including a start menu
        // the user opened while the volume was still fading.
        let osd = Surface {
            window: events.create(Spec {
                // The one surface that declines the mouse. Everything else the
                // shell owns is here to be clicked; this is here to be read,
                // and a press aimed at the document under a volume indicator
                // must reach the document. See `design-decisions.md` 566.
                input_transparent: true,
                ..chrome(
                    "Shell overlays",
                    display.width,
                    display.height,
                    (0, 0),
                    Layer::Overlay,
                )
            })?,
            origin: (0.0, 0.0),
        };

        events.watch_desktop(true)?;

        // The shortcuts, claimed on the panel — the one surface that is mapped
        // for the whole session. The popup surface is unmapped whenever no menu
        // is open, which is most of the time, and a grab held by a window the
        // user cannot see is easier to reason about when that window is at least
        // always there.
        //
        // Without this every shortcut below is dead the moment the user clicks
        // into an application: the compositor routes a keystroke to whoever has
        // the keyboard, and that is not the shell. Alt+Tab could not work at
        // all — it exists to be pressed from inside another window.
        //
        // A refusal is fatal, on the same argument the surfaces above are: a
        // desktop whose window switcher does not respond is not a degraded
        // desktop. The one refusal that is *not* a bug — another shell already
        // holds the chord — is a refusal to run two shells at once, which is
        // correct.
        // Asked of the shell rather than of a constant: the set is derived from
        // whatever is in the shell's hotkey registry, so a rebound shortcut is
        // grabbed under its new chord without anyone having to remember to edit
        // a second list.
        for (key, modifiers) in shell.global_chords() {
            events.grab_key(panel.window, key, modifiers)?;
        }

        let mut session = Self {
            events,
            shell,
            wallpaper: WallpaperManager::new(),
            background,
            panel,
            popups,
            // The compositor maps a new window; nothing is open yet, so the
            // first `paint_chrome` unmaps it. Recording `true` here rather than
            // `false` is what makes that first unmap actually happen.
            popups_shown: true,
            osd,
            // For the same reason as `popups_shown`: nothing is showing yet, so
            // the first `paint_chrome` has to unmap a surface the compositor
            // just mapped.
            osd_shown: true,
            // Nothing is open on a fresh desktop, and nothing was grabbed above.
            escape_held: false,
            revision: 0,
            dirty: false,
            running: false,
            launches: Vec::new(),
            animations: AnimationManager::new(),
            wallpaper_image: None,
            wallpaper_error: None,
        };
        session.repaint()?;
        Ok(session)
    }

    /// The shell being driven.
    #[must_use]
    pub const fn shell(&self) -> &DesktopShell {
        &self.shell
    }

    /// The shell being driven, to configure or to drive directly.
    ///
    /// Anything changed through here needs a [`repaint`](Self::repaint): this
    /// type learns that the picture is stale from the events it handles, and an
    /// outside caller reaching past it is not one of those.
    pub const fn shell_mut(&mut self) -> &mut DesktopShell {
        &mut self.shell
    }

    /// The desktop background. Changing it needs a
    /// [`paint_background`](Self::paint_background).
    pub const fn wallpaper_mut(&mut self) -> &mut WallpaperManager {
        &mut self.wallpaper
    }

    /// The full-screen surface behind every window.
    #[must_use]
    pub const fn background(&self) -> Surface {
        self.background
    }

    /// The taskbar's own surface.
    #[must_use]
    pub const fn panel(&self) -> Surface {
        self.panel
    }

    /// The full-screen surface the menus are drawn on, mapped only while one is
    /// open.
    #[must_use]
    pub const fn popups(&self) -> Surface {
        self.popups
    }

    /// The full-screen click-through surface the heads-up overlays are drawn
    /// on, mapped only while one is showing.
    #[must_use]
    pub const fn osd(&self) -> Surface {
        self.osd
    }

    /// The programs the user has asked to start since this was last called.
    ///
    /// The one intent this loop cannot carry out itself. A shell has no channel
    /// to the process server, and inventing one here would put policy about
    /// *how* a program starts — namespace, capabilities, environment — inside
    /// the window manager. So the path comes out here for whoever does own
    /// process creation. See `known-issues.md`
    /// `TD-SHELL-HAS-NOWHERE-TO-SEND-A-LAUNCH`.
    ///
    /// A [`PathBuf`] and not a `String`, because the name of a program is a
    /// filesystem path and our paths are byte strings — a browsed executable
    /// whose name has no UTF-8 spelling must reach the process server as the
    /// bytes that name it, not as a lossy rendering that names nothing.
    pub fn take_launches(&mut self) -> Vec<PathBuf> {
        core::mem::take(&mut self.launches)
    }

    /// Paint everything: background and chrome.
    ///
    /// # Errors
    ///
    /// As [`EventLoop::submit`].
    pub fn repaint(&mut self) -> Result<(), Error<T>> {
        self.paint_background()?;
        self.paint_chrome()
    }

    /// Paint the wallpaper.
    ///
    /// Separate from the chrome because it is the one surface whose picture
    /// does not depend on anything an input event changes; repainting it on
    /// every click would re-encode a full-screen image to say nothing new.
    ///
    /// # Errors
    ///
    /// As [`EventLoop::submit`].
    pub fn paint_background(&mut self) -> Result<(), Error<T>> {
        // Before the picture, the pixels the picture refers to. `render_image`
        // emits an `Image` command naming `current_image_id`, and the
        // compositor draws *nothing, silently* for an id it has never been
        // given bytes for — so an upload that has not happened is a wallpaper
        // that does not appear and says nothing about why.
        self.refresh_wallpaper_image()?;
        let width = self.shell.screen_width as f32;
        let height = self.shell.screen_height as f32;
        // The same zone the taskbar clock reads in — see
        // `DesktopShell::seconds_since_local_midnight` for why this is asked of
        // the shell rather than computed here.
        let day = self.shell.seconds_since_local_midnight(unix_now());
        // Built here rather than held, for the same reason the chrome builds
        // one per paint: the shell's `appearance` is the single source of
        // truth, and a cached palette is a second one that goes stale the
        // moment the user switches mode.
        let p = Palette::from_settings(&self.shell.appearance);
        let mut tree = RenderTree::new();
        tree.commands
            .extend(self.wallpaper.get_render_commands(&p, width, height, day));
        self.events
            .submit(self.background.window, &self.background.localize(&tree))
    }

    /// Why the wallpaper is not on screen, if it is not.
    ///
    /// `None` covers both "the picture is up" and "no picture was asked for" —
    /// a solid-colour or gradient background is not a failure, and a caller
    /// that wanted to distinguish the two would be asking the wrong object:
    /// [`WallpaperManager::mode`] is what says whether an image was wanted.
    ///
    /// The string names the path, because the interesting failures are all
    /// about *which* file: a slideshow directory with one truncated frame in it
    /// reports that frame, and a wallpaper carried over from another machine
    /// reports the path that no longer exists.
    pub fn wallpaper_error(&self) -> Option<&str> {
        self.wallpaper_error.as_deref()
    }

    /// Make sure the compositor holds the pixels that the background surface's
    /// `Image` command is about to name.
    ///
    /// [`WallpaperManager`] performs no I/O by design — that is what keeps its
    /// tests runnable with no filesystem — so it allocates an image id and
    /// emits a command naming it, and something else has to put bytes under
    /// that id. This is that something else, and it lives here rather than in
    /// the manager because this is the layer that owns the connection.
    ///
    /// Called on every `paint_background` and almost always does nothing: the
    /// `(id, path)` pair it remembers is unchanged, so there is no read, no
    /// decode and no upload. It does work exactly when the wallpaper actually
    /// changed, which is what the id was allocated to signal.
    /// Record why the wallpaper could not be shown — and tell the user.
    ///
    /// The single writer of `wallpaper_error`. It exists because the field had
    /// four assignments and no reader outside a getter nobody called: a
    /// wallpaper that failed to decode left the user looking at a plain colour
    /// with no way at all to find out why. Routing every assignment through
    /// here means a failure the shell *noticed* is a failure the shell *says*,
    /// and the two cannot drift apart by someone adding a fifth assignment that
    /// forgets to post.
    ///
    /// Posting is conditional on the message being new, not on it being
    /// `Some`. `paint_background` runs on every repaint, so an unconditional
    /// post would put one notification per mouse click into the history for as
    /// long as a corrupt file stayed selected.
    ///
    /// The notification is posted but the pane is **not** opened. A wallpaper
    /// that did not load is a thing to explain, not an emergency to interrupt
    /// with: the desktop is fully usable, and a panel that shoved itself over
    /// the screen at login because of a missing file would be worse than the
    /// missing file.
    fn set_wallpaper_error(&mut self, why: Option<String>) {
        if why == self.wallpaper_error {
            return;
        }
        if let Some(message) = why.as_deref() {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            // The id is discarded: nothing here ever needs to refer back to
            // this notification. It is a message, not a progress indicator to
            // be updated later.
            let _ = self.shell.notify(notif_pane::Notification {
                id: 0,
                app_name: "Desktop".to_owned(),
                title: "Wallpaper could not be shown".to_owned(),
                body: message.to_owned(),
                timestamp: now,
                // Not `High`: the desktop still works and still has a
                // background. High priority is for something the user has to
                // act on now, and reserving it for those is what stops it
                // meaning nothing.
                priority: notif_pane::NotifPriority::Normal,
                read: false,
                action: None,
                // Left to `notify`, which is the only thing that knows whether
                // focus assist is silencing "Desktop" right now. Setting it
                // here would be this call answering a question it cannot see
                // the state of.
                silent: false,
            });
            self.dirty = true;
        }
        self.wallpaper_error = why;
    }

    fn refresh_wallpaper_image(&mut self) -> Result<(), Error<T>> {
        let id = self.wallpaper.current_image_id();
        let want = self.wallpaper.current_image_path().map(str::to_owned);

        // An id of zero means "no picture": `render_image` emits no `Image`
        // command at all, so anything still uploaded is unreachable and costs
        // the link's image budget for nothing.
        let Some(path) = want.filter(|_| id != 0) else {
            self.release_wallpaper_image()?;
            self.set_wallpaper_error(None);
            return Ok(());
        };

        if self
            .wallpaper_image
            .as_ref()
            .is_some_and(|(had, from)| *had == id && *from == path)
        {
            return Ok(());
        }

        // Released before the read rather than after the upload. The old
        // picture is already unreachable — the render tree names the new id —
        // and holding it across a decode is what would make a slideshow of
        // full-screen images charge the link's budget for two at once, so a
        // budget that fits the wallpaper would still refuse the next slide.
        self.release_wallpaper_image()?;
        self.wallpaper_image = Some((id, path.clone()));

        let decoded = std::fs::read(&path)
            .map_err(|e| format!("{path}: {e}"))
            .and_then(|bytes| {
                // The default limit is the compositor's own buffer ceiling, so
                // a picture refused here is one the compositor would have
                // refused anyway — and refusing it from the header costs a
                // header rather than a decompressed framebuffer.
                imagecodec::decode(&bytes, imagecodec::Limits::default())
                    .map_err(|e| format!("{path}: {e}"))
            });
        let image = match decoded {
            Ok(image) => image,
            Err(why) => {
                self.set_wallpaper_error(Some(why));
                return Ok(());
            }
        };

        let (width, height, stride) = (image.width, image.height, image.stride());
        let bytes = image.to_argb_bytes();
        let Some(mut handle) = self.events.window_mut(self.background.window) else {
            // Unreachable in a live session: the background surface is created
            // in `start` and never closed. Handled rather than unwrapped
            // because "the compositor cannot lose my window" is an assumption
            // about the other end of a socket, and this crate does not get to
            // make those.
            self.set_wallpaper_error(Some(format!("{path}: the background surface is gone")));
            return Ok(());
        };
        match handle.upload_image(id, width, height, stride, PixelFormat::Argb8888, bytes) {
            Ok(()) => {
                self.set_wallpaper_error(None);
                Ok(())
            }
            // A refusal is the compositor saying "not this picture" — too big
            // for the link's image budget, most likely — and is exactly as
            // survivable as a corrupt file: the colour underlay still paints.
            // Every other error says the connection itself is unusable, and
            // those propagate, because the `submit` two lines later would fail
            // the same way and swallowing them here would only delay it.
            Err(ConnectionError::Refused(why)) => {
                self.set_wallpaper_error(Some(format!("{path}: {why}")));
                Ok(())
            }
            Err(other) => Err(other),
        }
    }

    /// Give back whatever the background surface is holding, if anything.
    ///
    /// Dropping an id that was never successfully uploaded is not an error —
    /// see [`oswindow::WindowHandle::drop_image`] — which is what lets this be
    /// called without first asking whether the last attempt worked.
    fn release_wallpaper_image(&mut self) -> Result<(), Error<T>> {
        let Some((id, _)) = self.wallpaper_image.take() else {
            return Ok(());
        };
        if let Some(mut handle) = self.events.window_mut(self.background.window) {
            handle.drop_image(id)?;
        }
        Ok(())
    }

    /// Give the Run box's file chooser a listing of whatever directory it is
    /// showing, if it is showing one it has not been given.
    ///
    /// This is the filesystem half of the split described on
    /// `DesktopShell::run_browser_listed`: the shell holds the chooser and
    /// knows what directory it is in, and the session does the reading. Keeping
    /// the read out here is what lets the shell's several thousand tests run
    /// with no filesystem at all, and is the same arrangement the wallpaper
    /// uses — the manager names a picture, `refresh_wallpaper_image` reads it.
    ///
    /// One listing per paint is enough, and is not a limit in practice: a
    /// navigation marks the shell dirty, a dirty shell paints, and the paint
    /// comes through here. Answering repeatedly in a loop would be answering a
    /// question the user has not asked yet.
    ///
    /// An unreadable directory yields an empty listing rather than an error —
    /// see `guitk::dialog::list_directory`. A user who wandered into a folder
    /// they cannot read has not broken anything, and a modal error over a modal
    /// chooser would be two dialogs deep for a normal thing to find.
    fn refresh_run_browser(&mut self) {
        let Some(path) = self
            .shell
            .run_browser_wants()
            .map(std::path::Path::to_path_buf)
        else {
            return;
        };
        let entries = guitk::dialog::list_directory(&path);
        self.shell.set_run_browser_entries(entries);
        // The listing changed what the chooser draws, and nothing else in this
        // paint knows that: the event that caused the navigation was handled
        // before the read happened.
        self.dirty = true;
    }

    /// Paint the taskbar, and the menus if any are open.
    ///
    /// # Errors
    ///
    /// As [`EventLoop::submit`] and [`oswindow::WindowHandle::set_visible`].
    pub fn paint_chrome(&mut self) -> Result<(), Error<T>> {
        // Before the picture, the contents the picture is of — the same
        // ordering, and for the same reason, as `paint_background`'s upload of
        // the wallpaper's pixels. The shell reads no files, so a chooser it has
        // put up is showing an empty directory until somebody lists it, and
        // that somebody is here.
        self.refresh_run_browser();
        let bar = self.shell.render_taskbar();
        self.events
            .submit(self.panel.window, &self.panel.localize(&bar))?;

        let open = self.popups_open();
        if open != self.popups_shown {
            if let Some(mut handle) = self.events.window_mut(self.popups.window) {
                handle.set_visible(open)?;
            }
            self.popups_shown = open;
        }
        if open {
            let mut tree = RenderTree::new();
            // Alt-Tab last: it is modal, and while it is up it belongs over
            // whatever was already open rather than under it.
            for part in [
                // The overview first, because it is the only part that covers
                // the whole screen and dims what is behind it: anything drawn
                // under it would be dimmed twice and read as a smudge. It is
                // mutually exclusive with the rest in practice (`dismiss_popups`
                // closes it, and opening it closes them), so like the three
                // below this ordering states an invariant rather than resolves
                // a case that arises.
                self.shell.render_overview(),
                self.shell.render_start_menu(),
                self.shell.render_calendar(),
                // Over the menus, under Alt-Tab: opening the tiling overlay
                // dismisses them (`toggle_zone_overlay`), so the order between
                // the three above is a statement of the invariant rather than a
                // case that arises.
                self.shell.render_zone_overlay(),
                // Over both of the full-screen overlays above, because both dim
                // what is behind them and a reference card read through a scrim
                // is a reference card the user opened for nothing — and neither
                // one closes the card, so this is a case that genuinely arises
                // rather than an invariant. Under Alt-Tab for the same reason
                // the menus are: Alt+Tab leaves the card open, and the switcher
                // is modal while it is up.
                self.shell.render_shortcut_card(),
                self.shell.render_alt_tab(),
                // Last of all, over Alt-Tab too, and for the opposite reason to
                // the overview: the pane's scrim dims what is *behind* it, and
                // the pane itself is a column that leaves most of the screen
                // showing. Drawn earlier it would be the thing dimmed, by its
                // own scrim, under a switcher it is supposed to be in front of.
                self.shell.render_notifications(),
                // Last of all. The Run box is the shell's only modal dialog:
                // while it is up it owns the keyboard
                // (`DesktopShell::handle_hotkey`) and every press
                // (`handle_mouse`), and a surface that owns the input has to be
                // the surface on top or the user is typing into something they
                // cannot see. Nothing else is open underneath it in practice —
                // opening it dismisses the popups — so this states the invariant
                // rather than resolving a case that arises.
                self.shell.render_run_dialog(),
                // Over even the Run box, and this one *is* a case that arises
                // rather than an invariant: the chooser is raised from the box
                // and the box stays up underneath it, so that cancelling
                // returns the user to the command line they had typed. The
                // input routing agrees — `handle_mouse_inner` and
                // `handle_hotkey_inner` both offer the chooser every event
                // before the box sees one.
                self.shell.render_run_browser(),
            ]
            .into_iter()
            .flatten()
            {
                tree.commands.extend(part.commands);
            }
            self.events
                .submit(self.popups.window, &self.popups.localize(&tree))?;
        }

        // The overlays, on their own surface and on their own schedule: an OSD
        // is not a popup and neither one's visibility implies anything about
        // the other's.
        let overlays = self.shell.render_osd();
        let showing = overlays.is_some();
        if showing != self.osd_shown {
            if let Some(mut handle) = self.events.window_mut(self.osd.window) {
                handle.set_visible(showing)?;
            }
            self.osd_shown = showing;
        }
        if let Some(tree) = overlays {
            self.events
                .submit(self.osd.window, &self.osd.localize(&tree))?;
        }
        Ok(())
    }

    /// Whether anything the popup surface exists to show is showing.
    ///
    /// The power menu is not consulted: it is a submenu of the start menu and
    /// `DesktopShell` keeps it closed whenever the start menu is
    /// (`close_start_menu`), so a `power_menu_open` term here could only ever
    /// be redundant — or, if that invariant broke, could hide the break.
    fn popups_open(&self) -> bool {
        self.shell.start_menu_open
            || self.shell.calendar.visible
            || self.shell.notifications.pane_state().is_visible()
            || self.shell.alt_tab_active
            || self.shell.snap.is_overlay_visible()
            || self.shell.overview.visible
            || self.shell.run_dialog.is_visible()
            || self.shell.shortcut_card_open
    }

    /// Handle everything waiting, without blocking. Reports whether anything
    /// happened.
    ///
    /// # Errors
    ///
    /// As [`EventLoop::poll`] and [`Self::paint_chrome`].
    pub fn pump(&mut self) -> Result<bool, Error<T>> {
        let mut worked = false;
        while let Some((window, event)) = self.events.poll()? {
            worked = true;
            self.dispatch(window, event)?;
        }

        // *After* the input, deliberately. `poll` is also what reads the window
        // list off the wire, so by here the connection may already hold a newer
        // desktop than the taskbar the user clicked was drawn from. Folding it
        // in first would renumber `taskbar_windows` underneath a click that was
        // aimed at the old numbering — the click would minimise whichever
        // window had inherited the slot. The shell's own copy therefore stays
        // one revision behind until every event of this batch has been answered
        // against the picture it was aimed at.
        let latest = self.events.desktop_revision();
        if latest != self.revision {
            self.revision = latest;
            // Whole, not just the windows: which desktop is showing arrives
            // in the same frame, and a shell that read the two separately
            // could read them from different frames.
            //
            // The requests are collected rather than sent inside the `if`
            // because sending borrows `self.events` mutably and the list is a
            // borrow *of* it. They are what the user's window rules asked for
            // about windows that arrived in this list: a rule can only be
            // carried out by asking the compositor, and the shell is the only
            // thing here holding a connection.
            let requests = if let Some(list) = self.events.desktop() {
                self.shell.apply_window_list(list)
            } else {
                Vec::new()
            };
            for request in requests {
                self.request(request)?;
            }
            self.dirty = true;
            worked = true;
        }

        if self.dirty {
            self.dirty = false;
            self.paint_chrome()?;
        }

        // Last, and unconditionally. Not in `dispatch`, because a popup does not
        // only close in answer to an event — `run` closes them on shutdown, a
        // caller may close them outright, and the window-list fold above can
        // take the last window a menu was about — and not behind `dirty`, since
        // a shell that decided nothing needed repainting has still changed what
        // Escape means. One boolean compare per pump buys never having to ask
        // which of those paths was taken.
        self.reconcile_escape_grab()?;
        Ok(worked)
    }

    /// Run until [`quit`](Self::quit) or until the compositor hangs up.
    ///
    /// # Errors
    ///
    /// As [`Self::pump`], plus whatever the transport's `wait` reports.
    pub fn run(&mut self) -> Result<(), Error<T>> {
        self.running = true;
        while self.running && self.events.connection().is_open() {
            // Block only when there was nothing to do: waiting after a burst
            // would add a frame of latency to the next one for nothing.
            if !self.pump()? {
                // `EventLoop::wait`, not `Connection::wait`. The connection
                // knows nothing about wake-ups, so parking there would park
                // straight past every deadline the shell registered — and the
                // only symptom would be an animation that stops.
                self.events.wait()?;
            }
        }
        self.running = false;
        Ok(())
    }

    /// The event loop underneath.
    ///
    /// Public so a shell can register frame-clock wake-ups
    /// ([`EventLoop::wake_after`]) for whatever it is animating. Everything
    /// else the session needs from the loop it does itself.
    pub const fn events_mut(&mut self) -> &mut EventLoop<T> {
        &mut self.events
    }

    /// Everything currently moving.
    #[must_use]
    pub const fn animations(&self) -> &AnimationManager {
        &self.animations
    }

    /// Start a window animation and ask the frame clock for the first frame.
    ///
    /// Arming here rather than leaving it to the caller is the point: an
    /// animation that is registered but never woken is the defect this whole
    /// path exists to remove, and it is invisible in a unit test because a
    /// `tick` called by hand advances exactly as designed.
    ///
    /// Refused silently when reduced motion is on — see
    /// [`AnimationManager::animate_window`]. The wake-up follows the same
    /// answer, so reduced motion really is an idle desktop rather than an
    /// invisible animation still costing a wake-up every frame.
    pub fn animate_window(&mut self, anim: WindowAnimation) {
        self.animations.animate_window(anim);
        self.arm_next_frame();
    }

    /// Slide the desktop, as [`AnimationManager::animate_desktop_switch`], and
    /// ask for the first frame.
    pub fn animate_desktop_switch(&mut self, direction: f32) {
        let width = f32::from(u16::try_from(self.shell.screen_width).unwrap_or(u16::MAX));
        self.animations.animate_desktop_switch(direction, width);
        self.arm_next_frame();
    }

    /// Turn animations off, or back on, for accessibility.
    ///
    /// Turning them off cancels what is already running rather than letting it
    /// finish: a user who has just asked for less motion is asking about the
    /// motion on screen now, not about the next one.
    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.animations.reduced_motion = reduced;
        if reduced {
            self.animations.cancel_all();
            self.shell.overview.end_fade();
            self.dirty = true;
        }
    }

    /// Stop [`run`](Self::run) at the end of the current batch.
    pub const fn quit(&mut self) {
        self.running = false;
    }

    /// Whether [`run`](Self::run) is executing.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.running
    }

    /// Which of the three surfaces an event arrived for, if any.
    ///
    /// `None` for an id this shell does not own. That is not impossible — a
    /// compositor is free to send anything — and the right answer is to ignore
    /// it rather than to guess a surface, since guessing means translating by
    /// the wrong origin and acting on a point the user did not click.
    const fn surface_for(&self, window: u64) -> Option<Surface> {
        if window == self.background.window {
            Some(self.background)
        } else if window == self.panel.window {
            Some(self.panel)
        } else if window == self.popups.window {
            Some(self.popups)
        } else if window == self.osd.window {
            // Listed even though it is click-through and so can never carry a
            // pointer event: a surface the shell owns but does not recognise
            // would be counted as the compositor misrouting, and every
            // non-pointer event the loop may deliver about it — a resize, a
            // close — would be dropped on that mistaken ground.
            Some(self.osd)
        } else {
            None
        }
    }

    fn dispatch(&mut self, window: u64, event: Event) -> Result<(), Error<T>> {
        let Some(surface) = self.surface_for(window) else {
            return Ok(());
        };
        // Sampled around the whole handler rather than beside the one call that
        // opens the overview today. `OverviewState::show` is not called from
        // here at all — it is reached through `handle_hotkey`, several frames
        // down, and the shell is free to grow a second way in (a taskbar
        // button, a corner gesture) without this having to learn about it. What
        // is watched is the observable fact "it is open now and was not
        // before", which no new caller can arrive behind.
        let overview_was_visible = self.shell.overview.visible;
        // Same trick for the notification pane, with one extra condition: the
        // pane's slide *ends* by changing this same flag (a slide-out finishes
        // at `Hidden`), and that happens in `step_frame`. Sampling around a
        // tick as well would see the end of the slide as a fresh gesture and
        // start the slide over, for ever.
        let notifications_were_open = self.shell.notifications.pane_state().is_visible();
        // And once more for the overlays. Sampled here rather than beside
        // `handle_hotkey` because a volume key is not the only way one can
        // appear — `shell_mut().show_osd(..)` is public, and the shell will grow
        // its own callers (a brightness key, a caps-lock report) that this must
        // not have to learn about one at a time.
        let osd_was_visible = self.shell.osd.has_visible();
        let is_tick = matches!(event, Event::Tick { .. });
        match event {
            Event::Mouse(mouse) => self.pointer(&surface.to_screen(&mouse))?,
            Event::Key(key) => {
                let outcome = self.shell.handle_hotkey(&key);
                if outcome.consumed {
                    self.dirty = true;
                }
                // Sent in the order the shortcut named them, and every one is
                // sent even if an earlier one is refused — Super+D minimizes
                // every window on the desktop, and one of them having just
                // closed is no reason to leave the rest on screen.
                for request in outcome.requests {
                    self.request(request)?;
                }
                // The keyboard's half of `ShellAction::Launch`, and it lands in
                // the same queue that the pointer's half does — a command typed
                // into the Run box and a start-menu entry clicked are the same
                // ask, and whoever drains `take_launches` should not be able to
                // tell which one it was. `extend`, not a loop, because unlike
                // `request` there is nothing here that can fail: the session
                // does not start the program either, it only records that one
                // was asked for.
                self.launches.extend(outcome.launches);
            }
            // The background surface is screen-sized by construction, so the
            // compositor resizing it *is* the display changing size. Everything
            // the shell places is derived from the screen, so the other two
            // surfaces have to move with it.
            Event::Resize { width, height } if window == self.background.window => {
                self.resize_display(width, height)?;
            }
            // The frame clock. `elapsed_ms` is measured wall time since the
            // previous frame of this animation, not the interval that was
            // asked for, so a late frame steps further rather than slowing the
            // animation down.
            Event::Tick { elapsed_ms } => self.step_frame(elapsed_ms),
            _ => {}
        }
        if !overview_was_visible && self.shell.overview.visible {
            self.begin_overview_fade();
        }
        // Both directions, unlike the overview: the pane slides out as well as
        // in, so what is watched is that the answer *changed*, not that it
        // became true.
        if !is_tick && notifications_were_open != self.shell.notifications.pane_state().is_visible()
        {
            self.begin_notifications_slide();
        }
        // An overlay that has just appeared has started something nothing else
        // will wake up for: the overview and the pane are opened by gestures
        // that go on to draw, but an OSD's whole life is a timeout, and a
        // timeout with no frame behind it never comes due.
        //
        // Only on the transition, never unconditionally: re-arming a wake-up
        // that is already pending resets the instant its delta is measured
        // from, so a keystroke landing mid-fade would silently shorten that
        // frame and make every animation on screen jump.
        if !osd_was_visible && self.shell.osd.has_visible() {
            self.arm_next_frame();
        }
        Ok(())
    }

    /// Hold Escape exactly while the shell has something for it to close.
    ///
    /// The one shortcut that cannot be claimed once and kept. A permanent grab
    /// would take Escape from every dialog, every text field and every menu in
    /// every application on the desktop; no grab at all would mean a start menu
    /// opened by a click could not be closed by a key, because the press would
    /// go to whatever window is behind it.
    ///
    /// Reconciled from [`DesktopShell::any_popup_open`] once per
    /// [`pump`](Self::pump), rather than at the handful of places that open and
    /// close menus. There are at least six such surfaces and several ways into
    /// each — a click, a chord, a tray icon, the pane's own Escape — and a
    /// scheme that had to be extended at each new one is a scheme that will be
    /// forgotten at the seventh. What is watched is the observable answer, which
    /// no new caller can arrive behind.
    ///
    /// Per pump and not per *event*, because a menu also closes without one:
    /// `run` dismisses them on the way out, and a caller holding the session can
    /// call `dismiss_popups` directly. Both used to leave Escape claimed with
    /// nothing left for it to close, which is the exact harm this is here to
    /// avoid, only inverted.
    ///
    /// `escape_held` makes this a round trip only when the answer *changes*, so
    /// an idle desktop with nothing open costs nothing per pump.
    fn reconcile_escape_grab(&mut self) -> Result<(), Error<T>> {
        let wanted = self.shell.any_popup_open();
        if wanted == self.escape_held {
            return Ok(());
        }
        // Asked of the shell rather than of a constant, because the chords the
        // user has put on conditional actions are the user's business: a
        // rebound "dismiss" is still grabbed and released with the popups.
        for (key, modifiers) in self.shell.conditional_chords() {
            if wanted {
                self.events.grab_key(self.panel.window, key, modifiers)?;
            } else {
                self.events.ungrab_key(self.panel.window, key, modifiers)?;
            }
        }
        self.escape_held = wanted;
        Ok(())
    }

    /// Rewind the notification pane's jump into a slide, now that something has
    /// opened or closed it.
    ///
    /// The counterpart of [`begin_overview_fade`](Self::begin_overview_fade)
    /// and for the same reason: `show`/`hide`/`toggle` land the pane on its
    /// destination, so a caller with no frame clock — a test, an embedder, the
    /// login screen — gets a pane that is fully open or fully gone rather than
    /// one stuck at zero progress off the right edge of the screen. This
    /// session has a clock, so it puts the pane back where it started and lets
    /// the clock carry it. See `design-decisions.md` §520 and §562.
    fn begin_notifications_slide(&mut self) {
        if self.animations.reduced_motion {
            return;
        }
        self.shell.notifications.begin_slide();
        self.arm_next_frame();
    }

    /// Start the overview's backdrop fade, now that something has opened it.
    ///
    /// This session is the caller that owns a clock, and
    /// [`OverviewState::begin_fade`] is documented as only for such a caller:
    /// the fade is deliberately *not* started by `show`, so that every other
    /// caller of `show` — a test, a layout pass, an embedder driving the shell
    /// by hand — gets a fully-open overview instead of one waiting for a frame
    /// that never comes. See `design-decisions.md` §520.
    fn begin_overview_fade(&mut self) {
        if self.animations.reduced_motion {
            return;
        }
        self.shell
            .overview
            .begin_fade(self.shell.overview_config.fade_ms);
        self.arm_next_frame();
    }

    /// Advance every animation by the time the frame clock measured, and decide
    /// whether to ask for another frame.
    ///
    /// The re-arm is here rather than at the call sites that *start* animations
    /// because it is the only place that knows whether anything is still
    /// moving. Wake-ups are one-shot (`design-decisions.md` §521 §2), so
    /// "stop" is what happens by not doing this — a handler that returns early
    /// leaves the desktop idle rather than leaving a timer running for ever.
    fn step_frame(&mut self, elapsed_ms: u64) {
        // A `u64` of milliseconds that does not fit in a `u32` is 49 days, so
        // this is the loop having been stopped in a debugger rather than a real
        // frame. Saturating puts every animation at its end, which is where a
        // user returning after 49 days expects to find them.
        let dt = u32::try_from(elapsed_ms).unwrap_or(u32::MAX);
        // Asked *before* the step, not after: the step that finishes the last
        // animation is still a step that changed what is on screen, and reading
        // this afterwards would drop exactly the frame that puts everything at
        // its destination.
        let moved = self.anything_moving();
        // Stepped whatever the manager says, because the overview's fade is not
        // the manager's — it lives on the overview so that the overview can be
        // drawn correctly by a caller that has no manager at all.
        self.shell.overview.tick_fade(dt);
        // The pane keeps its own clock too, and in seconds rather than
        // milliseconds: it is a `guitk` widget, whose animation convention is a
        // float of seconds. Converted here rather than changed there, because
        // the millisecond integer is this session's convention (it is what the
        // wake-up reports) and the pane is drawn by callers that have neither.
        #[expect(
            clippy::cast_precision_loss,
            reason = "a frame long enough to lose f32 precision is 97 days"
        )]
        let secs = dt as f32 / 1000.0;
        self.shell.notifications.tick(secs);
        // Given `elapsed_ms` rather than `dt`: the overlay clock is the one
        // animator here that is not stepped but *dated*, so a frame long enough
        // to saturate the `u32` above must not be quietly shortened to 49 days
        // when the whole point of that frame is to retire everything on screen.
        self.shell.advance_osd(elapsed_ms);
        // The stepped rectangles are deliberately not used here. A window's
        // geometry belongs to the compositor, not to the shell — the shell
        // cannot move a window by drawing it somewhere else — so a window
        // animation run in this process can only be read back through
        // `animations()` by whatever asked for it. Until the compositor grows
        // its own animation path, `animate_window` is for callers that render
        // the result themselves.
        drop(self.animations.tick(dt));
        if moved {
            self.dirty = true;
        }
        self.arm_next_frame();
    }

    /// Ask for the next frame if anything is still moving.
    fn arm_next_frame(&mut self) {
        if self.anything_moving() {
            self.events.wake_after(self.panel.window, FRAME_INTERVAL);
        }
    }

    /// Whether anything on screen is mid-animation.
    ///
    /// The single condition that keeps an idle desktop idle: false here means
    /// no wake-up is registered and the loop parks with no bound at all. Every
    /// animated thing the shell owns must be named here — one that is not is a
    /// thing that stops moving the moment nothing else is.
    fn anything_moving(&self) -> bool {
        self.animations.has_active()
            || self.shell.overview.is_fading()
            || self.shell.notifications.is_sliding()
            // `has_visible`, not "is fading": an overlay sitting at full opacity
            // waiting for its timeout is still a thing that has to be woken up
            // for, because the timeout is the thing the wake-up is measuring
            // towards. A term that only counted fades would leave a freshly
            // shown OSD on screen for ever on an otherwise idle desktop.
            || self.shell.osd.has_visible()
    }

    /// One pointer event, already in screen coordinates.
    fn pointer(&mut self, event: &MouseEvent) -> Result<(), Error<T>> {
        match self.shell.handle_mouse(event) {
            // Not the shell's, and nothing the shell drew has changed. The
            // compositor routes a press to the topmost window containing it, so
            // in a live session this is a click on bare desktop.
            ShellAction::Pass => {}
            ShellAction::Consumed => self.dirty = true,
            ShellAction::Launch(path) => {
                self.launches.push(path);
                self.dirty = true;
            }
            ShellAction::Control(request) => self.request(request)?,
        }
        Ok(())
    }

    /// Send one thing the shell asked for on to the compositor.
    ///
    /// Both input paths land here: a taskbar click and Alt+F4 are the same ask
    /// about the same window, and the compositor cannot tell them apart. The
    /// repaint is unconditional because the shell has drawn something that
    /// depends on the answer — a pressed button, a closed switcher — even when
    /// the answer turns out to be no.
    fn request(&mut self, request: ShellRequest) -> Result<(), Error<T>> {
        self.dirty = true;
        let sent = match request {
            ShellRequest::Window(WindowRequest { window, action }) => {
                self.events.control_window(window.0, action)
            }
            ShellRequest::SwitchDesktop { desktop } => self.events.switch_desktop(desktop),
            ShellRequest::MoveWindowToDesktop { window, desktop } => {
                self.events.move_window_to_desktop(window.0, desktop)
            }
        };
        match sent {
            Ok(()) => {}
            // A refusal means the window went away between the list the
            // button was drawn from and the click. That is an ordinary
            // race on a live desktop, not a fault: the next window list
            // is already on its way and the repaint above will drop the
            // button. Anything else — a dead transport, a frame that
            // will not decode — is fatal and propagates.
            Err(ConnectionError::Refused(_)) => {}
            Err(other) => return Err(other),
        }
        Ok(())
    }

    /// Follow the display to a new size.
    fn resize_display(&mut self, width: u32, height: u32) -> Result<(), Error<T>> {
        self.shell.screen_width = width;
        self.shell.screen_height = height;

        let bar = self.shell.taskbar_rect();
        self.panel.origin = (bar.x, bar.y);
        if let Some(mut handle) = self.events.window_mut(self.panel.window) {
            handle.set_position(pos(bar.x), pos(bar.y))?;
            handle.set_size(px(bar.w), px(bar.h))?;
        }
        if let Some(mut handle) = self.events.window_mut(self.popups.window) {
            handle.set_size(width, height)?;
        }
        // The overlay surface follows too. `DesktopShell::show_osd` re-seeds the
        // manager's own screen size on the way in, so the drawing would centre
        // itself correctly regardless — but on a surface still the old size, and
        // an overlay centred on a 4K desktop inside a 1080p window is clipped
        // away entirely.
        if let Some(mut handle) = self.events.window_mut(self.osd.window) {
            handle.set_size(width, height)?;
        }
        self.repaint()
    }
}

/// A surface spec for one of the shell's own windows.
///
/// Undecorated, unresizable and opaque, every time: a title bar on the taskbar
/// would be a title bar on the title bars, and a shell panel the user could
/// drag to a different size is a shell panel that no longer matches the work
/// area the windows were tiled into.
///
/// Clickable, too. The one surface that is not — the heads-up overlay — says so
/// at its own call site with `Spec { input_transparent: true, ..chrome(..) }`,
/// which is where a reader will be asking the question.
fn chrome(title: &str, width: u32, height: u32, at: (i32, i32), layer: Layer) -> Spec {
    Spec {
        title: title.to_owned(),
        // One id for all four surfaces, not one each: they are four windows of
        // a single program, and saying so is exactly what an app id is for.
        // Nothing reads it today — the shell's own chrome sits outside
        // `Layer::Normal`, so it never reaches the window list rules are
        // evaluated against — but leaving it empty would make the shell the one
        // program on the desktop that declines to name itself, and the first
        // tool to group windows by program would show it as four unrelated
        // strangers.
        app_id: "slateos-shell".to_owned(),
        width,
        height,
        position: Some(at),
        resizable: false,
        decorations: false,
        transparent: true,
        input_transparent: false,
        min_size: None,
        max_size: None,
        layer,
    }
}

/// A length in whole pixels. Float-to-integer `as` saturates in Rust, so a
/// negative or absurd length clamps instead of wrapping to something enormous.
fn px(v: f32) -> u32 {
    v.round() as u32
}

/// A coordinate in whole pixels, saturating as [`px`] does.
fn pos(v: f32) -> i32 {
    v.round() as i32
}

/// Seconds since the epoch, or 0 on a clock set before it.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests;
