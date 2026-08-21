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
//! # The three surfaces, and why three
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
//!
//! The popup surface is full-screen rather than menu-sized, and that is the
//! whole mechanism behind click-outside-to-dismiss: a press on bare desktop
//! while a menu is open has to reach the shell in order to close the menu, and
//! it can only do that if some surface of the shell's is under it. It is
//! unmapped while nothing is open, so an idle desktop is not covered by an
//! invisible sheet that eats every click.
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

use std::time::{SystemTime, UNIX_EPOCH};

use guitk::event::{Event, MouseEvent};
use guitk::render::RenderTree;
use oswindow::{ConnectionError, ConnectionTransport as Transport, Error, EventLoop, Layer, Spec};

use crate::wallpaper::WallpaperManager;
use crate::{DesktopShell, ShellAction, WindowRequest};

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
    /// The last window-list revision folded into `shell`.
    revision: u64,
    /// Whether the chrome needs repainting before the next block.
    dirty: bool,
    running: bool,
    launches: Vec<String>,
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

        events.watch_desktop(true)?;

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
            revision: 0,
            dirty: false,
            running: false,
            launches: Vec::new(),
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

    /// The programs the user has asked to start since this was last called.
    ///
    /// The one intent this loop cannot carry out itself. A shell has no channel
    /// to the process server, and inventing one here would put policy about
    /// *how* a program starts — namespace, capabilities, environment — inside
    /// the window manager. So the path comes out here for whoever does own
    /// process creation. See `known-issues.md`
    /// `TD-SHELL-HAS-NOWHERE-TO-SEND-A-LAUNCH`.
    pub fn take_launches(&mut self) -> Vec<String> {
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
        let width = self.shell.screen_width as f32;
        let height = self.shell.screen_height as f32;
        // The same zone the taskbar clock reads in — see
        // `DesktopShell::seconds_since_local_midnight` for why this is asked of
        // the shell rather than computed here.
        let day = self.shell.seconds_since_local_midnight(unix_now());
        let mut tree = RenderTree::new();
        tree.commands
            .extend(self.wallpaper.get_render_commands(width, height, day));
        self.events
            .submit(self.background.window, &self.background.localize(&tree))
    }

    /// Paint the taskbar, and the menus if any are open.
    ///
    /// # Errors
    ///
    /// As [`EventLoop::submit`] and [`oswindow::WindowHandle::set_visible`].
    pub fn paint_chrome(&mut self) -> Result<(), Error<T>> {
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
                self.shell.render_start_menu(),
                self.shell.render_calendar(),
                // Over the menus, under Alt-Tab: opening the tiling overlay
                // dismisses them (`toggle_zone_overlay`), so the order between
                // the three above is a statement of the invariant rather than a
                // case that arises.
                self.shell.render_zone_overlay(),
                self.shell.render_alt_tab(),
            ]
            .into_iter()
            .flatten()
            {
                tree.commands.extend(part.commands);
            }
            self.events
                .submit(self.popups.window, &self.popups.localize(&tree))?;
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
            || self.shell.alt_tab_active
            || self.shell.snap.is_overlay_visible()
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
        // in first would renumber `visible_windows` underneath a click that was
        // aimed at the old numbering — the click would minimise whichever
        // window had inherited the slot. The shell's own copy therefore stays
        // one revision behind until every event of this batch has been answered
        // against the picture it was aimed at.
        let latest = self.events.desktop_revision();
        if latest != self.revision {
            self.revision = latest;
            self.shell.apply_window_list(self.events.desktop_windows());
            self.dirty = true;
            worked = true;
        }

        if self.dirty {
            self.dirty = false;
            self.paint_chrome()?;
        }
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
                self.events.connection().wait()?;
            }
        }
        self.running = false;
        Ok(())
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
        } else {
            None
        }
    }

    fn dispatch(&mut self, window: u64, event: Event) -> Result<(), Error<T>> {
        let Some(surface) = self.surface_for(window) else {
            return Ok(());
        };
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
            }
            // The background surface is screen-sized by construction, so the
            // compositor resizing it *is* the display changing size. Everything
            // the shell places is derived from the screen, so the other two
            // surfaces have to move with it.
            Event::Resize { width, height } if window == self.background.window => {
                self.resize_display(width, height)?;
            }
            _ => {}
        }
        Ok(())
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
    fn request(&mut self, request: WindowRequest) -> Result<(), Error<T>> {
        self.dirty = true;
        match self.events.control_window(request.window.0, request.action) {
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
        self.repaint()
    }
}

/// A surface spec for one of the shell's own windows.
///
/// Undecorated, unresizable and opaque, every time: a title bar on the taskbar
/// would be a title bar on the title bars, and a shell panel the user could
/// drag to a different size is a shell panel that no longer matches the work
/// area the windows were tiled into.
fn chrome(title: &str, width: u32, height: u32, at: (i32, i32), layer: Layer) -> Spec {
    Spec {
        title: title.to_owned(),
        width,
        height,
        position: Some(at),
        resizable: false,
        decorations: false,
        transparent: true,
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
