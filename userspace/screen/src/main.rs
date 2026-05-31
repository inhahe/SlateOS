//! OurOS Terminal Multiplexer
//!
//! Manage multiple terminal windows within a single console session.
//! Similar to GNU screen / tmux.
//!
//! # Usage
//!
//! ```text
//! screen                   Start new session
//! screen -S name           Start named session
//! screen -ls               List sessions
//! screen -r [name]         Reattach to session
//! screen -d -r name        Detach elsewhere, attach here
//! ```
//!
//! # Key Bindings (Ctrl+A prefix)
//!
//! ```text
//! Ctrl+A c     Create new window
//! Ctrl+A n     Next window
//! Ctrl+A p     Previous window
//! Ctrl+A 0-9   Switch to window N
//! Ctrl+A k     Kill current window
//! Ctrl+A A     Rename window
//! Ctrl+A "     List windows
//! Ctrl+A w     Show window bar
//! Ctrl+A d     Detach session
//! Ctrl+A [     Enter scrollback mode
//! Ctrl+A ?     Show help
//! ```

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Syscall interface
// ============================================================================

// Native OurOS console syscalls (kernel syscall/number.rs).  These were
// previously 0/1, which are SYS_YIELD and SYS_EXIT — so a "read char" call
// actually terminated the process.  Correct numbers are 100/101.
const SYS_CONSOLE_WRITE: u64 = 100;
const SYS_CONSOLE_READ_CHAR: u64 = 101;
// Native OurOS monotonic clock (kernel syscall/number.rs); no-arg, returns
// boot-relative nanoseconds in rax.  (Syscall 30 is SYS_IRQ_REGISTER.)
const SYS_CLOCK_MONOTONIC: u64 = 10;
const SYS_PROCESS_SPAWN: u64 = 500;

#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

fn console_write(s: &str) {
    unsafe {
        syscall3(SYS_CONSOLE_WRITE, s.as_ptr() as u64, s.len() as u64, 0);
    }
}

fn console_read_char() -> Option<u8> {
    let ret = unsafe { syscall3(SYS_CONSOLE_READ_CHAR, 0, 0, 0) };
    if ret < 0 { None } else { Some(ret as u8) }
}

fn clock_ns() -> u64 {
    let ret = unsafe { syscall3(SYS_CLOCK_MONOTONIC, 0, 0, 0) };
    if ret < 0 { 0 } else { ret as u64 }
}

// ============================================================================
// Terminal helpers
// ============================================================================

fn term_clear() {
    console_write("\x1b[2J\x1b[H");
}

fn term_cursor_to(row: u16, col: u16) {
    let s = format!("\x1b[{};{}H", row, col);
    console_write(&s);
}

fn term_reverse_video() {
    console_write("\x1b[7m");
}

fn term_reset_attr() {
    console_write("\x1b[0m");
}

fn term_clear_line() {
    console_write("\x1b[2K");
}

fn term_set_scroll_region(top: u16, bottom: u16) {
    let s = format!("\x1b[{};{}r", top, bottom);
    console_write(&s);
}

fn term_alt_screen_on() {
    console_write("\x1b[?1049h");
}

fn term_alt_screen_off() {
    console_write("\x1b[?1049l");
}

// ============================================================================
// Configuration
// ============================================================================

const MAX_WINDOWS: usize = 10;
const SCROLLBACK_LINES: usize = 1000;
const TERM_ROWS: u16 = 25;
const TERM_COLS: u16 = 80;

// ============================================================================
// Window
// ============================================================================

struct Window {
    id: usize,
    title: String,
    scrollback: Vec<String>,
    alive: bool,
    // Current visible content (simplified — track last rendered lines).
    scroll_offset: usize,
    input_buffer: String,
}

impl Window {
    fn new(id: usize, title: &str) -> Self {
        Window {
            id,
            title: title.to_string(),
            scrollback: Vec::new(),
            alive: true,
            scroll_offset: 0,
            input_buffer: String::new(),
        }
    }

    fn add_line(&mut self, line: &str) {
        self.scrollback.push(line.to_string());
        if self.scrollback.len() > SCROLLBACK_LINES {
            self.scrollback.remove(0);
        }
    }

    fn visible_lines(&self, rows: u16) -> &[String] {
        let total = self.scrollback.len();
        let visible = (rows as usize).saturating_sub(2); // Reserve 2 for status bars.
        let start = if total > visible {
            if self.scroll_offset > 0 {
                total.saturating_sub(visible + self.scroll_offset)
            } else {
                total.saturating_sub(visible)
            }
        } else {
            0
        };
        let end = (start + visible).min(total);
        &self.scrollback[start..end]
    }
}

// ============================================================================
// Session
// ============================================================================

struct Session {
    name: String,
    windows: Vec<Window>,
    active_window: usize,
    next_window_id: usize,
    running: bool,
    showing_help: bool,
    showing_window_list: bool,
    renaming: bool,
    rename_buffer: String,
    scrollback_mode: bool,
}

impl Session {
    fn new(name: &str) -> Self {
        let mut session = Session {
            name: name.to_string(),
            windows: Vec::new(),
            active_window: 0,
            next_window_id: 0,
            running: true,
            showing_help: false,
            showing_window_list: false,
            renaming: false,
            rename_buffer: String::new(),
            scrollback_mode: false,
        };
        session.create_window("shell");
        session
    }

    fn create_window(&mut self, title: &str) -> usize {
        if self.windows.len() >= MAX_WINDOWS {
            return self.active_window;
        }
        let id = self.next_window_id;
        self.next_window_id += 1;
        let mut win = Window::new(id, title);
        win.add_line(&format!("OurOS Screen — Window {} ({})", id, title));
        win.add_line("Type commands here. Use Ctrl+A ? for help.");
        win.add_line("");
        self.windows.push(win);
        let idx = self.windows.len() - 1;
        self.active_window = idx;
        idx
    }

    fn kill_window(&mut self, idx: usize) {
        if idx < self.windows.len() {
            self.windows[idx].alive = false;
            self.windows.remove(idx);
            if self.windows.is_empty() {
                self.running = false;
            } else if self.active_window >= self.windows.len() {
                self.active_window = self.windows.len() - 1;
            }
        }
    }

    fn next_window(&mut self) {
        if !self.windows.is_empty() {
            self.active_window = (self.active_window + 1) % self.windows.len();
        }
    }

    fn prev_window(&mut self) {
        if !self.windows.is_empty() {
            if self.active_window == 0 {
                self.active_window = self.windows.len() - 1;
            } else {
                self.active_window -= 1;
            }
        }
    }

    fn switch_to(&mut self, idx: usize) {
        if idx < self.windows.len() {
            self.active_window = idx;
        }
    }

    fn active(&self) -> Option<&Window> {
        self.windows.get(self.active_window)
    }

    fn active_mut(&mut self) -> Option<&mut Window> {
        self.windows.get_mut(self.active_window)
    }
}

// ============================================================================
// Rendering
// ============================================================================

fn render_status_bar(session: &Session) {
    let rows = TERM_ROWS;

    // Bottom status bar: window list.
    term_cursor_to(rows - 1, 1);
    term_reverse_video();
    term_clear_line();

    let mut bar = format!("[screen: {}] ", session.name);
    for (i, win) in session.windows.iter().enumerate() {
        if i == session.active_window {
            bar.push_str(&format!("*{} {} ", win.id, win.title));
        } else {
            bar.push_str(&format!(" {} {} ", win.id, win.title));
        }
    }

    // Pad to full width.
    while bar.len() < TERM_COLS as usize {
        bar.push(' ');
    }
    bar.truncate(TERM_COLS as usize);
    console_write(&bar);
    term_reset_attr();

    // Top title bar.
    term_cursor_to(1, 1);
    term_reverse_video();
    term_clear_line();

    let title = if let Some(win) = session.active() {
        format!(
            " OurOS Screen — {} [{}/{}]",
            win.title,
            session.active_window + 1,
            session.windows.len()
        )
    } else {
        " OurOS Screen".to_string()
    };

    let mut title_bar = title;
    while title_bar.len() < TERM_COLS as usize {
        title_bar.push(' ');
    }
    title_bar.truncate(TERM_COLS as usize);
    console_write(&title_bar);
    term_reset_attr();
}

fn render_window(session: &Session) {
    let rows = TERM_ROWS;

    if let Some(win) = session.active() {
        let visible = win.visible_lines(rows);
        let content_rows = (rows as usize).saturating_sub(2);

        for i in 0..content_rows {
            term_cursor_to(i as u16 + 2, 1);
            term_clear_line();
            if i < visible.len() {
                let line = &visible[i];
                if line.len() > TERM_COLS as usize {
                    console_write(&line[..TERM_COLS as usize]);
                } else {
                    console_write(line);
                }
            }
        }

        // Show prompt on last content line.
        if let Some(win) = session.active() {
            let prompt_row = (2 + visible.len()).min(content_rows + 1) as u16 + 1;
            if prompt_row < rows - 1 {
                term_cursor_to(prompt_row, 1);
                term_clear_line();
                let prompt = format!("$ {}", win.input_buffer);
                console_write(&prompt);
            }
        }
    }
}

fn render_help() {
    term_clear();
    console_write("\x1b[1;33m");
    console_write("  OurOS Screen — Key Bindings\r\n");
    console_write("\x1b[0m\r\n");
    console_write("  All commands use Ctrl+A as prefix key.\r\n\r\n");
    console_write("  \x1b[1mWindow Management:\x1b[0m\r\n");
    console_write("    Ctrl+A c       Create new window\r\n");
    console_write("    Ctrl+A n       Next window\r\n");
    console_write("    Ctrl+A p       Previous window\r\n");
    console_write("    Ctrl+A 0-9     Switch to window N\r\n");
    console_write("    Ctrl+A k       Kill current window\r\n");
    console_write("    Ctrl+A A       Rename current window\r\n");
    console_write("    Ctrl+A \"       List all windows\r\n");
    console_write("    Ctrl+A w       Show window bar\r\n\r\n");
    console_write("  \x1b[1mSession:\x1b[0m\r\n");
    console_write("    Ctrl+A d       Detach session\r\n");
    console_write("    Ctrl+A [       Enter scrollback mode\r\n");
    console_write("    Ctrl+A ?       Show this help\r\n\r\n");
    console_write("  \x1b[1mScrollback Mode:\x1b[0m\r\n");
    console_write("    Up/Down/PgUp/PgDn   Scroll\r\n");
    console_write("    q/Escape            Exit scrollback\r\n\r\n");
    console_write("  Press any key to return...\r\n");
}

fn render_window_list(session: &Session) {
    term_clear();
    console_write("\x1b[1;33m  Window List\x1b[0m\r\n\r\n");
    console_write("  Num  Name                         Status\r\n");
    console_write("  ---  ----                         ------\r\n");

    for (i, win) in session.windows.iter().enumerate() {
        let marker = if i == session.active_window { "*" } else { " " };
        let status = if win.alive { "active" } else { "dead" };
        let line = format!("  {}{:<4} {:<28} {}\r\n", marker, win.id, win.title, status);
        console_write(&line);
    }

    console_write("\r\n  Press any key to return, or number to switch...\r\n");
}

fn full_render(session: &Session) {
    if session.showing_help {
        render_help();
        return;
    }
    if session.showing_window_list {
        render_window_list(session);
        return;
    }

    render_window(session);
    render_status_bar(session);
}

// ============================================================================
// Session persistence
// ============================================================================

fn session_dir() -> String {
    let uid = env::var("USER").unwrap_or_else(|_| "root".to_string());
    format!("/tmp/screen-{}", uid)
}

fn save_session_info(session: &Session) {
    let dir = session_dir();
    let _ = fs::create_dir_all(&dir);

    let info_path = format!("{}/{}.session", dir, session.name);
    let mut info = format!("name={}\n", session.name);
    info.push_str(&format!("pid={}\n", process::id()));
    info.push_str(&format!("windows={}\n", session.windows.len()));
    info.push_str(&format!("active={}\n", session.active_window));
    info.push_str("status=attached\n");
    let ts = clock_ns();
    info.push_str(&format!("created={}\n", ts));

    for (i, win) in session.windows.iter().enumerate() {
        info.push_str(&format!("window.{}.id={}\n", i, win.id));
        info.push_str(&format!("window.{}.title={}\n", i, win.title));
        info.push_str(&format!("window.{}.lines={}\n", i, win.scrollback.len()));
    }

    let _ = fs::write(&info_path, &info);
}

fn mark_session_detached(name: &str) {
    let dir = session_dir();
    let info_path = format!("{}/{}.session", dir, name);
    if let Ok(content) = fs::read_to_string(&info_path) {
        let updated = content.replace("status=attached", "status=detached");
        let _ = fs::write(&info_path, &updated);
    }
}

fn remove_session_info(name: &str) {
    let dir = session_dir();
    let info_path = format!("{}/{}.session", dir, name);
    let _ = fs::remove_file(&info_path);
}

fn list_sessions() {
    let dir = session_dir();

    if let Ok(entries) = fs::read_dir(&dir) {
        let mut found = false;
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && let Some(session_name) = name.strip_suffix(".session")
                && let Ok(content) = fs::read_to_string(entry.path())
            {
                let status = if content.contains("status=attached") {
                    "(Attached)"
                } else {
                    "(Detached)"
                };
                let pid = content
                    .lines()
                    .find(|l| l.starts_with("pid="))
                    .and_then(|l| l[4..].parse::<u32>().ok())
                    .unwrap_or(0);
                println!("\t{}.{}\t{}", pid, session_name, status);
                found = true;
            }
        }
        if !found {
            println!("No screen sessions found.");
        }
    } else {
        println!("No screen sessions found.");
    }
}

// ============================================================================
// Input handling
// ============================================================================

/// Read a character, possibly an escape sequence.
fn read_key() -> Option<Key> {
    let ch = console_read_char()?;

    if ch == 0x1b {
        // Escape sequence.
        let ch2 = console_read_char();
        match ch2 {
            Some(b'[') => {
                let ch3 = console_read_char();
                match ch3 {
                    Some(b'A') => return Some(Key::Up),
                    Some(b'B') => return Some(Key::Down),
                    Some(b'C') => return Some(Key::Right),
                    Some(b'D') => return Some(Key::Left),
                    Some(b'H') => return Some(Key::Home),
                    Some(b'F') => return Some(Key::End),
                    Some(b'5') => {
                        let _ = console_read_char(); // consume '~'
                        return Some(Key::PageUp);
                    }
                    Some(b'6') => {
                        let _ = console_read_char(); // consume '~'
                        return Some(Key::PageDown);
                    }
                    Some(b'3') => {
                        let _ = console_read_char(); // consume '~'
                        return Some(Key::Delete);
                    }
                    _ => return Some(Key::Escape),
                }
            }
            _ => return Some(Key::Escape),
        }
    }

    if ch == 1 {
        // Ctrl+A — our prefix key.
        return Some(Key::CtrlA);
    }

    if ch == 13 || ch == 10 {
        return Some(Key::Enter);
    }

    if ch == 127 || ch == 8 {
        return Some(Key::Backspace);
    }

    // Other Ctrl keys.
    if ch < 32 {
        return Some(Key::Ctrl(ch + b'@'));
    }

    Some(Key::Char(ch as char))
}

#[derive(Debug)]
enum Key {
    Char(char),
    Enter,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Escape,
    CtrlA,
    Ctrl(u8),
}

// ============================================================================
// Command processing
// ============================================================================

fn process_command(session: &mut Session, input: &str) {
    let input = input.trim();
    if input.is_empty() {
        return;
    }

    if let Some(win) = session.active_mut() {
        win.add_line(&format!("$ {}", input));
    }

    // Built-in commands.
    match input {
        "help" => {
            if let Some(win) = session.active_mut() {
                win.add_line(
                    "Available commands: help, clear, exit, windows, whoami, pwd, echo, date",
                );
            }
        }
        "clear" => {
            if let Some(win) = session.active_mut() {
                win.scrollback.clear();
            }
        }
        "exit" | "quit" => {
            let idx = session.active_window;
            session.kill_window(idx);
        }
        "windows" | "wins" => {
            let mut lines_to_add = Vec::new();
            for (i, win) in session.windows.iter().enumerate() {
                let marker = if i == session.active_window { "*" } else { " " };
                let line = format!(
                    "{}{}: {} ({} lines)",
                    marker,
                    win.id,
                    win.title,
                    win.scrollback.len()
                );
                lines_to_add.push(line);
            }
            if let Some(active) = session.active_mut() {
                for line in &lines_to_add {
                    active.add_line(line);
                }
            }
        }
        "whoami" => {
            let user = env::var("USER").unwrap_or_else(|_| "root".to_string());
            if let Some(win) = session.active_mut() {
                win.add_line(&user);
            }
        }
        "pwd" => {
            let cwd = env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "/".to_string());
            if let Some(win) = session.active_mut() {
                win.add_line(&cwd);
            }
        }
        "date" => {
            let ns = clock_ns();
            let secs = ns / 1_000_000_000;
            if let Some(win) = session.active_mut() {
                win.add_line(&format!("uptime: {}s", secs));
            }
        }
        _ => {
            if let Some(rest) = input.strip_prefix("echo ") {
                if let Some(win) = session.active_mut() {
                    win.add_line(rest);
                }
            } else {
                // Try to execute as external command.
                let cmd_path = format!("/bin/{}", input.split_whitespace().next().unwrap_or(""));
                if fs::metadata(&cmd_path).is_ok() {
                    if let Some(win) = session.active_mut() {
                        win.add_line(&format!("[executing: {}]", input));
                    }
                    // In a real implementation, we'd spawn a child process here.
                    let path_ptr = cmd_path.as_ptr() as u64;
                    let path_len = cmd_path.len() as u64;
                    let ret = unsafe { syscall3(SYS_PROCESS_SPAWN, path_ptr, path_len, 0) };
                    if ret < 0 {
                        if let Some(win) = session.active_mut() {
                            win.add_line(&format!("error: could not execute ({})", ret));
                        }
                    } else if let Some(win) = session.active_mut() {
                        win.add_line(&format!("[spawned pid {}]", ret));
                    }
                } else if let Some(win) = session.active_mut() {
                    win.add_line(&format!(
                        "command not found: {}",
                        input.split_whitespace().next().unwrap_or("")
                    ));
                }
            }
        }
    }
}

// ============================================================================
// Main event loop
// ============================================================================

fn run_session(session: &mut Session) {
    term_alt_screen_on();
    term_clear();
    term_set_scroll_region(2, TERM_ROWS - 2);

    let mut ctrl_a_pending = false;

    save_session_info(session);

    loop {
        if !session.running {
            break;
        }

        full_render(session);

        // Position cursor after prompt.
        if !session.showing_help
            && !session.showing_window_list
            && let Some(win) = session.active()
        {
            let visible_count = win.visible_lines(TERM_ROWS).len();
            let prompt_row = (2 + visible_count).min((TERM_ROWS as usize) - 2) as u16 + 1;
            let col = 3 + win.input_buffer.len() as u16;
            term_cursor_to(prompt_row, col);
        }

        let key = match read_key() {
            Some(k) => k,
            None => {
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }
        };

        // Handle Ctrl+A prefix mode.
        if ctrl_a_pending {
            ctrl_a_pending = false;
            match key {
                Key::Char('c') => {
                    let n = session.windows.len();
                    session.create_window(&format!("window-{}", n));
                    term_clear();
                }
                Key::Char('n') => {
                    session.next_window();
                    term_clear();
                }
                Key::Char('p') => {
                    session.prev_window();
                    term_clear();
                }
                Key::Char('k') => {
                    let idx = session.active_window;
                    session.kill_window(idx);
                    term_clear();
                }
                Key::Char('d') => {
                    // Detach.
                    mark_session_detached(&session.name);
                    session.running = false;
                    console_write("\r\n[detached from session]\r\n");
                }
                Key::Char('A') => {
                    session.renaming = true;
                    session.rename_buffer.clear();
                    // Show rename prompt.
                    term_cursor_to(TERM_ROWS, 1);
                    term_clear_line();
                    console_write("Rename window: ");
                }
                Key::Char('"') => {
                    session.showing_window_list = true;
                    term_clear();
                }
                Key::Char('w') => {
                    // Flash window bar — it's already shown, just redraw.
                    render_status_bar(session);
                }
                Key::Char('[') => {
                    session.scrollback_mode = true;
                    if let Some(win) = session.active_mut() {
                        win.scroll_offset = 0;
                    }
                }
                Key::Char('?') => {
                    session.showing_help = true;
                    term_clear();
                }
                Key::Char(c) if c.is_ascii_digit() => {
                    let idx = (c as u8 - b'0') as usize;
                    session.switch_to(idx);
                    term_clear();
                }
                Key::CtrlA => {
                    // Ctrl+A Ctrl+A sends a literal Ctrl+A.
                    if let Some(win) = session.active_mut() {
                        win.input_buffer.push('\x01');
                    }
                }
                _ => {
                    // Unknown Ctrl+A command — ignore.
                }
            }
            continue;
        }

        // Handle special modes.
        if session.showing_help {
            session.showing_help = false;
            term_clear();
            continue;
        }

        if session.showing_window_list {
            match key {
                Key::Char(c) if c.is_ascii_digit() => {
                    let idx = (c as u8 - b'0') as usize;
                    session.switch_to(idx);
                    session.showing_window_list = false;
                    term_clear();
                }
                _ => {
                    session.showing_window_list = false;
                    term_clear();
                }
            }
            continue;
        }

        if session.renaming {
            match key {
                Key::Enter => {
                    if !session.rename_buffer.is_empty() {
                        let new_name = session.rename_buffer.clone();
                        if let Some(win) = session.active_mut() {
                            win.title = new_name;
                        }
                    }
                    session.renaming = false;
                    session.rename_buffer.clear();
                    term_clear();
                }
                Key::Backspace => {
                    session.rename_buffer.pop();
                    term_cursor_to(TERM_ROWS, 1);
                    term_clear_line();
                    console_write(&format!("Rename window: {}", session.rename_buffer));
                }
                Key::Escape => {
                    session.renaming = false;
                    session.rename_buffer.clear();
                    term_clear();
                }
                Key::Char(c) => {
                    session.rename_buffer.push(c);
                    console_write(&format!("{}", c));
                }
                _ => {}
            }
            continue;
        }

        if session.scrollback_mode {
            match key {
                Key::Up => {
                    if let Some(win) = session.active_mut()
                        && win.scroll_offset < win.scrollback.len()
                    {
                        win.scroll_offset += 1;
                    }
                }
                Key::Down => {
                    if let Some(win) = session.active_mut()
                        && win.scroll_offset > 0
                    {
                        win.scroll_offset -= 1;
                    }
                }
                Key::PageUp => {
                    if let Some(win) = session.active_mut() {
                        let page = (TERM_ROWS as usize).saturating_sub(4);
                        win.scroll_offset = (win.scroll_offset + page).min(win.scrollback.len());
                    }
                }
                Key::PageDown => {
                    if let Some(win) = session.active_mut() {
                        let page = (TERM_ROWS as usize).saturating_sub(4);
                        win.scroll_offset = win.scroll_offset.saturating_sub(page);
                    }
                }
                Key::Char('q') | Key::Escape => {
                    session.scrollback_mode = false;
                    if let Some(win) = session.active_mut() {
                        win.scroll_offset = 0;
                    }
                    term_clear();
                }
                _ => {}
            }
            continue;
        }

        // Normal input mode.
        match key {
            Key::CtrlA => {
                ctrl_a_pending = true;
            }
            Key::Enter => {
                let input = if let Some(win) = session.active_mut() {
                    let buf = win.input_buffer.clone();
                    win.input_buffer.clear();
                    buf
                } else {
                    String::new()
                };
                process_command(session, &input);
            }
            Key::Backspace => {
                if let Some(win) = session.active_mut() {
                    win.input_buffer.pop();
                }
            }
            Key::Char(c) => {
                if let Some(win) = session.active_mut() {
                    win.input_buffer.push(c);
                }
            }
            Key::Ctrl(b'C') => {
                // Ctrl+C — clear input.
                if let Some(win) = session.active_mut() {
                    win.add_line(&format!("$ {}^C", win.input_buffer));
                    win.input_buffer.clear();
                }
            }
            Key::Ctrl(b'D') => {
                // Ctrl+D — EOF.
                if let Some(win) = session.active()
                    && win.input_buffer.is_empty()
                {
                    let idx = session.active_window;
                    session.kill_window(idx);
                    term_clear();
                }
            }
            Key::Ctrl(b'L') => {
                term_clear();
            }
            _ => {}
        }
    }

    term_alt_screen_off();
    term_set_scroll_region(1, TERM_ROWS);
}

// ============================================================================
// CLI
// ============================================================================

fn print_usage() {
    println!("OurOS Screen — Terminal Multiplexer v0.1.0");
    println!();
    println!("USAGE:");
    println!("  screen                 Start new session");
    println!("  screen -S name         Start named session");
    println!("  screen -ls             List sessions");
    println!("  screen -list           List sessions");
    println!("  screen -r [name]       Reattach to session");
    println!("  screen -d -r name      Detach other, attach here");
    println!("  screen -wipe           Clean dead sessions");
    println!("  screen -h              Show this help");
    println!();
    println!("KEY BINDINGS:");
    println!("  All commands use Ctrl+A as the prefix key.");
    println!("  Ctrl+A c    Create new window");
    println!("  Ctrl+A n/p  Next/previous window");
    println!("  Ctrl+A d    Detach session");
    println!("  Ctrl+A ?    Show help");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut session_name: Option<String> = None;
    let mut list_mode = false;
    let mut reattach: Option<String> = None;
    let mut detach_first = false;
    let mut wipe = false;

    let mut idx = 1;
    while idx < args.len() {
        match args[idx].as_str() {
            "-h" | "--help" | "help" => {
                print_usage();
                return;
            }
            "--version" => {
                println!("screen (OurOS) 0.1.0");
                return;
            }
            "-S" => {
                idx += 1;
                if idx < args.len() {
                    session_name = Some(args[idx].clone());
                }
            }
            "-ls" | "-list" | "list" => {
                list_mode = true;
            }
            "-r" => {
                idx += 1;
                if idx < args.len() {
                    reattach = Some(args[idx].clone());
                } else {
                    reattach = Some(String::new()); // Reattach to most recent.
                }
            }
            "-d" => {
                detach_first = true;
            }
            "-wipe" | "wipe" => {
                wipe = true;
            }
            _ => {
                // Treat as session name for -r.
                if reattach.is_some() && reattach.as_ref().is_some_and(|s| s.is_empty()) {
                    reattach = Some(args[idx].clone());
                }
            }
        }
        idx += 1;
    }

    if list_mode {
        list_sessions();
        return;
    }

    if wipe {
        let dir = session_dir();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str()
                    && name.ends_with(".session")
                    && let Ok(content) = fs::read_to_string(entry.path())
                    && content.contains("status=detached")
                {
                    let _ = fs::remove_file(entry.path());
                    println!("Removed: {}", name);
                }
            }
        }
        return;
    }

    if let Some(ref name) = reattach {
        if detach_first && !name.is_empty() {
            mark_session_detached(name);
        }
        // Try to reattach — for now, just create a new session with that name.
        let session_name = if name.is_empty() {
            // Find most recent.
            let dir = session_dir();
            let mut latest: Option<String> = None;
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    if let Some(fname) = entry.file_name().to_str()
                        && fname.ends_with(".session")
                    {
                        latest = Some(fname[..fname.len() - 8].to_string());
                    }
                }
            }
            match latest {
                Some(n) => n,
                None => {
                    eprintln!("No detached sessions found.");
                    process::exit(1);
                }
            }
        } else {
            name.clone()
        };

        println!("[reattaching to session '{}']", session_name);
        let mut session = Session::new(&session_name);
        run_session(&mut session);
        remove_session_info(&session_name);
        return;
    }

    // Start new session.
    let name = session_name.unwrap_or_else(|| format!("{}", process::id()));

    let mut session = Session::new(&name);
    run_session(&mut session);
    remove_session_info(&name);
}
