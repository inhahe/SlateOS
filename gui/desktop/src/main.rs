//! The desktop shell's demonstration binary.
//!
//! Drives [`desktop::DesktopShell`] through a scripted session — open windows,
//! render the taskbar, walk the start menu, page the calendar, snap and switch
//! desktops — printing what each step produced. It is a way to look at what the
//! shell's surfaces render with no compositor to submit them to.
//!
//! It is **not** the shell, and it is no longer the only thing that drives one:
//! [`desktop::session::ShellSession`] is the real loop, and it is what a live
//! session runs. This binary stays because a library whose only caller is its
//! own test suite is one refactor away from drifting from what a real session
//! does — it is a second caller, going through the public API.
//!
//! It is also the last caller of [`desktop::DesktopShell::add_window`] and the
//! geometry methods around it — the old private window manager, which a real
//! session does not use because the compositor tells it what windows exist. The
//! `-- taskbar --` section below builds a *fresh* `DesktopShell` and feeds it
//! `apply_window_list` for exactly that reason: ids the shell minted and ids the
//! compositor assigned must not be mixed.

use desktop::{DesktopShell, ShellAction, WindowInfo, calendar, click};
use guitk::event::{Key, KeyEvent, Modifiers};

fn main() {
    let mut desktop = DesktopShell::new(1920, 1080);

    // Adopt whatever the user last chose in the appearance panel. Nothing to
    // recover from if the file is absent — that is a fresh install, and the
    // defaults are what it should look like.
    desktop.load_appearance();
    println!(
        "Appearance: {} theme, {:.0}% scaling, UI font {} at {}pt",
        if desktop.appearance.theme_mode.is_light() {
            "light"
        } else {
            "dark"
        },
        desktop.appearance.scale_factor() * 100.0,
        desktop.appearance.fonts.ui_font,
        desktop.appearance.fonts.ui_size,
    );

    // Simulate some windows
    let w1 = desktop.add_window("Terminal", 100, 100, 800, 600, 1001);
    let w2 = desktop.add_window("File Explorer", 200, 150, 700, 500, 1002);
    let _w3 = desktop.add_window("Text Editor", 300, 200, 900, 650, 1003);

    // Render taskbar
    let taskbar = desktop.render_taskbar();
    println!("Taskbar rendered: {} commands", taskbar.len());

    // Test keyboard shortcuts
    let alt_f4 = KeyEvent {
        key: Key::F4,
        pressed: true,
        modifiers: Modifiers::alt(),
        text: None,
    };
    let closed = desktop.handle_hotkey(&alt_f4);
    println!(
        "Alt+F4 asked the compositor for {:?}; the shell still shows {} windows",
        closed.requests,
        desktop.windows.len()
    );

    // Open the start menu and pick Settings out of it, the way a click would.
    let start = desktop.start_button_rect();
    desktop.handle_mouse(&click(start.x + 8.0, start.y + 8.0));
    let settings_row = desktop
        .start_menu_entries()
        .iter()
        .position(|entry| entry.name == "Settings")
        .and_then(|index| index.checked_sub(desktop.start_menu_scroll));
    if let Some(row) = settings_row {
        let rect = desktop.start_menu_row_rect(row);
        match desktop.handle_mouse(&click(rect.x + 8.0, rect.y + 8.0)) {
            ShellAction::Launch(path) => println!("Start menu asked to launch: {path}"),
            other => println!("Start menu returned {other:?}"),
        }
    }

    // Open the power menu from the start menu's footer and pick Shutdown, the
    // way a user reaching for the power button would.
    desktop.handle_mouse(&click(start.x + 8.0, start.y + 8.0));
    let power = desktop.power_button_rect();
    desktop.handle_mouse(&click(power.x + 8.0, power.y + 8.0));
    let shutdown_row = desktop
        .power_menu_entries()
        .iter()
        .position(|entry| entry.name == "Shutdown");
    if let Some(row) = shutdown_row {
        let rect = desktop.power_menu_row_rect(row);
        match desktop.handle_mouse(&click(rect.x + 8.0, rect.y + 8.0)) {
            ShellAction::Launch(path) => println!("Power menu asked to launch: {path}"),
            other => println!("Power menu returned {other:?}"),
        }
    }

    // Open the calendar from the tray clock, page to the next month, and shut
    // it with Escape — the whole popup path a user takes.
    let clock = desktop.clock_rect();
    desktop.handle_mouse(&click(clock.x + 4.0, clock.y + clock.h / 2.0));
    if let Some(tree) = desktop.render_calendar() {
        println!("Calendar popup: {} commands", tree.len());
    }
    let (cal_x, cal_y) = desktop.calendar_origin();
    let next =
        calendar::MonthLayout::new(&desktop.calendar, cal_x, cal_y, desktop.calendar_scale())
            .next_arrow();
    desktop.handle_mouse(&click(next.x + next.w / 2.0, next.y + next.h / 2.0));
    println!(
        "Calendar showing {}/{}",
        desktop.calendar.view_month, desktop.calendar.view_year
    );
    let escape = KeyEvent {
        key: Key::Escape,
        pressed: true,
        modifiers: Modifiers::default(),
        text: None,
    };
    println!(
        "Escape closed it: {} (still open: {})",
        desktop.handle_hotkey(&escape).consumed,
        desktop.calendar.visible
    );

    // Test window snapping
    desktop.snap_window(w1, true);
    desktop.snap_window(w2, false);
    if let Some(w) = desktop.windows.get(&w1) {
        println!(
            "Window 1 snapped left: {}x{} at ({},{})",
            w.width, w.height, w.x, w.y
        );
    }

    // Test virtual desktop switching. Which desktop is showing is the shell's
    // own; which window then has the keyboard is not, so the switch hands back a
    // request the same way a click does.
    let raise = desktop.switch_desktop(1);
    println!(
        "Switched to desktop {}: {} visible windows, raise {raise:?}",
        desktop.current_desktop_number(),
        desktop.visible_windows().len()
    );

    // The taskbar the way a live session drives it, which is the one path in
    // this file that does not go through the shell's own bookkeeping: the
    // compositor says what exists, a click produces a *request*, and the only
    // thing that moves the shell is the next list. Standing in for the
    // compositor here is this function, which is why the request is printed
    // rather than sent — but the shape is the real one, and the demo would stop
    // agreeing with itself if the shell ever went back to acting on its own.
    //
    // A shell of its own for this, deliberately. The session above opened its
    // windows with `add_window` and moved them between virtual desktops, and
    // ids from that regime would collide with the compositor's — which is
    // itself the point: the two ways of learning what exists are not meant to
    // be mixed, and a live shell only ever uses the second.
    println!("\n-- taskbar --");
    let mut desktop = DesktopShell::new(1920, 1080);
    let mut editor = WindowInfo::new(1, 1001, "notes.txt");
    editor.focused = true;
    desktop.apply_window_list(&[WindowInfo::new(0, 1000, "Terminal"), editor]);
    println!(
        "Compositor says {} windows, focused {:?}",
        desktop.visible_windows().len(),
        desktop.focused_window
    );

    let button = desktop.taskbar_button_rect(1);
    match desktop.handle_mouse(&click(button.x + button.w / 2.0, button.y + button.h / 2.0)) {
        ShellAction::Control(request) => {
            println!(
                "Taskbar asked the compositor for {:?} on {:?}",
                request.action, request.window
            );
        }
        other => println!("Taskbar returned {other:?}"),
    }
    println!(
        "...and until the compositor answers, the shell still shows {} windows",
        desktop.visible_windows().len()
    );

    // The compositor did as it was asked, and the shell finds out the only way
    // it can.
    let mut away = WindowInfo::new(1, 1001, "notes.txt");
    away.minimized = true;
    desktop.apply_window_list(&[WindowInfo::new(0, 1000, "Terminal"), away]);
    println!(
        "After the next list: {} windows, focused {:?}",
        desktop.visible_windows().len(),
        desktop.focused_window
    );

    println!("\nDesktop shell initialized successfully.");
}
