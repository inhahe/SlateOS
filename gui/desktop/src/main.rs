//! The desktop shell's demonstration binary.
//!
//! Drives [`desktop::DesktopShell`] through a scripted session — take a window
//! list from the compositor, render the taskbar, walk the start menu, page the
//! calendar, tile and switch desktops — printing what each step produced. It is
//! a way to look at what the shell's surfaces render with no compositor to
//! submit them to.
//!
//! It is **not** the shell, and it is not the only thing that drives one:
//! [`desktop::session::ShellSession`] is the real loop, and it is what a live
//! session runs. This binary stays because a library whose only caller is its
//! own test suite is one refactor away from drifting from what a real session
//! does — it is a second caller, going through the public API.
//!
//! Everything below therefore goes through the one door a live session uses:
//! [`desktop::DesktopShell::apply_window_list`] to learn what exists, and a
//! *request* — printed here, since there is nothing to send it to — for
//! everything the shell wants changed. It used to open its windows with an
//! `add_window` that no live session had, and act on them with a private window
//! manager whose edits the next list from the compositor threw away; the demo
//! showed a snapped window at a rectangle no user would ever have seen.

use desktop::{DesktopShell, ShellAction, WindowInfo, WindowList, calendar, click};
use guitk::event::{Key, KeyEvent, Modifiers};

/// A window as the compositor would describe it. Nothing here has geometry:
/// where the window is is not something the shell is told, because it is not
/// something the shell decides.
fn window(id: u64, pid: u64, title: &str) -> WindowInfo {
    WindowInfo::new(id, pid, title)
}

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

    // The compositor says what is on the desktop. The list is bottom-to-top, so
    // the editor is the topmost window, and it is the one holding focus.
    let mut editor = window(3, 1003, "Text Editor");
    editor.focused = true;
    desktop.apply_window_list(&WindowList::new(
        0,
        vec![
            window(1, 1001, "Terminal"),
            window(2, 1002, "File Explorer"),
            editor,
        ],
    ));
    println!(
        "Compositor says {} windows, focused {:?}",
        desktop.taskbar_windows().len(),
        desktop.focused_window
    );

    // Render taskbar
    let taskbar = desktop.render_taskbar();
    println!("Taskbar rendered: {} commands", taskbar.len());

    // Test keyboard shortcuts
    let alt_f4 = KeyEvent {
        key: Key::F4,
        pressed: true,
        modifiers: Modifiers::alt(),
        text: String::new(),
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
        text: String::new(),
    };
    println!(
        "Escape closed it: {} (still open: {})",
        desktop.handle_hotkey(&escape).consumed,
        desktop.calendar.visible
    );

    // Tiling. The shortcut names an *edge*; which pixels that edge turns into is
    // worked out by the compositor from its own bounds, and this demo has no
    // rectangle to print because the shell never computes one.
    let snap_left = KeyEvent {
        key: Key::Left,
        pressed: true,
        modifiers: Modifiers {
            super_key: true,
            ..Modifiers::NONE
        },
        text: String::new(),
    };
    println!(
        "Super+Left asked the compositor for {:?}",
        desktop.handle_hotkey(&snap_left).requests
    );

    // A taskbar click, which is the same shape as the keystrokes above: the
    // compositor says what exists, the click produces a *request*, and the only
    // thing that moves the shell is the next list.
    println!("\n-- taskbar --");
    let button = desktop.taskbar_button_rect(2);
    match desktop.handle_mouse(&click(button.x + button.w / 2.0, button.y + button.h / 2.0)) {
        ShellAction::Control(request) => {
            println!("Taskbar asked the compositor for {request:?}");
        }
        other => println!("Taskbar returned {other:?}"),
    }
    println!(
        "...and until the compositor answers, the shell still shows {} windows",
        desktop.taskbar_windows().len()
    );

    // The compositor did as it was asked, and the shell finds out the only way
    // it can.
    let mut away = window(3, 1003, "Text Editor");
    away.minimized = true;
    desktop.apply_window_list(&WindowList::new(
        0,
        vec![
            window(1, 1001, "Terminal"),
            window(2, 1002, "File Explorer"),
            away,
        ],
    ));
    println!(
        "After the next list: {} windows, focused {:?}",
        desktop.taskbar_windows().len(),
        desktop.focused_window
    );

    // Virtual desktop switching, which is a request like everything else: the
    // compositor holds each window's desktop number and hides the ones filed
    // elsewhere, so the shell asks and then waits for the list that says it
    // happened. Nothing below has changed yet, and that is the point.
    let switch = desktop.switch_desktop(1);
    println!("Asked the compositor for {switch:?}");
    println!(
        "...still showing desktop {} with {} windows, because no list has come back",
        desktop.current_desktop_number(),
        desktop.taskbar_windows().len()
    );

    // The compositor did it. Every window is still open, every one is now filed
    // on desktop 0, and the list says desktop 1 is what the screen shows — so
    // the taskbar has nothing to draw.
    desktop.apply_window_list(&WindowList::new(
        1,
        vec![
            window(1, 1001, "Terminal"),
            window(2, 1002, "File Explorer"),
        ],
    ));
    println!(
        "After the next list: desktop {} with {} windows",
        desktop.current_desktop_number(),
        desktop.taskbar_windows().len()
    );

    println!("\nDesktop shell initialized successfully.");
}
