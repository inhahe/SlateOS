// OurOS logind — login/session manager daemon
//
// Multi-personality binary:
//   logind    — session/seat/user tracking daemon (systemd-logind compatible)
//   loginctl  — control login sessions, seats, and users
//
// Usage:
//   logind [--foreground] [--no-wall]
//   loginctl list-sessions
//   loginctl list-users
//   loginctl list-seats
//   loginctl show-session <ID>
//   loginctl show-user <UID>
//   loginctl show-seat <SEAT>
//   loginctl activate <ID>
//   loginctl lock-session <ID>
//   loginctl unlock-session <ID>
//   loginctl terminate-session <ID>
//   loginctl terminate-user <UID>
//   loginctl kill-session <ID> [--signal=SIGNAL]
//   loginctl kill-user <UID> [--signal=SIGNAL]
//   loginctl poweroff [--force]
//   loginctl reboot [--force]
//   loginctl suspend [--force]
//   loginctl hibernate [--force]

#![cfg_attr(not(test), no_main)]

use std::collections::HashMap;
use std::env;
use std::io::{self, Write};

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

/// Run directory for daemon state files.
const RUN_DIR: &str = "/run/systemd/logind";

/// Session state directory.
const SESSION_DIR: &str = "/run/systemd/sessions";

/// Seat state directory.
const SEAT_DIR: &str = "/run/systemd/seats";

/// User state directory.
const USER_DIR: &str = "/run/systemd/users";

/// Inhibitor lock directory.
const INHIBIT_DIR: &str = "/run/systemd/inhibit";

/// VT switching device path.
const VT_MASTER: &str = "/dev/tty0";

/// Maximum number of concurrent sessions.
const MAX_SESSIONS: usize = 8192;

/// Maximum number of inhibitor locks.
const MAX_INHIBITORS: usize = 1024;

/// Default idle timeout in seconds.
const DEFAULT_IDLE_TIMEOUT: u64 = 1800;

/// Idle action delay in seconds.
const DEFAULT_IDLE_ACTION_DELAY: u64 = 30;

/// Maximum session ID value before wrapping.
const MAX_SESSION_ID: u64 = 999_999;

// ============================================================================
// Syscall numbers (OurOS ABI)
// ============================================================================

const SYS_CHANNEL_OPEN: u64 = 200;
const SYS_CHANNEL_SEND: u64 = 201;
const SYS_CHANNEL_RECV: u64 = 202;
const SYS_CHANNEL_CLOSE: u64 = 203;
const SYS_SHUTDOWN: u64 = 90;
const SYS_REBOOT: u64 = 91;

/// ACPI S3 - suspend to RAM.
const ACPI_S3_SUSPEND: u64 = 3;
/// ACPI S4 - hibernate (suspend to disk).
const ACPI_S4_HIBERNATE: u64 = 4;

// ============================================================================
// Syscall interface
// ============================================================================

#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures arguments are valid for the given syscall number.
    // The `syscall` instruction is the defined kernel entry point on x86-64.
    // rcx and r11 are clobbered per the hardware specification.
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

#[cfg(not(target_arch = "x86_64"))]
unsafe fn syscall3(_nr: u64, _a1: u64, _a2: u64, _a3: u64) -> i64 {
    -1
}

// ============================================================================
// Session types and state
// ============================================================================

/// Session type — how the user is connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionType {
    /// Text console / virtual terminal.
    Tty,
    /// X11 graphical session.
    X11,
    /// Wayland graphical session.
    Wayland,
    /// Unspecified / unknown session type.
    Unspecified,
}

impl SessionType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Tty => "tty",
            Self::X11 => "x11",
            Self::Wayland => "wayland",
            Self::Unspecified => "unspecified",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "tty" => Self::Tty,
            "x11" => Self::X11,
            "wayland" => Self::Wayland,
            _ => Self::Unspecified,
        }
    }
}

/// Session class — the purpose of the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionClass {
    /// Normal user session.
    User,
    /// Background/system session with no human at the terminal.
    Greeter,
    /// Lock screen session.
    LockScreen,
}

impl SessionClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Greeter => "greeter",
            Self::LockScreen => "lock-screen",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "greeter" => Self::Greeter,
            "lock-screen" => Self::LockScreen,
            _ => Self::User,
        }
    }
}

/// Session state — lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionState {
    /// Session is being set up (PAM, cgroup creation, etc.).
    Opening,
    /// Session is fully active.
    Online,
    /// Session is active on the foreground VT / seat.
    Active,
    /// Session is in the background but still alive.
    Background,
    /// Session is being torn down.
    Closing,
}

impl SessionState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Opening => "opening",
            Self::Online => "online",
            Self::Active => "active",
            Self::Background => "background",
            Self::Closing => "closing",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "opening" => Self::Opening,
            "online" => Self::Online,
            "active" => Self::Active,
            "background" => Self::Background,
            "closing" => Self::Closing,
            _ => Self::Online,
        }
    }
}

/// A login session.
#[derive(Debug, Clone)]
struct Session {
    /// Unique session identifier (e.g. "1", "2", "c1").
    id: String,
    /// UID of the user owning this session.
    uid: u32,
    /// Username.
    user: String,
    /// Seat this session is attached to (empty if none).
    seat: String,
    /// Virtual terminal number (0 if none).
    vt_nr: u32,
    /// Session type.
    session_type: SessionType,
    /// Session class.
    class: SessionClass,
    /// Session state.
    state: SessionState,
    /// Whether the session screen is locked.
    locked: bool,
    /// Whether this session is marked as idle.
    idle: bool,
    /// Timestamp (seconds since epoch) when session was created.
    created_at: u64,
    /// Timestamp when session last became idle (0 if not idle).
    idle_since: u64,
    /// Leader PID of the session.
    leader_pid: u32,
    /// TTY or display identifier (e.g. "/dev/tty1", ":0").
    tty: String,
    /// Remote hostname (empty if local).
    remote_host: String,
    /// Whether the session is remote.
    remote: bool,
    /// Service that created this session (e.g. "sshd", "login").
    service: String,
    /// Desktop environment identifier (e.g. "gnome", "kde").
    desktop: String,
    /// Scope unit name for cgroup management.
    scope: String,
}

impl Session {
    fn new(id: &str, uid: u32, user: &str) -> Self {
        Self {
            id: id.to_string(),
            uid,
            user: user.to_string(),
            seat: String::new(),
            vt_nr: 0,
            session_type: SessionType::Unspecified,
            class: SessionClass::User,
            state: SessionState::Online,
            locked: false,
            idle: false,
            created_at: 0,
            idle_since: 0,
            leader_pid: 0,
            tty: String::new(),
            remote_host: String::new(),
            remote: false,
            service: String::new(),
            desktop: String::new(),
            scope: String::new(),
        }
    }

    /// Format as a single-line summary for list-sessions output.
    fn format_list_line(&self) -> String {
        format!(
            "{:<8} {:<6} {:<16} {:<12} {}",
            self.id,
            self.uid,
            self.user,
            self.seat_display(),
            self.tty_display(),
        )
    }

    fn seat_display(&self) -> &str {
        if self.seat.is_empty() {
            "-"
        } else {
            &self.seat
        }
    }

    fn tty_display(&self) -> &str {
        if self.tty.is_empty() {
            "-"
        } else {
            &self.tty
        }
    }

    /// Format detailed properties for show-session output.
    fn format_properties(&self) -> String {
        let mut out = String::with_capacity(512);
        out.push_str(&format!("        Id={}\n", self.id));
        out.push_str(&format!("      User={} ({})\n", self.user, self.uid));
        out.push_str(&format!("      Seat={}\n", self.seat_display()));
        if self.vt_nr > 0 {
            out.push_str(&format!("     VTNr={}\n", self.vt_nr));
        }
        out.push_str(&format!("      Type={}\n", self.session_type.as_str()));
        out.push_str(&format!("     Class={}\n", self.class.as_str()));
        out.push_str(&format!("     State={}\n", self.state.as_str()));
        out.push_str(&format!("    Locked={}\n", if self.locked { "yes" } else { "no" }));
        out.push_str(&format!("      Idle={}\n", if self.idle { "yes" } else { "no" }));
        if self.idle && self.idle_since > 0 {
            out.push_str(&format!(" IdleSince={}\n", self.idle_since));
        }
        out.push_str(&format!("    Leader={}\n", self.leader_pid));
        out.push_str(&format!("       TTY={}\n", self.tty_display()));
        out.push_str(&format!("    Remote={}\n", if self.remote { "yes" } else { "no" }));
        if self.remote && !self.remote_host.is_empty() {
            out.push_str(&format!("RemoteHost={}\n", self.remote_host));
        }
        if !self.service.is_empty() {
            out.push_str(&format!("   Service={}\n", self.service));
        }
        if !self.desktop.is_empty() {
            out.push_str(&format!("   Desktop={}\n", self.desktop));
        }
        if !self.scope.is_empty() {
            out.push_str(&format!("     Scope={}\n", self.scope));
        }
        out.push_str(&format!(" CreatedAt={}\n", self.created_at));
        out
    }
}

/// A tracked user with one or more sessions.
#[derive(Debug, Clone)]
struct User {
    /// User ID.
    uid: u32,
    /// Username.
    name: String,
    /// State of the user: "active", "online", "lingering", "closing".
    state: String,
    /// Session IDs belonging to this user.
    sessions: Vec<String>,
    /// Timestamp when user first logged in (seconds since epoch).
    logged_in_since: u64,
    /// Whether linger is enabled for this user.
    linger: bool,
    /// Slice unit name for resource control.
    slice: String,
}

impl User {
    fn new(uid: u32, name: &str) -> Self {
        Self {
            uid,
            name: name.to_string(),
            state: "online".to_string(),
            sessions: Vec::new(),
            logged_in_since: 0,
            linger: false,
            slice: format!("user-{uid}.slice"),
        }
    }

    /// Format as a single-line summary for list-users output.
    fn format_list_line(&self) -> String {
        format!(
            "{:<8} {:<16} {}",
            self.uid,
            self.name,
            self.state,
        )
    }

    /// Format detailed properties for show-user output.
    fn format_properties(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str(&format!("       UID={}\n", self.uid));
        out.push_str(&format!("      Name={}\n", self.name));
        out.push_str(&format!("     State={}\n", self.state));
        out.push_str(&format!(
            "  Sessions={}\n",
            if self.sessions.is_empty() {
                "-".to_string()
            } else {
                self.sessions.join(" ")
            }
        ));
        out.push_str(&format!("    Linger={}\n", if self.linger { "yes" } else { "no" }));
        out.push_str(&format!("     Slice={}\n", self.slice));
        if self.logged_in_since > 0 {
            out.push_str(&format!("     Since={}\n", self.logged_in_since));
        }
        out
    }
}

/// A physical seat (display/input grouping).
#[derive(Debug, Clone)]
struct Seat {
    /// Seat identifier (e.g. "seat0").
    id: String,
    /// Session IDs attached to this seat.
    sessions: Vec<String>,
    /// Which session is currently active on this seat.
    active_session: String,
    /// Whether this seat can do graphical output.
    can_graphical: bool,
    /// Whether this seat can do multi-session (VT switching).
    can_multi_session: bool,
    /// Whether this seat supports TTY sessions.
    can_tty: bool,
}

impl Seat {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            sessions: Vec::new(),
            active_session: String::new(),
            can_graphical: true,
            can_multi_session: true,
            can_tty: true,
        }
    }

    /// Format as a single-line summary for list-seats output.
    fn format_list_line(&self) -> String {
        self.id.clone()
    }

    /// Format detailed properties for show-seat output.
    fn format_properties(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str(&format!("          Id={}\n", self.id));
        out.push_str(&format!(
            "    Sessions={}\n",
            if self.sessions.is_empty() {
                "-".to_string()
            } else {
                self.sessions.join(" ")
            }
        ));
        out.push_str(&format!(
            "ActiveSessn={}\n",
            if self.active_session.is_empty() {
                "-"
            } else {
                &self.active_session
            }
        ));
        out.push_str(&format!(
            " CanGraphical={}\n",
            if self.can_graphical { "yes" } else { "no" }
        ));
        out.push_str(&format!(
            "CanMultiSess={}\n",
            if self.can_multi_session { "yes" } else { "no" }
        ));
        out.push_str(&format!(
            "      CanTTY={}\n",
            if self.can_tty { "yes" } else { "no" }
        ));
        out
    }
}

/// What kind of action an inhibitor lock prevents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InhibitWhat {
    Shutdown,
    Sleep,
    Idle,
    HandlePowerKey,
    HandleSuspendKey,
    HandleHibernateKey,
    HandleLidSwitch,
}

impl InhibitWhat {
    fn as_str(self) -> &'static str {
        match self {
            Self::Shutdown => "shutdown",
            Self::Sleep => "sleep",
            Self::Idle => "idle",
            Self::HandlePowerKey => "handle-power-key",
            Self::HandleSuspendKey => "handle-suspend-key",
            Self::HandleHibernateKey => "handle-hibernate-key",
            Self::HandleLidSwitch => "handle-lid-switch",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "shutdown" => Some(Self::Shutdown),
            "sleep" => Some(Self::Sleep),
            "idle" => Some(Self::Idle),
            "handle-power-key" => Some(Self::HandlePowerKey),
            "handle-suspend-key" => Some(Self::HandleSuspendKey),
            "handle-hibernate-key" => Some(Self::HandleHibernateKey),
            "handle-lid-switch" => Some(Self::HandleLidSwitch),
            _ => None,
        }
    }
}

/// Inhibitor lock mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InhibitMode {
    /// Block the operation entirely until the lock is released.
    Block,
    /// Delay the operation for a grace period.
    Delay,
}

impl InhibitMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Delay => "delay",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "delay" => Self::Delay,
            _ => Self::Block,
        }
    }
}

/// An inhibitor lock held by a process.
#[derive(Debug, Clone)]
struct Inhibitor {
    /// What action is inhibited.
    what: InhibitWhat,
    /// Who created this lock (application name).
    who: String,
    /// Human-readable reason.
    why: String,
    /// Lock mode.
    mode: InhibitMode,
    /// UID of the lock holder.
    uid: u32,
    /// PID of the lock holder.
    pid: u32,
}

impl Inhibitor {
    fn format_line(&self) -> String {
        format!(
            "{:<20} {:<6} {:<6} {:<8} {}",
            self.what.as_str(),
            self.uid,
            self.pid,
            self.mode.as_str(),
            self.why,
        )
    }
}

/// Power action request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PowerAction {
    PowerOff,
    Reboot,
    Suspend,
    Hibernate,
}

impl PowerAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::PowerOff => "poweroff",
            Self::Reboot => "reboot",
            Self::Suspend => "suspend",
            Self::Hibernate => "hibernate",
        }
    }
}

/// Daemon configuration.
#[derive(Debug, Clone)]
struct DaemonConfig {
    /// How many seconds of inactivity before the system is considered idle.
    idle_timeout: u64,
    /// Delay in seconds after idle before taking idle action.
    idle_action_delay: u64,
    /// What action to take on idle ("ignore", "poweroff", "suspend", etc.).
    idle_action: String,
    /// What to do when the power key is pressed.
    handle_power_key: String,
    /// What to do when the suspend key is pressed.
    handle_suspend_key: String,
    /// What to do when the hibernate key is pressed.
    handle_hibernate_key: String,
    /// What to do when the lid is closed.
    handle_lid_switch: String,
    /// What to do when the lid is closed while docked.
    handle_lid_switch_docked: String,
    /// Whether to allow poweroff from loginctl even without active session.
    allow_poweroff: bool,
    /// Whether to allow reboot from loginctl.
    allow_reboot: bool,
    /// Whether to allow suspend from loginctl.
    allow_suspend: bool,
    /// Whether to allow hibernate from loginctl.
    allow_hibernate: bool,
    /// Kill user processes when session ends.
    kill_user_processes: bool,
    /// Delay in seconds for inhibitor delay locks.
    inhibit_delay_max: u64,
    /// Maximum number of sessions to track.
    max_sessions: usize,
    /// Enable wall messages on shutdown.
    wall_message: bool,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
            idle_action_delay: DEFAULT_IDLE_ACTION_DELAY,
            idle_action: "ignore".to_string(),
            handle_power_key: "poweroff".to_string(),
            handle_suspend_key: "suspend".to_string(),
            handle_hibernate_key: "hibernate".to_string(),
            handle_lid_switch: "suspend".to_string(),
            handle_lid_switch_docked: "ignore".to_string(),
            allow_poweroff: true,
            allow_reboot: true,
            allow_suspend: true,
            allow_hibernate: true,
            kill_user_processes: false,
            inhibit_delay_max: 5,
            max_sessions: MAX_SESSIONS,
            wall_message: true,
        }
    }
}

// ============================================================================
// Daemon state
// ============================================================================

/// The complete state of the logind daemon.
#[derive(Debug)]
struct Daemon {
    /// Active sessions indexed by session ID.
    sessions: HashMap<String, Session>,
    /// Tracked users indexed by UID.
    users: HashMap<u32, User>,
    /// Seats indexed by seat ID.
    seats: HashMap<String, Seat>,
    /// Active inhibitor locks.
    inhibitors: Vec<Inhibitor>,
    /// Next session ID to allocate.
    next_session_id: u64,
    /// Whether the system is considered idle.
    system_idle: bool,
    /// Timestamp when system became idle (0 if not idle).
    idle_since: u64,
    /// Configuration.
    config: DaemonConfig,
    /// Whether the daemon is running.
    running: bool,
}

impl Daemon {
    fn new(config: DaemonConfig) -> Self {
        let mut seats = HashMap::new();
        // Always create seat0 as the default local seat.
        seats.insert("seat0".to_string(), Seat::new("seat0"));

        Self {
            sessions: HashMap::new(),
            users: HashMap::new(),
            seats,
            inhibitors: Vec::new(),
            next_session_id: 1,
            system_idle: false,
            idle_since: 0,
            config,
            running: true,
        }
    }

    /// Allocate a new unique session ID.
    fn allocate_session_id(&mut self) -> String {
        let id = self.next_session_id;
        self.next_session_id = if id >= MAX_SESSION_ID { 1 } else { id + 1 };
        id.to_string()
    }

    /// Create a new session and register it with the daemon.
    fn create_session(
        &mut self,
        uid: u32,
        user: &str,
        session_type: SessionType,
        class: SessionClass,
        seat_id: &str,
        vt_nr: u32,
        tty: &str,
        remote: bool,
        remote_host: &str,
        service: &str,
        desktop: &str,
        leader_pid: u32,
    ) -> Result<String, &'static str> {
        if self.sessions.len() >= self.config.max_sessions {
            return Err("maximum session limit reached");
        }

        let id = self.allocate_session_id();
        let mut session = Session::new(&id, uid, user);
        session.session_type = session_type;
        session.class = class;
        session.vt_nr = vt_nr;
        session.tty = tty.to_string();
        session.remote = remote;
        session.remote_host = remote_host.to_string();
        session.service = service.to_string();
        session.desktop = desktop.to_string();
        session.leader_pid = leader_pid;
        session.state = SessionState::Active;
        session.scope = format!("session-{id}.scope");

        // Attach to seat if specified.
        if !seat_id.is_empty() {
            session.seat = seat_id.to_string();
            if let Some(seat) = self.seats.get_mut(seat_id) {
                seat.sessions.push(id.clone());
                // First session on the seat becomes active.
                if seat.active_session.is_empty() {
                    seat.active_session = id.clone();
                }
            }
        }

        // Track user.
        let user_entry = self.users.entry(uid).or_insert_with(|| User::new(uid, user));
        user_entry.sessions.push(id.clone());
        user_entry.state = "active".to_string();

        self.sessions.insert(id.clone(), session);
        Ok(id)
    }

    /// Terminate a session by ID.
    fn terminate_session(&mut self, session_id: &str) -> Result<(), &'static str> {
        let session = self.sessions.get_mut(session_id).ok_or("session not found")?;
        session.state = SessionState::Closing;

        let uid = session.uid;
        let seat_id = session.seat.clone();

        // Remove from seat.
        if !seat_id.is_empty() {
            if let Some(seat) = self.seats.get_mut(&seat_id) {
                seat.sessions.retain(|s| s != session_id);
                if seat.active_session == session_id {
                    seat.active_session = seat.sessions.first().cloned().unwrap_or_default();
                }
            }
        }

        // Remove from user tracking.
        if let Some(user) = self.users.get_mut(&uid) {
            user.sessions.retain(|s| s != session_id);
            if user.sessions.is_empty() {
                if !user.linger {
                    user.state = "closing".to_string();
                } else {
                    user.state = "lingering".to_string();
                }
            }
        }

        self.sessions.remove(session_id);
        Ok(())
    }

    /// Terminate all sessions belonging to a user.
    fn terminate_user(&mut self, uid: u32) -> Result<(), &'static str> {
        let session_ids: Vec<String> = self
            .sessions
            .values()
            .filter(|s| s.uid == uid)
            .map(|s| s.id.clone())
            .collect();

        if session_ids.is_empty() {
            return Err("user has no sessions");
        }

        for sid in &session_ids {
            // Ignore errors from individual session termination — best effort.
            let _ = self.terminate_session(sid);
        }

        self.users.remove(&uid);
        Ok(())
    }

    /// Activate a session (make it the foreground session on its seat).
    fn activate_session(&mut self, session_id: &str) -> Result<(), &'static str> {
        let session = self.sessions.get(session_id).ok_or("session not found")?;
        let seat_id = session.seat.clone();

        if seat_id.is_empty() {
            return Err("session is not attached to a seat");
        }

        // Deactivate the currently active session on this seat.
        let seat = self.seats.get(&seat_id).ok_or("seat not found")?;
        let prev_active = seat.active_session.clone();

        if !prev_active.is_empty() && prev_active != session_id {
            if let Some(prev) = self.sessions.get_mut(&prev_active) {
                if prev.state == SessionState::Active {
                    prev.state = SessionState::Background;
                }
            }
        }

        // Activate the requested session.
        if let Some(sess) = self.sessions.get_mut(session_id) {
            sess.state = SessionState::Active;
        }
        if let Some(seat) = self.seats.get_mut(&seat_id) {
            seat.active_session = session_id.to_string();
        }
        Ok(())
    }

    /// Lock a session's screen.
    fn lock_session(&mut self, session_id: &str) -> Result<(), &'static str> {
        let session = self.sessions.get_mut(session_id).ok_or("session not found")?;
        session.locked = true;
        Ok(())
    }

    /// Unlock a session's screen.
    fn unlock_session(&mut self, session_id: &str) -> Result<(), &'static str> {
        let session = self.sessions.get_mut(session_id).ok_or("session not found")?;
        session.locked = false;
        Ok(())
    }

    /// Mark a session as idle.
    fn set_session_idle(&mut self, session_id: &str, idle: bool, timestamp: u64) -> Result<(), &'static str> {
        let session = self.sessions.get_mut(session_id).ok_or("session not found")?;
        session.idle = idle;
        session.idle_since = if idle { timestamp } else { 0 };
        Ok(())
    }

    /// Add an inhibitor lock.
    fn add_inhibitor(
        &mut self,
        what: InhibitWhat,
        who: &str,
        why: &str,
        mode: InhibitMode,
        uid: u32,
        pid: u32,
    ) -> Result<(), &'static str> {
        if self.inhibitors.len() >= MAX_INHIBITORS {
            return Err("maximum inhibitor limit reached");
        }
        self.inhibitors.push(Inhibitor {
            what,
            who: who.to_string(),
            why: why.to_string(),
            mode,
            uid,
            pid,
        });
        Ok(())
    }

    /// Remove inhibitor locks held by a given PID.
    fn remove_inhibitors_by_pid(&mut self, pid: u32) -> usize {
        let before = self.inhibitors.len();
        self.inhibitors.retain(|i| i.pid != pid);
        before - self.inhibitors.len()
    }

    /// Check whether a given action is inhibited.
    fn is_inhibited(&self, what: InhibitWhat, mode: InhibitMode) -> bool {
        self.inhibitors.iter().any(|i| i.what == what && i.mode == mode)
    }

    /// Check whether a given action is inhibited (any mode).
    fn is_inhibited_any(&self, what: InhibitWhat) -> bool {
        self.inhibitors.iter().any(|i| i.what == what)
    }

    /// Update the system idle state based on all sessions.
    fn update_idle_state(&mut self, now: u64) {
        let all_idle = self.sessions.values().all(|s| s.idle);
        if all_idle && !self.sessions.is_empty() {
            if !self.system_idle {
                self.system_idle = true;
                self.idle_since = now;
            }
        } else {
            self.system_idle = false;
            self.idle_since = 0;
        }
    }

    /// Switch VTs on a seat (for multi-session seats).
    fn switch_vt(&mut self, seat_id: &str, vt_nr: u32) -> Result<(), &'static str> {
        let seat = self.seats.get(seat_id).ok_or("seat not found")?;
        if !seat.can_multi_session {
            return Err("seat does not support multi-session");
        }

        // Find the session on this seat with the matching VT.
        let target_session = self
            .sessions
            .values()
            .find(|s| s.seat == seat_id && s.vt_nr == vt_nr)
            .map(|s| s.id.clone());

        if let Some(sid) = target_session {
            self.activate_session(&sid)
        } else {
            Err("no session on that VT")
        }
    }

    /// Handle a power action request, checking inhibitors first.
    fn request_power_action(&self, action: PowerAction, force: bool) -> PowerActionResult {
        let inhibit_what = match action {
            PowerAction::PowerOff | PowerAction::Reboot => InhibitWhat::Shutdown,
            PowerAction::Suspend | PowerAction::Hibernate => InhibitWhat::Sleep,
        };

        if !force && self.is_inhibited(inhibit_what, InhibitMode::Block) {
            return PowerActionResult::Inhibited;
        }

        let allowed = match action {
            PowerAction::PowerOff => self.config.allow_poweroff,
            PowerAction::Reboot => self.config.allow_reboot,
            PowerAction::Suspend => self.config.allow_suspend,
            PowerAction::Hibernate => self.config.allow_hibernate,
        };

        if !allowed {
            return PowerActionResult::Denied;
        }

        PowerActionResult::Allowed
    }

    /// Create a new seat.
    fn create_seat(&mut self, id: &str) -> Result<(), &'static str> {
        if self.seats.contains_key(id) {
            return Err("seat already exists");
        }
        self.seats.insert(id.to_string(), Seat::new(id));
        Ok(())
    }

    /// Remove a seat and detach all sessions from it.
    fn remove_seat(&mut self, seat_id: &str) -> Result<(), &'static str> {
        if seat_id == "seat0" {
            return Err("cannot remove seat0");
        }
        let seat = self.seats.remove(seat_id).ok_or("seat not found")?;

        // Detach sessions from the removed seat.
        for sid in &seat.sessions {
            if let Some(session) = self.sessions.get_mut(sid) {
                session.seat.clear();
                session.vt_nr = 0;
            }
        }
        Ok(())
    }

    /// Send a signal to all processes in a session.
    fn kill_session(&self, session_id: &str, _signal: i32) -> Result<u32, &'static str> {
        let session = self.sessions.get(session_id).ok_or("session not found")?;
        // In a real implementation this would walk the session's cgroup and
        // send the signal to every process. We return the leader PID to
        // indicate the target.
        Ok(session.leader_pid)
    }

    /// Send a signal to all processes belonging to a user.
    fn kill_user(&self, uid: u32, _signal: i32) -> Result<Vec<u32>, &'static str> {
        let pids: Vec<u32> = self
            .sessions
            .values()
            .filter(|s| s.uid == uid)
            .map(|s| s.leader_pid)
            .collect();
        if pids.is_empty() {
            return Err("user has no sessions");
        }
        Ok(pids)
    }
}

/// Result of a power action request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PowerActionResult {
    Allowed,
    Inhibited,
    Denied,
}

// ============================================================================
// Personality detection
// ============================================================================

/// Determine which personality to run based on argv[0] or subcommand.
fn detect_personality(args: &[String]) -> &'static str {
    if let Some(arg0) = args.first() {
        let basename = arg0.rsplit('/').next().unwrap_or(arg0);
        let basename = basename.rsplit('\\').next().unwrap_or(basename);
        let basename = basename.strip_suffix(".exe").unwrap_or(basename);

        // Exact match on "loginctl" in the binary name takes priority.
        if basename.contains("loginctl") {
            return "loginctl";
        }

        // If the binary name contains "logind" (but not "loginctl"), check
        // for a subcommand before falling through to the daemon default.
        if basename.contains("logind") {
            if let Some(sub) = args.get(1) {
                match sub.as_str() {
                    "ctl" | "loginctl" => return "loginctl",
                    "daemon" => return "logind",
                    _ => {}
                }
            }
            return "logind";
        }
    }

    // Check for subcommand form when argv[0] is unrecognized.
    if let Some(sub) = args.get(1) {
        match sub.as_str() {
            "ctl" | "loginctl" => return "loginctl",
            "daemon" => return "logind",
            _ => {}
        }
    }

    // Default to daemon
    "logind"
}

// ============================================================================
// Configuration parsing
// ============================================================================

/// Parse a logind.conf style configuration from text content.
fn parse_config(content: &str) -> DaemonConfig {
    let mut config = DaemonConfig::default();
    let mut in_login_section = false;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            in_login_section = line == "[Login]";
            continue;
        }
        if !in_login_section {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "IdleAction" => config.idle_action = value.to_string(),
                "IdleActionSec" => {
                    if let Ok(v) = value.parse() {
                        config.idle_timeout = v;
                    }
                }
                "HandlePowerKey" => config.handle_power_key = value.to_string(),
                "HandleSuspendKey" => config.handle_suspend_key = value.to_string(),
                "HandleHibernateKey" => config.handle_hibernate_key = value.to_string(),
                "HandleLidSwitch" => config.handle_lid_switch = value.to_string(),
                "HandleLidSwitchDocked" => config.handle_lid_switch_docked = value.to_string(),
                "KillUserProcesses" => {
                    config.kill_user_processes = value == "yes" || value == "true" || value == "1";
                }
                "InhibitDelayMaxSec" => {
                    if let Ok(v) = value.parse() {
                        config.inhibit_delay_max = v;
                    }
                }
                _ => {}
            }
        }
    }

    config
}

// ============================================================================
// Daemon entry point
// ============================================================================

/// Daemon command-line flags.
struct DaemonArgs {
    foreground: bool,
    no_wall: bool,
}

fn parse_daemon_args(args: &[String]) -> DaemonArgs {
    let mut result = DaemonArgs {
        foreground: false,
        no_wall: false,
    };

    for arg in args {
        match arg.as_str() {
            "--foreground" | "-f" => result.foreground = true,
            "--no-wall" => result.no_wall = true,
            "--help" | "-h" => {
                let _ = writeln!(
                    io::stdout(),
                    "Usage: logind [OPTIONS]\n\n\
                     Options:\n  \
                       --foreground, -f   Run in the foreground\n  \
                       --no-wall          Do not send wall messages\n  \
                       --help, -h         Show this help\n  \
                       --version          Show version"
                );
            }
            "--version" => {
                let _ = writeln!(io::stdout(), "logind {VERSION}");
            }
            _ => {}
        }
    }

    result
}

/// Run the logind daemon.
fn run_daemon(args: &[String]) -> i32 {
    let daemon_args = parse_daemon_args(args);

    // Try to load configuration from /etc/systemd/logind.conf.
    let config = match std::fs::read_to_string("/etc/systemd/logind.conf") {
        Ok(content) => parse_config(&content),
        Err(_) => DaemonConfig::default(),
    };

    let mut daemon = Daemon::new(config);

    if daemon_args.no_wall {
        daemon.config.wall_message = false;
    }

    let _ = writeln!(io::stderr(), "logind: starting session manager (v{VERSION})");

    if !daemon_args.foreground {
        let _ = writeln!(io::stderr(), "logind: would daemonize (not implemented in stub)");
    }

    // Create runtime directories (best-effort).
    for dir in &[RUN_DIR, SESSION_DIR, SEAT_DIR, USER_DIR, INHIBIT_DIR] {
        let _ = std::fs::create_dir_all(dir);
    }

    let _ = writeln!(
        io::stderr(),
        "logind: ready (max_sessions={}, idle_timeout={}s)",
        daemon.config.max_sessions,
        daemon.config.idle_timeout,
    );

    // In a real implementation, this would enter an event loop listening
    // on D-Bus for session/seat/power requests. For now we just indicate
    // readiness and return.
    daemon.running = false;

    let _ = writeln!(io::stderr(), "logind: shutting down");
    0
}

// ============================================================================
// loginctl entry point
// ============================================================================

/// Parse a signal name or number.
fn parse_signal(s: &str) -> Option<i32> {
    // Strip --signal= prefix if present.
    let s = s.strip_prefix("--signal=").unwrap_or(s);
    let s = s.strip_prefix("--kill-signal=").unwrap_or(s);

    // Try numeric first.
    if let Ok(n) = s.parse::<i32>() {
        return Some(n);
    }

    // Named signals (POSIX subset).
    let name = if let Some(stripped) = s.strip_prefix("SIG") {
        stripped
    } else {
        s
    };

    match name.to_ascii_uppercase().as_str() {
        "HUP" => Some(1),
        "INT" => Some(2),
        "QUIT" => Some(3),
        "ILL" => Some(4),
        "ABRT" => Some(6),
        "FPE" => Some(8),
        "KILL" => Some(9),
        "SEGV" => Some(11),
        "PIPE" => Some(13),
        "ALRM" => Some(14),
        "TERM" => Some(15),
        "USR1" => Some(10),
        "USR2" => Some(12),
        "STOP" => Some(19),
        "CONT" => Some(18),
        _ => None,
    }
}

/// loginctl subcommand parsed from CLI.
#[derive(Debug, PartialEq)]
enum LoginctlCommand {
    ListSessions,
    ListUsers,
    ListSeats,
    ShowSession(String),
    ShowUser(String),
    ShowSeat(String),
    Activate(String),
    LockSession(String),
    UnlockSession(String),
    TerminateSession(String),
    TerminateUser(String),
    KillSession(String, i32),
    KillUser(String, i32),
    PowerOff(bool),
    Reboot(bool),
    Suspend(bool),
    Hibernate(bool),
    Help,
    Version,
}

fn parse_loginctl_args(args: &[String]) -> LoginctlCommand {
    if args.is_empty() {
        return LoginctlCommand::Help;
    }

    let mut signal = 15_i32; // Default to SIGTERM.
    let mut force = false;
    let mut positional: Vec<&str> = Vec::new();

    for arg in args {
        if arg.starts_with("--signal=") || arg.starts_with("--kill-signal=") {
            if let Some(s) = parse_signal(arg) {
                signal = s;
            }
        } else if arg == "--force" || arg == "-f" {
            force = true;
        } else if arg == "--help" || arg == "-h" {
            return LoginctlCommand::Help;
        } else if arg == "--version" {
            return LoginctlCommand::Version;
        } else if !arg.starts_with('-') {
            positional.push(arg);
        }
    }

    let cmd = if let Some(&first) = positional.first() {
        first
    } else {
        return LoginctlCommand::Help;
    };

    let arg1 = positional.get(1).copied().unwrap_or("").to_string();

    match cmd {
        "list-sessions" => LoginctlCommand::ListSessions,
        "list-users" => LoginctlCommand::ListUsers,
        "list-seats" => LoginctlCommand::ListSeats,
        "show-session" => LoginctlCommand::ShowSession(arg1),
        "show-user" => LoginctlCommand::ShowUser(arg1),
        "show-seat" => LoginctlCommand::ShowSeat(arg1),
        "activate" => LoginctlCommand::Activate(arg1),
        "lock-session" => LoginctlCommand::LockSession(arg1),
        "unlock-session" => LoginctlCommand::UnlockSession(arg1),
        "terminate-session" => LoginctlCommand::TerminateSession(arg1),
        "terminate-user" => LoginctlCommand::TerminateUser(arg1),
        "kill-session" => LoginctlCommand::KillSession(arg1, signal),
        "kill-user" => LoginctlCommand::KillUser(arg1, signal),
        "poweroff" => LoginctlCommand::PowerOff(force),
        "reboot" => LoginctlCommand::Reboot(force),
        "suspend" => LoginctlCommand::Suspend(force),
        "hibernate" => LoginctlCommand::Hibernate(force),
        _ => {
            let _ = writeln!(io::stderr(), "loginctl: unknown command '{cmd}'");
            LoginctlCommand::Help
        }
    }
}

/// Execute a loginctl command.
///
/// In a real implementation, these would communicate with the logind daemon
/// via D-Bus. Here we operate on a local `Daemon` instance to demonstrate
/// the logic and enable thorough testing.
fn run_loginctl_command(daemon: &mut Daemon, cmd: &LoginctlCommand) -> i32 {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    match cmd {
        LoginctlCommand::ListSessions => {
            let _ = writeln!(out, "{:<8} {:<6} {:<16} {:<12} {}", "SESSION", "UID", "USER", "SEAT", "TTY");
            let mut sessions: Vec<&Session> = daemon.sessions.values().collect();
            sessions.sort_by(|a, b| a.id.cmp(&b.id));
            for session in &sessions {
                let _ = writeln!(out, "{}", session.format_list_line());
            }
            let _ = writeln!(out, "\n{} sessions listed.", sessions.len());
            0
        }
        LoginctlCommand::ListUsers => {
            let _ = writeln!(out, "{:<8} {:<16} {}", "UID", "USER", "STATE");
            let mut users: Vec<&User> = daemon.users.values().collect();
            users.sort_by_key(|u| u.uid);
            for user in &users {
                let _ = writeln!(out, "{}", user.format_list_line());
            }
            let _ = writeln!(out, "\n{} users listed.", users.len());
            0
        }
        LoginctlCommand::ListSeats => {
            let _ = writeln!(out, "SEAT");
            let mut seats: Vec<&Seat> = daemon.seats.values().collect();
            seats.sort_by(|a, b| a.id.cmp(&b.id));
            for seat in &seats {
                let _ = writeln!(out, "{}", seat.format_list_line());
            }
            let _ = writeln!(out, "\n{} seats listed.", seats.len());
            0
        }
        LoginctlCommand::ShowSession(id) => {
            if id.is_empty() {
                let _ = writeln!(io::stderr(), "loginctl: session ID required");
                return 1;
            }
            match daemon.sessions.get(id.as_str()) {
                Some(session) => {
                    let _ = write!(out, "{}", session.format_properties());
                    0
                }
                None => {
                    let _ = writeln!(io::stderr(), "loginctl: session '{id}' not found");
                    1
                }
            }
        }
        LoginctlCommand::ShowUser(uid_str) => {
            if uid_str.is_empty() {
                let _ = writeln!(io::stderr(), "loginctl: UID required");
                return 1;
            }
            let uid: u32 = match uid_str.parse() {
                Ok(v) => v,
                Err(_) => {
                    let _ = writeln!(io::stderr(), "loginctl: invalid UID '{uid_str}'");
                    return 1;
                }
            };
            match daemon.users.get(&uid) {
                Some(user) => {
                    let _ = write!(out, "{}", user.format_properties());
                    0
                }
                None => {
                    let _ = writeln!(io::stderr(), "loginctl: user {uid} not found");
                    1
                }
            }
        }
        LoginctlCommand::ShowSeat(id) => {
            if id.is_empty() {
                let _ = writeln!(io::stderr(), "loginctl: seat ID required");
                return 1;
            }
            match daemon.seats.get(id.as_str()) {
                Some(seat) => {
                    let _ = write!(out, "{}", seat.format_properties());
                    0
                }
                None => {
                    let _ = writeln!(io::stderr(), "loginctl: seat '{id}' not found");
                    1
                }
            }
        }
        LoginctlCommand::Activate(id) => {
            if id.is_empty() {
                let _ = writeln!(io::stderr(), "loginctl: session ID required");
                return 1;
            }
            match daemon.activate_session(id) {
                Ok(()) => {
                    let _ = writeln!(out, "Activated session {id}.");
                    0
                }
                Err(e) => {
                    let _ = writeln!(io::stderr(), "loginctl: failed to activate session: {e}");
                    1
                }
            }
        }
        LoginctlCommand::LockSession(id) => {
            if id.is_empty() {
                let _ = writeln!(io::stderr(), "loginctl: session ID required");
                return 1;
            }
            match daemon.lock_session(id) {
                Ok(()) => {
                    let _ = writeln!(out, "Session {id} locked.");
                    0
                }
                Err(e) => {
                    let _ = writeln!(io::stderr(), "loginctl: failed to lock session: {e}");
                    1
                }
            }
        }
        LoginctlCommand::UnlockSession(id) => {
            if id.is_empty() {
                let _ = writeln!(io::stderr(), "loginctl: session ID required");
                return 1;
            }
            match daemon.unlock_session(id) {
                Ok(()) => {
                    let _ = writeln!(out, "Session {id} unlocked.");
                    0
                }
                Err(e) => {
                    let _ = writeln!(io::stderr(), "loginctl: failed to unlock session: {e}");
                    1
                }
            }
        }
        LoginctlCommand::TerminateSession(id) => {
            if id.is_empty() {
                let _ = writeln!(io::stderr(), "loginctl: session ID required");
                return 1;
            }
            match daemon.terminate_session(id) {
                Ok(()) => {
                    let _ = writeln!(out, "Session {id} terminated.");
                    0
                }
                Err(e) => {
                    let _ = writeln!(io::stderr(), "loginctl: failed to terminate session: {e}");
                    1
                }
            }
        }
        LoginctlCommand::TerminateUser(uid_str) => {
            if uid_str.is_empty() {
                let _ = writeln!(io::stderr(), "loginctl: UID required");
                return 1;
            }
            let uid: u32 = match uid_str.parse() {
                Ok(v) => v,
                Err(_) => {
                    let _ = writeln!(io::stderr(), "loginctl: invalid UID '{uid_str}'");
                    return 1;
                }
            };
            match daemon.terminate_user(uid) {
                Ok(()) => {
                    let _ = writeln!(out, "User {uid} terminated.");
                    0
                }
                Err(e) => {
                    let _ = writeln!(io::stderr(), "loginctl: failed to terminate user: {e}");
                    1
                }
            }
        }
        LoginctlCommand::KillSession(id, sig) => {
            if id.is_empty() {
                let _ = writeln!(io::stderr(), "loginctl: session ID required");
                return 1;
            }
            match daemon.kill_session(id, *sig) {
                Ok(pid) => {
                    let _ = writeln!(out, "Sent signal {sig} to session {id} (leader PID {pid}).");
                    0
                }
                Err(e) => {
                    let _ = writeln!(io::stderr(), "loginctl: failed to kill session: {e}");
                    1
                }
            }
        }
        LoginctlCommand::KillUser(uid_str, sig) => {
            if uid_str.is_empty() {
                let _ = writeln!(io::stderr(), "loginctl: UID required");
                return 1;
            }
            let uid: u32 = match uid_str.parse() {
                Ok(v) => v,
                Err(_) => {
                    let _ = writeln!(io::stderr(), "loginctl: invalid UID '{uid_str}'");
                    return 1;
                }
            };
            match daemon.kill_user(uid, *sig) {
                Ok(pids) => {
                    let _ = writeln!(
                        out,
                        "Sent signal {sig} to user {uid} ({} session leaders).",
                        pids.len()
                    );
                    0
                }
                Err(e) => {
                    let _ = writeln!(io::stderr(), "loginctl: failed to kill user: {e}");
                    1
                }
            }
        }
        LoginctlCommand::PowerOff(force) => {
            handle_power_command(&mut out, daemon, PowerAction::PowerOff, *force)
        }
        LoginctlCommand::Reboot(force) => {
            handle_power_command(&mut out, daemon, PowerAction::Reboot, *force)
        }
        LoginctlCommand::Suspend(force) => {
            handle_power_command(&mut out, daemon, PowerAction::Suspend, *force)
        }
        LoginctlCommand::Hibernate(force) => {
            handle_power_command(&mut out, daemon, PowerAction::Hibernate, *force)
        }
        LoginctlCommand::Help => {
            let _ = writeln!(
                out,
                "loginctl - control the login manager\n\n\
                 Session Commands:\n  \
                   list-sessions                 List sessions\n  \
                   show-session ID               Show session properties\n  \
                   activate ID                   Activate a session\n  \
                   lock-session ID               Lock session screen\n  \
                   unlock-session ID             Unlock session screen\n  \
                   terminate-session ID          Terminate a session\n  \
                   kill-session ID [--signal=N]  Send signal to session\n\n\
                 User Commands:\n  \
                   list-users                    List users\n  \
                   show-user UID                 Show user properties\n  \
                   terminate-user UID            Terminate user sessions\n  \
                   kill-user UID [--signal=N]    Send signal to user\n\n\
                 Seat Commands:\n  \
                   list-seats                    List seats\n  \
                   show-seat SEAT                Show seat properties\n\n\
                 Power Commands:\n  \
                   poweroff [--force]            Power off the system\n  \
                   reboot [--force]              Reboot the system\n  \
                   suspend [--force]             Suspend the system\n  \
                   hibernate [--force]           Hibernate the system"
            );
            0
        }
        LoginctlCommand::Version => {
            let _ = writeln!(out, "loginctl {VERSION}");
            0
        }
    }
}

/// Handle power action commands (poweroff/reboot/suspend/hibernate).
fn handle_power_command(
    out: &mut io::StdoutLock<'_>,
    daemon: &Daemon,
    action: PowerAction,
    force: bool,
) -> i32 {
    match daemon.request_power_action(action, force) {
        PowerActionResult::Allowed => {
            let _ = writeln!(out, "Requesting {}...", action.as_str());
            // In a real system, this would issue the syscall or send a D-Bus
            // message to the daemon.
            0
        }
        PowerActionResult::Inhibited => {
            let _ = writeln!(
                io::stderr(),
                "loginctl: {} is inhibited, use --force to override",
                action.as_str()
            );
            1
        }
        PowerActionResult::Denied => {
            let _ = writeln!(
                io::stderr(),
                "loginctl: {} is not allowed by policy",
                action.as_str()
            );
            1
        }
    }
}

/// Run the loginctl personality.
fn run_loginctl(args: &[String]) -> i32 {
    let cmd = parse_loginctl_args(args);
    let config = DaemonConfig::default();
    let mut daemon = Daemon::new(config);
    run_loginctl_command(&mut daemon, &cmd)
}

// ============================================================================
// Entry points
// ============================================================================

fn run_main() -> i32 {
    let args: Vec<String> = env::args().collect();
    let personality = detect_personality(&args);

    // Strip the subcommand if present to pass remaining args.
    let sub_args: Vec<String> = if args.len() > 1
        && matches!(args.get(1).map(|s| s.as_str()), Some("ctl" | "loginctl" | "daemon"))
    {
        args[2..].to_vec()
    } else if args.len() > 1 {
        args[1..].to_vec()
    } else {
        Vec::new()
    };

    match personality {
        "loginctl" => run_loginctl(&sub_args),
        "logind" => run_daemon(&sub_args),
        _ => {
            let _ = writeln!(io::stderr(), "logind: unknown personality");
            1
        }
    }
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    run_main()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Helper: create a daemon with some sample data ---

    fn test_daemon() -> Daemon {
        let mut d = Daemon::new(DaemonConfig::default());
        d.create_session(
            1000, "alice", SessionType::Wayland, SessionClass::User,
            "seat0", 1, "/dev/tty1", false, "", "login", "gnome", 100,
        ).unwrap();
        d.create_session(
            1001, "bob", SessionType::Tty, SessionClass::User,
            "seat0", 2, "/dev/tty2", false, "", "login", "", 200,
        ).unwrap();
        d.create_session(
            1000, "alice", SessionType::X11, SessionClass::User,
            "", 0, ":1", false, "", "sshd", "kde", 300,
        ).unwrap();
        d
    }

    // --- Personality detection ---

    #[test]
    fn test_detect_logind_binary() {
        let args = vec!["logind".to_string()];
        assert_eq!(detect_personality(&args), "logind");
    }

    #[test]
    fn test_detect_loginctl_binary() {
        let args = vec!["loginctl".to_string(), "list-sessions".to_string()];
        assert_eq!(detect_personality(&args), "loginctl");
    }

    #[test]
    fn test_detect_logind_with_path() {
        let args = vec!["/usr/sbin/logind".to_string()];
        assert_eq!(detect_personality(&args), "logind");
    }

    #[test]
    fn test_detect_loginctl_with_path() {
        let args = vec!["/usr/bin/loginctl".to_string(), "status".to_string()];
        assert_eq!(detect_personality(&args), "loginctl");
    }

    #[test]
    fn test_detect_logind_windows_path() {
        let args = vec!["C:\\bin\\logind.exe".to_string()];
        assert_eq!(detect_personality(&args), "logind");
    }

    #[test]
    fn test_detect_loginctl_windows_path() {
        let args = vec!["C:\\bin\\loginctl.exe".to_string()];
        assert_eq!(detect_personality(&args), "loginctl");
    }

    #[test]
    fn test_detect_subcommand_ctl() {
        let args = vec!["logind".to_string(), "ctl".to_string(), "list-sessions".to_string()];
        assert_eq!(detect_personality(&args), "loginctl");
    }

    #[test]
    fn test_detect_subcommand_daemon() {
        let args = vec!["logind".to_string(), "daemon".to_string()];
        assert_eq!(detect_personality(&args), "logind");
    }

    #[test]
    fn test_detect_default_is_logind() {
        let args = vec!["something_else".to_string()];
        assert_eq!(detect_personality(&args), "logind");
    }

    #[test]
    fn test_detect_empty_args() {
        let args: Vec<String> = vec![];
        assert_eq!(detect_personality(&args), "logind");
    }

    // --- Session type ---

    #[test]
    fn test_session_type_roundtrip() {
        for st in &[SessionType::Tty, SessionType::X11, SessionType::Wayland, SessionType::Unspecified] {
            assert_eq!(SessionType::from_str(st.as_str()), *st);
        }
    }

    #[test]
    fn test_session_type_unknown() {
        assert_eq!(SessionType::from_str("mir"), SessionType::Unspecified);
    }

    // --- Session class ---

    #[test]
    fn test_session_class_roundtrip() {
        for sc in &[SessionClass::User, SessionClass::Greeter, SessionClass::LockScreen] {
            assert_eq!(SessionClass::from_str(sc.as_str()), *sc);
        }
    }

    #[test]
    fn test_session_class_unknown() {
        assert_eq!(SessionClass::from_str("robot"), SessionClass::User);
    }

    // --- Session state ---

    #[test]
    fn test_session_state_roundtrip() {
        for ss in &[
            SessionState::Opening, SessionState::Online, SessionState::Active,
            SessionState::Background, SessionState::Closing,
        ] {
            assert_eq!(SessionState::from_str(ss.as_str()), *ss);
        }
    }

    #[test]
    fn test_session_state_unknown() {
        assert_eq!(SessionState::from_str("limbo"), SessionState::Online);
    }

    // --- Session creation ---

    #[test]
    fn test_create_session_basic() {
        let mut d = Daemon::new(DaemonConfig::default());
        let id = d.create_session(
            1000, "alice", SessionType::Tty, SessionClass::User,
            "seat0", 1, "/dev/tty1", false, "", "login", "", 42,
        ).unwrap();
        assert_eq!(id, "1");
        assert_eq!(d.sessions.len(), 1);
        let s = d.sessions.get("1").unwrap();
        assert_eq!(s.uid, 1000);
        assert_eq!(s.user, "alice");
        assert_eq!(s.session_type, SessionType::Tty);
        assert_eq!(s.leader_pid, 42);
        assert_eq!(s.state, SessionState::Active);
    }

    #[test]
    fn test_create_session_assigns_seat() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.create_session(
            1000, "alice", SessionType::Tty, SessionClass::User,
            "seat0", 1, "/dev/tty1", false, "", "login", "", 100,
        ).unwrap();
        let seat = d.seats.get("seat0").unwrap();
        assert!(seat.sessions.contains(&"1".to_string()));
        assert_eq!(seat.active_session, "1");
    }

    #[test]
    fn test_create_session_tracks_user() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.create_session(
            1000, "alice", SessionType::Tty, SessionClass::User,
            "", 0, "", false, "", "", "", 100,
        ).unwrap();
        let user = d.users.get(&1000).unwrap();
        assert_eq!(user.name, "alice");
        assert_eq!(user.sessions.len(), 1);
    }

    #[test]
    fn test_create_multiple_sessions_same_user() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.create_session(1000, "alice", SessionType::Tty, SessionClass::User, "", 0, "", false, "", "", "", 100).unwrap();
        d.create_session(1000, "alice", SessionType::X11, SessionClass::User, "", 0, "", false, "", "", "", 200).unwrap();
        assert_eq!(d.sessions.len(), 2);
        let user = d.users.get(&1000).unwrap();
        assert_eq!(user.sessions.len(), 2);
    }

    #[test]
    fn test_create_session_max_limit() {
        let mut config = DaemonConfig::default();
        config.max_sessions = 2;
        let mut d = Daemon::new(config);
        d.create_session(1000, "a", SessionType::Tty, SessionClass::User, "", 0, "", false, "", "", "", 1).unwrap();
        d.create_session(1001, "b", SessionType::Tty, SessionClass::User, "", 0, "", false, "", "", "", 2).unwrap();
        let err = d.create_session(1002, "c", SessionType::Tty, SessionClass::User, "", 0, "", false, "", "", "", 3);
        assert!(err.is_err());
    }

    #[test]
    fn test_session_id_incrementing() {
        let mut d = Daemon::new(DaemonConfig::default());
        let id1 = d.allocate_session_id();
        let id2 = d.allocate_session_id();
        let id3 = d.allocate_session_id();
        assert_eq!(id1, "1");
        assert_eq!(id2, "2");
        assert_eq!(id3, "3");
    }

    #[test]
    fn test_session_id_wraps() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.next_session_id = MAX_SESSION_ID;
        let id1 = d.allocate_session_id();
        let id2 = d.allocate_session_id();
        assert_eq!(id1, MAX_SESSION_ID.to_string());
        assert_eq!(id2, "1");
    }

    #[test]
    fn test_session_remote() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.create_session(
            1000, "alice", SessionType::Tty, SessionClass::User,
            "", 0, "pts/0", true, "10.0.0.5", "sshd", "", 100,
        ).unwrap();
        let s = d.sessions.get("1").unwrap();
        assert!(s.remote);
        assert_eq!(s.remote_host, "10.0.0.5");
    }

    // --- Session termination ---

    #[test]
    fn test_terminate_session() {
        let mut d = test_daemon();
        assert!(d.sessions.contains_key("1"));
        d.terminate_session("1").unwrap();
        assert!(!d.sessions.contains_key("1"));
    }

    #[test]
    fn test_terminate_session_removes_from_seat() {
        let mut d = test_daemon();
        d.terminate_session("1").unwrap();
        let seat = d.seats.get("seat0").unwrap();
        assert!(!seat.sessions.contains(&"1".to_string()));
    }

    #[test]
    fn test_terminate_session_updates_user() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.create_session(1000, "alice", SessionType::Tty, SessionClass::User, "", 0, "", false, "", "", "", 100).unwrap();
        d.terminate_session("1").unwrap();
        let user = d.users.get(&1000).unwrap();
        assert_eq!(user.state, "closing");
    }

    #[test]
    fn test_terminate_session_not_found() {
        let mut d = Daemon::new(DaemonConfig::default());
        assert!(d.terminate_session("999").is_err());
    }

    #[test]
    fn test_terminate_user() {
        let mut d = test_daemon();
        d.terminate_user(1000).unwrap();
        assert!(!d.users.contains_key(&1000));
        // All of alice's sessions should be gone.
        let alice_sessions: Vec<_> = d.sessions.values().filter(|s| s.uid == 1000).collect();
        assert!(alice_sessions.is_empty());
    }

    #[test]
    fn test_terminate_user_no_sessions() {
        let mut d = Daemon::new(DaemonConfig::default());
        assert!(d.terminate_user(9999).is_err());
    }

    // --- Session activation ---

    #[test]
    fn test_activate_session() {
        let mut d = test_daemon();
        // Session "1" should be active, activate "2".
        d.activate_session("2").unwrap();
        assert_eq!(d.sessions.get("2").unwrap().state, SessionState::Active);
        assert_eq!(d.sessions.get("1").unwrap().state, SessionState::Background);
        assert_eq!(d.seats.get("seat0").unwrap().active_session, "2");
    }

    #[test]
    fn test_activate_session_no_seat() {
        let mut d = test_daemon();
        // Session "3" has no seat.
        let err = d.activate_session("3");
        assert!(err.is_err());
    }

    #[test]
    fn test_activate_session_not_found() {
        let mut d = Daemon::new(DaemonConfig::default());
        assert!(d.activate_session("999").is_err());
    }

    // --- Lock/unlock ---

    #[test]
    fn test_lock_session() {
        let mut d = test_daemon();
        d.lock_session("1").unwrap();
        assert!(d.sessions.get("1").unwrap().locked);
    }

    #[test]
    fn test_unlock_session() {
        let mut d = test_daemon();
        d.lock_session("1").unwrap();
        d.unlock_session("1").unwrap();
        assert!(!d.sessions.get("1").unwrap().locked);
    }

    #[test]
    fn test_lock_session_not_found() {
        let mut d = Daemon::new(DaemonConfig::default());
        assert!(d.lock_session("999").is_err());
    }

    // --- Idle tracking ---

    #[test]
    fn test_set_session_idle() {
        let mut d = test_daemon();
        d.set_session_idle("1", true, 12345).unwrap();
        let s = d.sessions.get("1").unwrap();
        assert!(s.idle);
        assert_eq!(s.idle_since, 12345);
    }

    #[test]
    fn test_set_session_not_idle() {
        let mut d = test_daemon();
        d.set_session_idle("1", true, 100).unwrap();
        d.set_session_idle("1", false, 200).unwrap();
        let s = d.sessions.get("1").unwrap();
        assert!(!s.idle);
        assert_eq!(s.idle_since, 0);
    }

    #[test]
    fn test_system_idle_all_sessions_idle() {
        let mut d = test_daemon();
        for id in ["1", "2", "3"] {
            d.set_session_idle(id, true, 100).unwrap();
        }
        d.update_idle_state(100);
        assert!(d.system_idle);
        assert_eq!(d.idle_since, 100);
    }

    #[test]
    fn test_system_not_idle_when_one_active() {
        let mut d = test_daemon();
        d.set_session_idle("1", true, 100).unwrap();
        // Session "2" is still not idle.
        d.update_idle_state(100);
        assert!(!d.system_idle);
    }

    #[test]
    fn test_system_not_idle_no_sessions() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.update_idle_state(100);
        assert!(!d.system_idle);
    }

    // --- Inhibitors ---

    #[test]
    fn test_add_inhibitor() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.add_inhibitor(InhibitWhat::Shutdown, "firefox", "downloading", InhibitMode::Block, 1000, 42).unwrap();
        assert_eq!(d.inhibitors.len(), 1);
        assert_eq!(d.inhibitors[0].who, "firefox");
    }

    #[test]
    fn test_inhibitor_limit() {
        let mut d = Daemon::new(DaemonConfig::default());
        for i in 0..MAX_INHIBITORS {
            d.add_inhibitor(InhibitWhat::Shutdown, "app", "reason", InhibitMode::Block, 1000, i as u32).unwrap();
        }
        let err = d.add_inhibitor(InhibitWhat::Shutdown, "app", "reason", InhibitMode::Block, 1000, 9999);
        assert!(err.is_err());
    }

    #[test]
    fn test_remove_inhibitors_by_pid() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.add_inhibitor(InhibitWhat::Shutdown, "app1", "r1", InhibitMode::Block, 1000, 42).unwrap();
        d.add_inhibitor(InhibitWhat::Sleep, "app2", "r2", InhibitMode::Delay, 1000, 42).unwrap();
        d.add_inhibitor(InhibitWhat::Idle, "app3", "r3", InhibitMode::Block, 1001, 99).unwrap();
        let removed = d.remove_inhibitors_by_pid(42);
        assert_eq!(removed, 2);
        assert_eq!(d.inhibitors.len(), 1);
    }

    #[test]
    fn test_is_inhibited() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.add_inhibitor(InhibitWhat::Shutdown, "app", "reason", InhibitMode::Block, 1000, 42).unwrap();
        assert!(d.is_inhibited(InhibitWhat::Shutdown, InhibitMode::Block));
        assert!(!d.is_inhibited(InhibitWhat::Shutdown, InhibitMode::Delay));
        assert!(!d.is_inhibited(InhibitWhat::Sleep, InhibitMode::Block));
    }

    #[test]
    fn test_is_inhibited_any() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.add_inhibitor(InhibitWhat::Shutdown, "app", "reason", InhibitMode::Delay, 1000, 42).unwrap();
        assert!(d.is_inhibited_any(InhibitWhat::Shutdown));
        assert!(!d.is_inhibited_any(InhibitWhat::Sleep));
    }

    // --- InhibitWhat / InhibitMode roundtrip ---

    #[test]
    fn test_inhibit_what_from_str() {
        assert_eq!(InhibitWhat::from_str("shutdown"), Some(InhibitWhat::Shutdown));
        assert_eq!(InhibitWhat::from_str("sleep"), Some(InhibitWhat::Sleep));
        assert_eq!(InhibitWhat::from_str("idle"), Some(InhibitWhat::Idle));
        assert_eq!(InhibitWhat::from_str("handle-power-key"), Some(InhibitWhat::HandlePowerKey));
        assert_eq!(InhibitWhat::from_str("bogus"), None);
    }

    #[test]
    fn test_inhibit_mode_roundtrip() {
        assert_eq!(InhibitMode::from_str("block"), InhibitMode::Block);
        assert_eq!(InhibitMode::from_str("delay"), InhibitMode::Delay);
        assert_eq!(InhibitMode::from_str("unknown"), InhibitMode::Block);
    }

    // --- Power action requests ---

    #[test]
    fn test_power_action_allowed() {
        let d = Daemon::new(DaemonConfig::default());
        assert_eq!(d.request_power_action(PowerAction::PowerOff, false), PowerActionResult::Allowed);
        assert_eq!(d.request_power_action(PowerAction::Reboot, false), PowerActionResult::Allowed);
        assert_eq!(d.request_power_action(PowerAction::Suspend, false), PowerActionResult::Allowed);
        assert_eq!(d.request_power_action(PowerAction::Hibernate, false), PowerActionResult::Allowed);
    }

    #[test]
    fn test_power_action_inhibited() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.add_inhibitor(InhibitWhat::Shutdown, "app", "busy", InhibitMode::Block, 1000, 42).unwrap();
        assert_eq!(d.request_power_action(PowerAction::PowerOff, false), PowerActionResult::Inhibited);
        assert_eq!(d.request_power_action(PowerAction::Reboot, false), PowerActionResult::Inhibited);
        // Suspend/hibernate use InhibitWhat::Sleep, not Shutdown.
        assert_eq!(d.request_power_action(PowerAction::Suspend, false), PowerActionResult::Allowed);
    }

    #[test]
    fn test_power_action_force_overrides_inhibitor() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.add_inhibitor(InhibitWhat::Shutdown, "app", "busy", InhibitMode::Block, 1000, 42).unwrap();
        assert_eq!(d.request_power_action(PowerAction::PowerOff, true), PowerActionResult::Allowed);
    }

    #[test]
    fn test_power_action_denied() {
        let mut config = DaemonConfig::default();
        config.allow_suspend = false;
        let d = Daemon::new(config);
        assert_eq!(d.request_power_action(PowerAction::Suspend, false), PowerActionResult::Denied);
    }

    // --- Seat management ---

    #[test]
    fn test_default_seat0_exists() {
        let d = Daemon::new(DaemonConfig::default());
        assert!(d.seats.contains_key("seat0"));
    }

    #[test]
    fn test_create_seat() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.create_seat("seat1").unwrap();
        assert!(d.seats.contains_key("seat1"));
    }

    #[test]
    fn test_create_seat_duplicate() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.create_seat("seat1").unwrap();
        assert!(d.create_seat("seat1").is_err());
    }

    #[test]
    fn test_remove_seat() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.create_seat("seat1").unwrap();
        d.remove_seat("seat1").unwrap();
        assert!(!d.seats.contains_key("seat1"));
    }

    #[test]
    fn test_remove_seat0_forbidden() {
        let mut d = Daemon::new(DaemonConfig::default());
        assert!(d.remove_seat("seat0").is_err());
    }

    #[test]
    fn test_remove_seat_detaches_sessions() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.create_seat("seat1").unwrap();
        d.create_session(
            1000, "alice", SessionType::Tty, SessionClass::User,
            "seat1", 1, "/dev/tty1", false, "", "", "", 100,
        ).unwrap();
        d.remove_seat("seat1").unwrap();
        let s = d.sessions.get("1").unwrap();
        assert!(s.seat.is_empty());
        assert_eq!(s.vt_nr, 0);
    }

    // --- VT switching ---

    #[test]
    fn test_switch_vt() {
        let mut d = test_daemon();
        // session "1" is on VT 1, session "2" is on VT 2.
        d.switch_vt("seat0", 2).unwrap();
        assert_eq!(d.seats.get("seat0").unwrap().active_session, "2");
        assert_eq!(d.sessions.get("2").unwrap().state, SessionState::Active);
    }

    #[test]
    fn test_switch_vt_nonexistent() {
        let mut d = test_daemon();
        assert!(d.switch_vt("seat0", 99).is_err());
    }

    // --- Kill session/user ---

    #[test]
    fn test_kill_session() {
        let d = test_daemon();
        let pid = d.kill_session("1", 15).unwrap();
        assert_eq!(pid, 100); // leader PID from test_daemon.
    }

    #[test]
    fn test_kill_session_not_found() {
        let d = Daemon::new(DaemonConfig::default());
        assert!(d.kill_session("999", 15).is_err());
    }

    #[test]
    fn test_kill_user() {
        let d = test_daemon();
        let pids = d.kill_user(1000, 9).unwrap();
        // Alice has sessions "1" (pid 100) and "3" (pid 300).
        assert_eq!(pids.len(), 2);
        assert!(pids.contains(&100));
        assert!(pids.contains(&300));
    }

    #[test]
    fn test_kill_user_not_found() {
        let d = Daemon::new(DaemonConfig::default());
        assert!(d.kill_user(9999, 15).is_err());
    }

    // --- Configuration parsing ---

    #[test]
    fn test_parse_config_defaults() {
        let config = parse_config("");
        assert_eq!(config.idle_timeout, DEFAULT_IDLE_TIMEOUT);
        assert_eq!(config.handle_power_key, "poweroff");
        assert!(!config.kill_user_processes);
    }

    #[test]
    fn test_parse_config_values() {
        let content = "\
[Login]
IdleAction=suspend
IdleActionSec=600
HandlePowerKey=hibernate
KillUserProcesses=yes
InhibitDelayMaxSec=10
";
        let config = parse_config(content);
        assert_eq!(config.idle_action, "suspend");
        assert_eq!(config.idle_timeout, 600);
        assert_eq!(config.handle_power_key, "hibernate");
        assert!(config.kill_user_processes);
        assert_eq!(config.inhibit_delay_max, 10);
    }

    #[test]
    fn test_parse_config_ignores_other_sections() {
        let content = "\
[Manager]
IdleAction=poweroff
[Login]
IdleAction=suspend
";
        let config = parse_config(content);
        assert_eq!(config.idle_action, "suspend");
    }

    #[test]
    fn test_parse_config_comments_blank_lines() {
        let content = "\
# A comment
; Another comment

[Login]
HandleSuspendKey=ignore
";
        let config = parse_config(content);
        assert_eq!(config.handle_suspend_key, "ignore");
    }

    // --- Signal parsing ---

    #[test]
    fn test_parse_signal_numeric() {
        assert_eq!(parse_signal("9"), Some(9));
        assert_eq!(parse_signal("15"), Some(15));
    }

    #[test]
    fn test_parse_signal_named() {
        assert_eq!(parse_signal("TERM"), Some(15));
        assert_eq!(parse_signal("KILL"), Some(9));
        assert_eq!(parse_signal("HUP"), Some(1));
        assert_eq!(parse_signal("USR1"), Some(10));
    }

    #[test]
    fn test_parse_signal_with_sig_prefix() {
        assert_eq!(parse_signal("SIGTERM"), Some(15));
        assert_eq!(parse_signal("SIGKILL"), Some(9));
    }

    #[test]
    fn test_parse_signal_with_flag_prefix() {
        assert_eq!(parse_signal("--signal=9"), Some(9));
        assert_eq!(parse_signal("--signal=TERM"), Some(15));
        assert_eq!(parse_signal("--kill-signal=HUP"), Some(1));
    }

    #[test]
    fn test_parse_signal_unknown() {
        assert_eq!(parse_signal("NOSUCHSIG"), None);
    }

    // --- loginctl argument parsing ---

    #[test]
    fn test_parse_loginctl_list_sessions() {
        let args = vec!["list-sessions".to_string()];
        assert_eq!(parse_loginctl_args(&args), LoginctlCommand::ListSessions);
    }

    #[test]
    fn test_parse_loginctl_list_users() {
        let args = vec!["list-users".to_string()];
        assert_eq!(parse_loginctl_args(&args), LoginctlCommand::ListUsers);
    }

    #[test]
    fn test_parse_loginctl_list_seats() {
        let args = vec!["list-seats".to_string()];
        assert_eq!(parse_loginctl_args(&args), LoginctlCommand::ListSeats);
    }

    #[test]
    fn test_parse_loginctl_show_session() {
        let args = vec!["show-session".to_string(), "1".to_string()];
        assert_eq!(parse_loginctl_args(&args), LoginctlCommand::ShowSession("1".to_string()));
    }

    #[test]
    fn test_parse_loginctl_activate() {
        let args = vec!["activate".to_string(), "3".to_string()];
        assert_eq!(parse_loginctl_args(&args), LoginctlCommand::Activate("3".to_string()));
    }

    #[test]
    fn test_parse_loginctl_poweroff() {
        let args = vec!["poweroff".to_string()];
        assert_eq!(parse_loginctl_args(&args), LoginctlCommand::PowerOff(false));
    }

    #[test]
    fn test_parse_loginctl_poweroff_force() {
        let args = vec!["poweroff".to_string(), "--force".to_string()];
        assert_eq!(parse_loginctl_args(&args), LoginctlCommand::PowerOff(true));
    }

    #[test]
    fn test_parse_loginctl_kill_session_signal() {
        let args = vec!["kill-session".to_string(), "5".to_string(), "--signal=9".to_string()];
        assert_eq!(parse_loginctl_args(&args), LoginctlCommand::KillSession("5".to_string(), 9));
    }

    #[test]
    fn test_parse_loginctl_empty() {
        let args: Vec<String> = vec![];
        assert_eq!(parse_loginctl_args(&args), LoginctlCommand::Help);
    }

    #[test]
    fn test_parse_loginctl_help_flag() {
        let args = vec!["--help".to_string()];
        assert_eq!(parse_loginctl_args(&args), LoginctlCommand::Help);
    }

    #[test]
    fn test_parse_loginctl_version() {
        let args = vec!["--version".to_string()];
        assert_eq!(parse_loginctl_args(&args), LoginctlCommand::Version);
    }

    // --- loginctl command execution ---

    #[test]
    fn test_loginctl_list_sessions_empty() {
        let mut d = Daemon::new(DaemonConfig::default());
        let rc = run_loginctl_command(&mut d, &LoginctlCommand::ListSessions);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_loginctl_list_sessions_with_data() {
        let mut d = test_daemon();
        let rc = run_loginctl_command(&mut d, &LoginctlCommand::ListSessions);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_loginctl_show_session_found() {
        let mut d = test_daemon();
        let rc = run_loginctl_command(&mut d, &LoginctlCommand::ShowSession("1".to_string()));
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_loginctl_show_session_not_found() {
        let mut d = Daemon::new(DaemonConfig::default());
        let rc = run_loginctl_command(&mut d, &LoginctlCommand::ShowSession("999".to_string()));
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_loginctl_show_session_empty_id() {
        let mut d = Daemon::new(DaemonConfig::default());
        let rc = run_loginctl_command(&mut d, &LoginctlCommand::ShowSession(String::new()));
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_loginctl_terminate_session() {
        let mut d = test_daemon();
        let rc = run_loginctl_command(&mut d, &LoginctlCommand::TerminateSession("1".to_string()));
        assert_eq!(rc, 0);
        assert!(!d.sessions.contains_key("1"));
    }

    #[test]
    fn test_loginctl_lock_unlock() {
        let mut d = test_daemon();
        let rc = run_loginctl_command(&mut d, &LoginctlCommand::LockSession("1".to_string()));
        assert_eq!(rc, 0);
        assert!(d.sessions.get("1").unwrap().locked);
        let rc = run_loginctl_command(&mut d, &LoginctlCommand::UnlockSession("1".to_string()));
        assert_eq!(rc, 0);
        assert!(!d.sessions.get("1").unwrap().locked);
    }

    #[test]
    fn test_loginctl_power_inhibited() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.add_inhibitor(InhibitWhat::Shutdown, "app", "busy", InhibitMode::Block, 1000, 42).unwrap();
        let rc = run_loginctl_command(&mut d, &LoginctlCommand::PowerOff(false));
        assert_eq!(rc, 1);
    }

    // --- Formatting ---

    #[test]
    fn test_session_format_list_line() {
        let mut s = Session::new("42", 1000, "alice");
        s.seat = "seat0".to_string();
        s.tty = "/dev/tty1".to_string();
        let line = s.format_list_line();
        assert!(line.contains("42"));
        assert!(line.contains("1000"));
        assert!(line.contains("alice"));
        assert!(line.contains("seat0"));
        assert!(line.contains("/dev/tty1"));
    }

    #[test]
    fn test_session_format_properties() {
        let mut s = Session::new("7", 1000, "alice");
        s.session_type = SessionType::Wayland;
        s.state = SessionState::Active;
        s.locked = true;
        let props = s.format_properties();
        assert!(props.contains("Id=7"));
        assert!(props.contains("User=alice (1000)"));
        assert!(props.contains("Type=wayland"));
        assert!(props.contains("Locked=yes"));
    }

    #[test]
    fn test_user_format_list_line() {
        let u = User::new(1000, "alice");
        let line = u.format_list_line();
        assert!(line.contains("1000"));
        assert!(line.contains("alice"));
    }

    #[test]
    fn test_seat_format_properties() {
        let mut seat = Seat::new("seat0");
        seat.sessions.push("1".to_string());
        seat.active_session = "1".to_string();
        let props = seat.format_properties();
        assert!(props.contains("Id=seat0"));
        assert!(props.contains("Sessions=1"));
        assert!(props.contains("ActiveSessn=1"));
    }

    #[test]
    fn test_inhibitor_format_line() {
        let inh = Inhibitor {
            what: InhibitWhat::Shutdown,
            who: "firefox".to_string(),
            why: "downloading file".to_string(),
            mode: InhibitMode::Block,
            uid: 1000,
            pid: 42,
        };
        let line = inh.format_line();
        assert!(line.contains("shutdown"));
        assert!(line.contains("1000"));
        assert!(line.contains("42"));
        assert!(line.contains("block"));
        assert!(line.contains("downloading file"));
    }

    #[test]
    fn test_session_seat_display_empty() {
        let s = Session::new("1", 1000, "alice");
        assert_eq!(s.seat_display(), "-");
        assert_eq!(s.tty_display(), "-");
    }

    #[test]
    fn test_user_linger() {
        let mut d = Daemon::new(DaemonConfig::default());
        d.create_session(1000, "alice", SessionType::Tty, SessionClass::User, "", 0, "", false, "", "", "", 100).unwrap();
        d.users.get_mut(&1000).unwrap().linger = true;
        d.terminate_session("1").unwrap();
        let user = d.users.get(&1000).unwrap();
        assert_eq!(user.state, "lingering");
    }

    // --- PowerAction display ---

    #[test]
    fn test_power_action_as_str() {
        assert_eq!(PowerAction::PowerOff.as_str(), "poweroff");
        assert_eq!(PowerAction::Reboot.as_str(), "reboot");
        assert_eq!(PowerAction::Suspend.as_str(), "suspend");
        assert_eq!(PowerAction::Hibernate.as_str(), "hibernate");
    }
}
