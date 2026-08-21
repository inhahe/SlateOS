//! The desktop shell's demonstration binary.
//!
//! Drives [`desktop::DesktopShell`] through a scripted session — open windows,
//! render the taskbar, walk the start menu, page the calendar, snap and switch
//! desktops — printing what each step produced. It is a way to exercise the
//! shell without a compositor, and the reason the shell's surfaces have output
//! that can be looked at at all.
//!
//! It is **not** the shell. The shell is the library beside it, which is what a
//! real session runs; keeping this here as a `[[bin]]` is what stops the
//! library's only caller from being its own tests.

use desktop::{DesktopShell, ShellAction, calendar, click};
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

    // Render window decorations
    let decorations = desktop.render_window_decorations();
    println!("Window decorations: {} commands", decorations.len());

    // Test keyboard shortcuts
    let alt_f4 = KeyEvent {
        key: Key::F4,
        pressed: true,
        modifiers: Modifiers::alt(),
        text: None,
    };
    desktop.handle_hotkey(&alt_f4);
    println!("After Alt+F4: {} windows remaining", desktop.windows.len());

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
        desktop.handle_hotkey(&escape),
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

    // Test virtual desktop switching
    desktop.switch_desktop(1);
    println!(
        "Switched to desktop {}: {} visible windows",
        desktop.current_desktop_number(),
        desktop.visible_windows().len()
    );

    println!("\nDesktop shell initialized successfully.");
}
