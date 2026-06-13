//! Multi-personality capability management utility for SlateOS.
//!
//! This binary detects the tool personality from `argv[0]`:
//!   - `capsh`    — capability-aware shell wrapper and inspection tool
//!   - `getcap`   — get file capabilities from extended attributes
//!   - `setcap`   — set file capabilities on files
//!   - `getpcaps` — get process capabilities from /proc
//!   - `captest`  — test capability support on the running system
//!
//! SlateOS uses capability-based security; this toolset provides the POSIX/Linux
//! compatibility layer for inspecting and manipulating capabilities.

#![deny(clippy::all)]

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;

// ---------------------------------------------------------------------------
// Capability definitions
// ---------------------------------------------------------------------------

/// All Linux-compatible capability constants.  The numeric values match the
/// Linux kernel's `include/uapi/linux/capability.h`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
enum Cap {
    Chown = 0,
    DacOverride = 1,
    DacReadSearch = 2,
    Fowner = 3,
    Fsetid = 4,
    Kill = 5,
    Setgid = 6,
    Setuid = 7,
    Setpcap = 8,
    LinuxImmutable = 9,
    NetBindService = 10,
    NetBroadcast = 11,
    NetAdmin = 12,
    NetRaw = 13,
    IpcLock = 14,
    IpcOwner = 15,
    SysModule = 16,
    SysRawio = 17,
    SysChroot = 18,
    SysPtrace = 19,
    SysPacct = 20,
    SysAdmin = 21,
    SysBoot = 22,
    SysNice = 23,
    SysResource = 24,
    SysTime = 25,
    SysTtyConfig = 26,
    Mknod = 27,
    Lease = 28,
    AuditWrite = 29,
    AuditControl = 30,
    Setfcap = 31,
    MacOverride = 32,
    MacAdmin = 33,
    Syslog = 34,
    WakeAlarm = 35,
    BlockSuspend = 36,
    AuditRead = 37,
    Perfmon = 38,
    Bpf = 39,
    CheckpointRestore = 40,
}

/// Total number of defined capabilities.
const CAP_COUNT: usize = 41;

/// Ordered list of all capabilities for iteration.
const ALL_CAPS: [Cap; CAP_COUNT] = [
    Cap::Chown,
    Cap::DacOverride,
    Cap::DacReadSearch,
    Cap::Fowner,
    Cap::Fsetid,
    Cap::Kill,
    Cap::Setgid,
    Cap::Setuid,
    Cap::Setpcap,
    Cap::LinuxImmutable,
    Cap::NetBindService,
    Cap::NetBroadcast,
    Cap::NetAdmin,
    Cap::NetRaw,
    Cap::IpcLock,
    Cap::IpcOwner,
    Cap::SysModule,
    Cap::SysRawio,
    Cap::SysChroot,
    Cap::SysPtrace,
    Cap::SysPacct,
    Cap::SysAdmin,
    Cap::SysBoot,
    Cap::SysNice,
    Cap::SysResource,
    Cap::SysTime,
    Cap::SysTtyConfig,
    Cap::Mknod,
    Cap::Lease,
    Cap::AuditWrite,
    Cap::AuditControl,
    Cap::Setfcap,
    Cap::MacOverride,
    Cap::MacAdmin,
    Cap::Syslog,
    Cap::WakeAlarm,
    Cap::BlockSuspend,
    Cap::AuditRead,
    Cap::Perfmon,
    Cap::Bpf,
    Cap::CheckpointRestore,
];

impl Cap {
    /// Canonical lowercase name (matches Linux `/proc/*/status` format).
    fn name(self) -> &'static str {
        match self {
            Self::Chown => "cap_chown",
            Self::DacOverride => "cap_dac_override",
            Self::DacReadSearch => "cap_dac_read_search",
            Self::Fowner => "cap_fowner",
            Self::Fsetid => "cap_fsetid",
            Self::Kill => "cap_kill",
            Self::Setgid => "cap_setgid",
            Self::Setuid => "cap_setuid",
            Self::Setpcap => "cap_setpcap",
            Self::LinuxImmutable => "cap_linux_immutable",
            Self::NetBindService => "cap_net_bind_service",
            Self::NetBroadcast => "cap_net_broadcast",
            Self::NetAdmin => "cap_net_admin",
            Self::NetRaw => "cap_net_raw",
            Self::IpcLock => "cap_ipc_lock",
            Self::IpcOwner => "cap_ipc_owner",
            Self::SysModule => "cap_sys_module",
            Self::SysRawio => "cap_sys_rawio",
            Self::SysChroot => "cap_sys_chroot",
            Self::SysPtrace => "cap_sys_ptrace",
            Self::SysPacct => "cap_sys_pacct",
            Self::SysAdmin => "cap_sys_admin",
            Self::SysBoot => "cap_sys_boot",
            Self::SysNice => "cap_sys_nice",
            Self::SysResource => "cap_sys_resource",
            Self::SysTime => "cap_sys_time",
            Self::SysTtyConfig => "cap_sys_tty_config",
            Self::Mknod => "cap_mknod",
            Self::Lease => "cap_lease",
            Self::AuditWrite => "cap_audit_write",
            Self::AuditControl => "cap_audit_control",
            Self::Setfcap => "cap_setfcap",
            Self::MacOverride => "cap_mac_override",
            Self::MacAdmin => "cap_mac_admin",
            Self::Syslog => "cap_syslog",
            Self::WakeAlarm => "cap_wake_alarm",
            Self::BlockSuspend => "cap_block_suspend",
            Self::AuditRead => "cap_audit_read",
            Self::Perfmon => "cap_perfmon",
            Self::Bpf => "cap_bpf",
            Self::CheckpointRestore => "cap_checkpoint_restore",
        }
    }

    /// Bit index within the capability mask.
    fn bit(self) -> u64 {
        1u64 << (self as u8)
    }

    /// Look up a capability by its lowercase name (with or without `cap_` prefix).
    fn from_name(s: &str) -> Option<Self> {
        let lower = s.to_ascii_lowercase();
        let key = if let Some(stripped) = lower.strip_prefix("cap_") {
            stripped
        } else {
            &lower
        };
        for &cap in &ALL_CAPS {
            let cap_key = cap.name().strip_prefix("cap_").unwrap_or(cap.name());
            if cap_key == key {
                return Some(cap);
            }
        }
        None
    }

    /// Look up a capability by its numeric index.
    #[allow(dead_code)]
    fn from_index(idx: u8) -> Option<Self> {
        if (idx as usize) < CAP_COUNT {
            Some(ALL_CAPS[idx as usize])
        } else {
            None
        }
    }
}

impl fmt::Display for Cap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

// ---------------------------------------------------------------------------
// Capability mask — a 64-bit set of capabilities
// ---------------------------------------------------------------------------

/// A bitmask representing a set of capabilities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CapMask(u64);

impl CapMask {
    fn empty() -> Self {
        Self(0)
    }

    fn full() -> Self {
        let mut m = 0u64;
        for &cap in &ALL_CAPS {
            m |= cap.bit();
        }
        Self(m)
    }

    fn set(&mut self, cap: Cap) {
        self.0 |= cap.bit();
    }

    fn clear(&mut self, cap: Cap) {
        self.0 &= !cap.bit();
    }

    fn has(self, cap: Cap) -> bool {
        (self.0 & cap.bit()) != 0
    }

    fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[allow(dead_code)]
    fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[allow(dead_code)]
    fn intersect(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    #[allow(dead_code)]
    fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Hex-encode the mask (zero-padded to 16 hex digits).
    fn to_hex(self) -> String {
        format!("{:016x}", self.0)
    }

    /// Decode a hex string into a capability mask.
    fn from_hex(s: &str) -> Result<Self, String> {
        let s = s.strip_prefix("0x").unwrap_or(s);
        let s = s.strip_prefix("0X").unwrap_or(s);
        u64::from_str_radix(s, 16)
            .map(Self)
            .map_err(|e| format!("invalid hex '{}': {}", s, e))
    }

    /// Return a sorted list of capabilities present in this mask.
    fn to_cap_list(self) -> Vec<Cap> {
        let mut out = Vec::new();
        for &cap in &ALL_CAPS {
            if self.has(cap) {
                out.push(cap);
            }
        }
        out
    }

    /// Format the mask as a comma-separated list of capability names.
    fn to_names(self) -> String {
        let caps = self.to_cap_list();
        if caps.is_empty() {
            return String::from("(none)");
        }
        caps.iter()
            .map(|c| c.name())
            .collect::<Vec<_>>()
            .join(",")
    }
}

impl fmt::Display for CapMask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_names())
    }
}

// ---------------------------------------------------------------------------
// Process capability sets
// ---------------------------------------------------------------------------

/// The five per-process capability sets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ProcessCaps {
    effective: CapMask,
    permitted: CapMask,
    inheritable: CapMask,
    bounding: CapMask,
    ambient: CapMask,
}

impl ProcessCaps {
    fn new_full() -> Self {
        let full = CapMask::full();
        Self {
            effective: full,
            permitted: full,
            inheritable: CapMask::empty(),
            bounding: full,
            ambient: CapMask::empty(),
        }
    }
}

impl fmt::Display for ProcessCaps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Effective:   {}", self.effective)?;
        writeln!(f, "Permitted:   {}", self.permitted)?;
        writeln!(f, "Inheritable: {}", self.inheritable)?;
        writeln!(f, "Bounding:    {}", self.bounding)?;
        write!(f, "Ambient:     {}", self.ambient)
    }
}

// ---------------------------------------------------------------------------
// File capability sets
// ---------------------------------------------------------------------------

/// Capabilities stored on a file (via extended attributes).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct FileCaps {
    permitted: CapMask,
    inheritable: CapMask,
    effective: bool, // single flag, not a full mask
}

impl FileCaps {
    fn is_empty(&self) -> bool {
        self.permitted.is_empty() && self.inheritable.is_empty() && !self.effective
    }
}

impl fmt::Display for FileCaps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return write!(f, "(none)");
        }

        // Collect all unique caps that appear in any set.
        let mut all_caps: Vec<Cap> = Vec::new();
        for &cap in &ALL_CAPS {
            if self.permitted.has(cap) || self.inheritable.has(cap) {
                all_caps.push(cap);
            }
        }

        // Group caps by their set membership pattern for compact output.
        let mut groups: HashMap<(bool, bool), Vec<&str>> = HashMap::new();
        for &cap in &all_caps {
            let key = (self.permitted.has(cap), self.inheritable.has(cap));
            groups.entry(key).or_default().push(cap.name());
        }

        let mut parts = Vec::new();
        for (&(in_p, in_i), names) in &groups {
            let mut flags = String::new();
            if self.effective {
                flags.push('e');
            }
            if in_i {
                flags.push('i');
            }
            if in_p {
                flags.push('p');
            }
            let name_str = names.join(",");
            parts.push(format!("{}={}", name_str, flags));
        }

        // Sort for deterministic output.
        parts.sort();
        write!(f, "{}", parts.join(" "))
    }
}

// ---------------------------------------------------------------------------
// Securebits
// ---------------------------------------------------------------------------

const SECBIT_NOROOT: u32 = 1 << 0;
const SECBIT_NOROOT_LOCKED: u32 = 1 << 1;
const SECBIT_NO_SETUID_FIXUP: u32 = 1 << 2;
const SECBIT_NO_SETUID_FIXUP_LOCKED: u32 = 1 << 3;
const SECBIT_KEEP_CAPS: u32 = 1 << 4;
const SECBIT_KEEP_CAPS_LOCKED: u32 = 1 << 5;
const SECBIT_NO_CAP_AMBIENT_RAISE: u32 = 1 << 6;
const SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED: u32 = 1 << 7;

fn format_securebits(bits: u32) -> String {
    let mut flags = Vec::new();
    if bits & SECBIT_NOROOT != 0 {
        flags.push("NOROOT");
    }
    if bits & SECBIT_NOROOT_LOCKED != 0 {
        flags.push("NOROOT_LOCKED");
    }
    if bits & SECBIT_NO_SETUID_FIXUP != 0 {
        flags.push("NO_SETUID_FIXUP");
    }
    if bits & SECBIT_NO_SETUID_FIXUP_LOCKED != 0 {
        flags.push("NO_SETUID_FIXUP_LOCKED");
    }
    if bits & SECBIT_KEEP_CAPS != 0 {
        flags.push("KEEP_CAPS");
    }
    if bits & SECBIT_KEEP_CAPS_LOCKED != 0 {
        flags.push("KEEP_CAPS_LOCKED");
    }
    if bits & SECBIT_NO_CAP_AMBIENT_RAISE != 0 {
        flags.push("NO_CAP_AMBIENT_RAISE");
    }
    if bits & SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED != 0 {
        flags.push("NO_CAP_AMBIENT_RAISE_LOCKED");
    }
    if flags.is_empty() {
        String::from("0x00 (none)")
    } else {
        format!("0x{:02x} ({})", bits, flags.join("|"))
    }
}

// ---------------------------------------------------------------------------
// Capability modes
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CapMode {
    Uncertain,
    NoPriv,
    Pure1eInit,
}

impl CapMode {
    fn name(self) -> &'static str {
        match self {
            Self::Uncertain => "UNCERTAIN",
            Self::NoPriv => "NOPRIV",
            Self::Pure1eInit => "PURE1E_INIT",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Uncertain => "Unknown or mixed capability state",
            Self::NoPriv => "No privilege mode - all capabilities dropped",
            Self::Pure1eInit => "Pure capability mode - no UID-based privilege",
        }
    }

    fn from_name(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "UNCERTAIN" => Some(Self::Uncertain),
            "NOPRIV" => Some(Self::NoPriv),
            "PURE1E_INIT" => Some(Self::Pure1eInit),
            _ => None,
        }
    }

    fn securebits(self) -> u32 {
        match self {
            Self::Uncertain => 0,
            Self::NoPriv => {
                SECBIT_NOROOT
                    | SECBIT_NOROOT_LOCKED
                    | SECBIT_NO_SETUID_FIXUP
                    | SECBIT_NO_SETUID_FIXUP_LOCKED
                    | SECBIT_NO_CAP_AMBIENT_RAISE
                    | SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED
            }
            Self::Pure1eInit => {
                SECBIT_NOROOT
                    | SECBIT_NOROOT_LOCKED
                    | SECBIT_NO_SETUID_FIXUP
                    | SECBIT_NO_SETUID_FIXUP_LOCKED
                    | SECBIT_NO_CAP_AMBIENT_RAISE
                    | SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED
            }
        }
    }
}

const ALL_MODES: [CapMode; 3] = [CapMode::Uncertain, CapMode::NoPriv, CapMode::Pure1eInit];

// ---------------------------------------------------------------------------
// Capability specification parser
// ---------------------------------------------------------------------------

/// Parsed result of a single capability specification token.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CapSpec {
    caps: Vec<Cap>,
    op: CapSpecOp,
    sets: CapSpecSets,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CapSpecOp {
    Set,    // `=` — set exactly
    Add,    // `+` — add
    Remove, // `-` — remove
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CapSpecSets {
    effective: bool,
    inheritable: bool,
    permitted: bool,
}

/// Parse a capability specification string such as:
///   `cap_net_bind_service=ep`
///   `cap_sys_admin+eip`
///   `=ep cap_chown-p`
///   `cap_net_raw,cap_net_admin=ep`
fn parse_cap_spec(spec: &str) -> Result<Vec<CapSpec>, String> {
    let mut results = Vec::new();

    for token in spec.split_whitespace() {
        results.push(parse_single_cap_spec(token)?);
    }

    if results.is_empty() {
        return Err("empty capability specification".to_string());
    }

    Ok(results)
}

fn parse_single_cap_spec(token: &str) -> Result<CapSpec, String> {
    // Find the operator: `=`, `+`, or `-`
    let (op, op_pos) = find_operator(token)?;

    let cap_part = &token[..op_pos];
    let set_part = &token[op_pos + 1..];

    // Parse capabilities (before the operator).
    let caps = if cap_part.is_empty() {
        // `=ep` means all capabilities.
        ALL_CAPS.to_vec()
    } else {
        parse_cap_names(cap_part)?
    };

    // Parse set flags (after the operator).
    let sets = parse_set_flags(set_part)?;

    Ok(CapSpec { caps, op, sets })
}

fn find_operator(token: &str) -> Result<(CapSpecOp, usize), String> {
    // Scan from left; the first `=`, `+`, or `-` that is not part of a
    // capability name is the operator.  Capability names use `_` and
    // lowercase letters, so `+`, `-`, `=` are unambiguous.
    for (i, ch) in token.char_indices() {
        match ch {
            '=' => return Ok((CapSpecOp::Set, i)),
            '+' => return Ok((CapSpecOp::Add, i)),
            '-' if i > 0 => return Ok((CapSpecOp::Remove, i)),
            _ => {}
        }
    }
    Err(format!("no operator (=, +, -) found in cap spec '{}'", token))
}

fn parse_cap_names(s: &str) -> Result<Vec<Cap>, String> {
    let mut caps = Vec::new();
    for name in s.split(',') {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        match Cap::from_name(name) {
            Some(cap) => caps.push(cap),
            None => return Err(format!("unknown capability '{}'", name)),
        }
    }
    Ok(caps)
}

fn parse_set_flags(s: &str) -> Result<CapSpecSets, String> {
    let mut sets = CapSpecSets::default();
    for ch in s.chars() {
        match ch {
            'e' | 'E' => sets.effective = true,
            'i' | 'I' => sets.inheritable = true,
            'p' | 'P' => sets.permitted = true,
            _ => return Err(format!("unknown set flag '{}' (expected e/i/p)", ch)),
        }
    }
    Ok(sets)
}

/// Apply a list of cap specs to a `FileCaps` struct.
fn apply_cap_specs(specs: &[CapSpec], fcaps: &mut FileCaps) {
    for spec in specs {
        for &cap in &spec.caps {
            match spec.op {
                CapSpecOp::Set => {
                    // Clear first, then set specified flags.
                    if spec.sets.permitted {
                        fcaps.permitted.set(cap);
                    } else {
                        fcaps.permitted.clear(cap);
                    }
                    if spec.sets.inheritable {
                        fcaps.inheritable.set(cap);
                    } else {
                        fcaps.inheritable.clear(cap);
                    }
                    if spec.sets.effective {
                        fcaps.effective = true;
                    }
                }
                CapSpecOp::Add => {
                    if spec.sets.permitted {
                        fcaps.permitted.set(cap);
                    }
                    if spec.sets.inheritable {
                        fcaps.inheritable.set(cap);
                    }
                    if spec.sets.effective {
                        fcaps.effective = true;
                    }
                }
                CapSpecOp::Remove => {
                    if spec.sets.permitted {
                        fcaps.permitted.clear(cap);
                    }
                    if spec.sets.inheritable {
                        fcaps.inheritable.clear(cap);
                    }
                    // Effective is a single flag on files; removing from
                    // any cap clears it if no effective caps remain.
                    if spec.sets.effective {
                        // Only clear the flag if no caps will have it.
                        fcaps.effective = false;
                    }
                }
            }
        }
    }
}

/// Apply cap specs to process caps (for --caps/--inh in capsh).
fn apply_cap_specs_to_process(specs: &[CapSpec], pcaps: &mut ProcessCaps) {
    for spec in specs {
        for &cap in &spec.caps {
            match spec.op {
                CapSpecOp::Set => {
                    if spec.sets.effective {
                        pcaps.effective.set(cap);
                    } else {
                        pcaps.effective.clear(cap);
                    }
                    if spec.sets.permitted {
                        pcaps.permitted.set(cap);
                    } else {
                        pcaps.permitted.clear(cap);
                    }
                    if spec.sets.inheritable {
                        pcaps.inheritable.set(cap);
                    } else {
                        pcaps.inheritable.clear(cap);
                    }
                }
                CapSpecOp::Add => {
                    if spec.sets.effective {
                        pcaps.effective.set(cap);
                    }
                    if spec.sets.permitted {
                        pcaps.permitted.set(cap);
                    }
                    if spec.sets.inheritable {
                        pcaps.inheritable.set(cap);
                    }
                }
                CapSpecOp::Remove => {
                    if spec.sets.effective {
                        pcaps.effective.clear(cap);
                    }
                    if spec.sets.permitted {
                        pcaps.permitted.clear(cap);
                    }
                    if spec.sets.inheritable {
                        pcaps.inheritable.clear(cap);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Simulated xattr storage
// ---------------------------------------------------------------------------

/// Path to the xattr sidecar directory for simulating `security.capability`.
fn xattr_dir() -> PathBuf {
    let base = env::var("SLATEOS_XATTR_DIR")
        .unwrap_or_else(|_| String::from("/var/lib/slateos/xattrs"));
    PathBuf::from(base)
}

/// Compute the sidecar path for a given file.
fn xattr_path_for(file: &Path) -> PathBuf {
    // Use a hash-like encoding of the absolute path to avoid collisions.
    let abs = fs::canonicalize(file)
        .unwrap_or_else(|_| file.to_path_buf());
    let encoded = abs
        .to_string_lossy()
        .replace('/', "_SLASH_")
        .replace('\\', "_BSLASH_")
        .replace(':', "_COLON_");
    xattr_dir().join(format!("{}.cap", encoded))
}

/// Serialise file caps to a sidecar file.
fn write_file_caps(file: &Path, caps: &FileCaps) -> io::Result<()> {
    let xp = xattr_path_for(file);
    if let Some(parent) = xp.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = format!(
        "{}\n{}\n{}\n",
        caps.permitted.to_hex(),
        caps.inheritable.to_hex(),
        if caps.effective { "1" } else { "0" }
    );
    fs::write(&xp, data)
}

/// Read file caps from the sidecar file.
fn read_file_caps(file: &Path) -> io::Result<Option<FileCaps>> {
    let xp = xattr_path_for(file);
    let data = match fs::read_to_string(&xp) {
        Ok(d) => d,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };

    let lines: Vec<&str> = data.lines().collect();
    if lines.len() < 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "malformed capability sidecar file",
        ));
    }

    let permitted = CapMask::from_hex(lines[0])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let inheritable = CapMask::from_hex(lines[1])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let effective = lines[2] == "1";

    Ok(Some(FileCaps {
        permitted,
        inheritable,
        effective,
    }))
}

/// Remove file caps sidecar.
fn remove_file_caps(file: &Path) -> io::Result<()> {
    let xp = xattr_path_for(file);
    match fs::remove_file(&xp) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// /proc-style process capability reader
// ---------------------------------------------------------------------------

/// Read process capabilities from `/proc/<pid>/status`.
fn read_proc_caps(pid: &str) -> Result<ProcessCaps, String> {
    let path = format!("/proc/{}/status", pid);
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {}", path, e))?;

    let mut caps = ProcessCaps::default();

    for line in content.lines() {
        if let Some(hex) = line.strip_prefix("CapInh:\t") {
            caps.inheritable = CapMask::from_hex(hex.trim())?;
        } else if let Some(hex) = line.strip_prefix("CapPrm:\t") {
            caps.permitted = CapMask::from_hex(hex.trim())?;
        } else if let Some(hex) = line.strip_prefix("CapEff:\t") {
            caps.effective = CapMask::from_hex(hex.trim())?;
        } else if let Some(hex) = line.strip_prefix("CapBnd:\t") {
            caps.bounding = CapMask::from_hex(hex.trim())?;
        } else if let Some(hex) = line.strip_prefix("CapAmb:\t") {
            caps.ambient = CapMask::from_hex(hex.trim())?;
        }
    }

    Ok(caps)
}

// ---------------------------------------------------------------------------
// Simulated process state (for capsh environment manipulation)
// ---------------------------------------------------------------------------

/// Mutable state that capsh manipulates before exec.
struct CapshState {
    caps: ProcessCaps,
    securebits: u32,
    uid: u32,
    gid: u32,
    groups: Vec<u32>,
    chroot: Option<String>,
    user: Option<String>,
    mode: CapMode,
}

impl CapshState {
    fn new() -> Self {
        Self {
            caps: ProcessCaps::new_full(),
            securebits: 0,
            uid: 0,
            gid: 0,
            groups: Vec::new(),
            chroot: None,
            user: None,
            mode: CapMode::Uncertain,
        }
    }

    fn print(&self) {
        println!("Current cap state:");
        println!("{}", self.caps);
        println!("Securebits: {}", format_securebits(self.securebits));
        println!("uid={} gid={}", self.uid, self.gid);
        if self.groups.is_empty() {
            println!("groups=");
        } else {
            let gs: Vec<String> = self.groups.iter().map(|g| g.to_string()).collect();
            println!("groups={}", gs.join(","));
        }
        if let Some(ref u) = self.user {
            println!("user={}", u);
        }
        if let Some(ref c) = self.chroot {
            println!("chroot={}", c);
        }
        println!("Mode: {}", self.mode.name());
    }
}

// ---------------------------------------------------------------------------
// Personality: capsh
// ---------------------------------------------------------------------------

fn run_capsh(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("capsh: use --print, --help, or -- <command>");
        return 1;
    }

    let mut state = CapshState::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        if arg == "--help" || arg == "-h" {
            print_capsh_help();
            return 0;
        } else if arg == "--print" {
            state.print();
        } else if let Some(hex) = arg.strip_prefix("--decode=") {
            match CapMask::from_hex(hex) {
                Ok(mask) => println!("{} = {}", hex, mask.to_names()),
                Err(e) => {
                    eprintln!("capsh: --decode: {}", e);
                    return 1;
                }
            }
        } else if let Some(spec) = arg.strip_prefix("--caps=") {
            match parse_cap_spec(spec) {
                Ok(specs) => apply_cap_specs_to_process(&specs, &mut state.caps),
                Err(e) => {
                    eprintln!("capsh: --caps: {}", e);
                    return 1;
                }
            }
        } else if let Some(cap_name) = arg.strip_prefix("--drop=") {
            match Cap::from_name(cap_name) {
                Some(cap) => state.caps.bounding.clear(cap),
                None => {
                    eprintln!("capsh: --drop: unknown capability '{}'", cap_name);
                    return 1;
                }
            }
        } else if let Some(cap_name) = arg.strip_prefix("--addamb=") {
            match Cap::from_name(cap_name) {
                Some(cap) => {
                    // Ambient can only be raised if the cap is in both
                    // permitted and inheritable sets.
                    if !state.caps.permitted.has(cap) || !state.caps.inheritable.has(cap) {
                        eprintln!(
                            "capsh: --addamb: {} must be in both permitted and inheritable sets",
                            cap_name
                        );
                        return 1;
                    }
                    state.caps.ambient.set(cap);
                }
                None => {
                    eprintln!("capsh: --addamb: unknown capability '{}'", cap_name);
                    return 1;
                }
            }
        } else if let Some(cap_name) = arg.strip_prefix("--delamb=") {
            match Cap::from_name(cap_name) {
                Some(cap) => state.caps.ambient.clear(cap),
                None => {
                    eprintln!("capsh: --delamb: unknown capability '{}'", cap_name);
                    return 1;
                }
            }
        } else if let Some(spec) = arg.strip_prefix("--inh=") {
            match parse_cap_spec(spec) {
                Ok(specs) => {
                    // Apply only to inheritable set.
                    for s in &specs {
                        for &cap in &s.caps {
                            match s.op {
                                CapSpecOp::Set => {
                                    if s.sets.inheritable || s.sets.effective || s.sets.permitted {
                                        state.caps.inheritable.set(cap);
                                    } else {
                                        state.caps.inheritable.clear(cap);
                                    }
                                }
                                CapSpecOp::Add => state.caps.inheritable.set(cap),
                                CapSpecOp::Remove => state.caps.inheritable.clear(cap),
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("capsh: --inh: {}", e);
                    return 1;
                }
            }
        } else if let Some(val) = arg.strip_prefix("--uid=") {
            match val.parse::<u32>() {
                Ok(uid) => state.uid = uid,
                Err(_) => {
                    eprintln!("capsh: --uid: invalid uid '{}'", val);
                    return 1;
                }
            }
        } else if let Some(val) = arg.strip_prefix("--gid=") {
            match val.parse::<u32>() {
                Ok(gid) => state.gid = gid,
                Err(_) => {
                    eprintln!("capsh: --gid: invalid gid '{}'", val);
                    return 1;
                }
            }
        } else if let Some(name) = arg.strip_prefix("--user=") {
            state.user = Some(name.to_string());
            // In a real OS, this would do getpwnam() and set uid/gid/groups.
            // Here we simulate for the compatibility layer.
            println!("capsh: switching to user '{}'", name);
        } else if let Some(dir) = arg.strip_prefix("--chroot=") {
            state.chroot = Some(dir.to_string());
            println!("capsh: would chroot to '{}'", dir);
        } else if let Some(hex) = arg.strip_prefix("--secbits=") {
            match u32::from_str_radix(
                hex.strip_prefix("0x").unwrap_or(hex.strip_prefix("0X").unwrap_or(hex)),
                16,
            ) {
                Ok(bits) => state.securebits = bits,
                Err(_) => {
                    eprintln!("capsh: --secbits: invalid hex '{}'", hex);
                    return 1;
                }
            }
        } else if let Some(val) = arg.strip_prefix("--keep=") {
            match val {
                "0" => state.securebits &= !SECBIT_KEEP_CAPS,
                "1" => state.securebits |= SECBIT_KEEP_CAPS,
                _ => {
                    eprintln!("capsh: --keep: expected 0 or 1, got '{}'", val);
                    return 1;
                }
            }
        } else if arg == "--modes" {
            println!("Supported capability modes:");
            for mode in &ALL_MODES {
                println!("  {:12} — {}", mode.name(), mode.description());
            }
        } else if let Some(name) = arg.strip_prefix("--mode=") {
            match CapMode::from_name(name) {
                Some(mode) => {
                    state.mode = mode;
                    state.securebits = mode.securebits();
                    match mode {
                        CapMode::NoPriv => {
                            state.caps.effective = CapMask::empty();
                            state.caps.permitted = CapMask::empty();
                            state.caps.inheritable = CapMask::empty();
                            state.caps.ambient = CapMask::empty();
                            state.caps.bounding = CapMask::empty();
                        }
                        CapMode::Pure1eInit => {
                            state.caps.ambient = CapMask::empty();
                        }
                        CapMode::Uncertain => {}
                    }
                    println!("capsh: set mode to {}", mode.name());
                }
                None => {
                    eprintln!("capsh: --mode: unknown mode '{}'", name);
                    return 1;
                }
            }
        } else if arg == "--noamb" {
            state.caps.ambient = CapMask::empty();
        } else if arg == "--" {
            // Everything after `--` is a command to exec.
            let cmd_args: Vec<&str> = args[i + 1..].iter().map(|s| s.as_str()).collect();
            if cmd_args.is_empty() {
                eprintln!("capsh: -- requires a command");
                return 1;
            }
            // In the real OS, we would apply all state changes and exec.
            // For now, report what would happen.
            println!("capsh: would exec {:?}", cmd_args);
            println!("  with capabilities:");
            state.print();
            return 0;
        } else {
            eprintln!("capsh: unknown option '{}'", arg);
            eprintln!("Try 'capsh --help' for more information.");
            return 1;
        }

        i += 1;
    }

    0
}

fn print_capsh_help() {
    println!(
        "\
capsh — capability shell wrapper and inspection tool

Usage: capsh [OPTIONS] [-- COMMAND [ARGS...]]

Options:
  --print              Display current capability state
  --decode=<hex>       Decode a hex capability mask to names
  --caps=<capspec>     Set process capabilities
  --drop=<cap>         Drop capability from bounding set
  --addamb=<cap>       Add capability to ambient set
  --delamb=<cap>       Remove capability from ambient set
  --inh=<capspec>      Set inheritable capabilities
  --uid=<n>            Set UID
  --gid=<n>            Set GID
  --user=<name>        Switch user (setuid/setgid/initgroups)
  --chroot=<dir>       Change root directory before exec
  --secbits=<hex>      Set securebits
  --keep=<0|1>         Set/clear SECBIT_KEEP_CAPS
  --modes              List supported capability modes
  --mode=<mode>        Set capability mode (NOPRIV/PURE1E_INIT/UNCERTAIN)
  --noamb              Clear all ambient capabilities
  --                   Execute remaining args as command
  --help               Show this help

Capability specification format:
  cap_name=flags       Set caps (flags: e=effective, i=inheritable, p=permitted)
  cap_name+flags       Add caps to specified sets
  cap_name-flags       Remove caps from specified sets
  =flags               Apply to all capabilities
  Multiple caps:       cap_a,cap_b=ep"
    );
}

// ---------------------------------------------------------------------------
// Personality: getcap
// ---------------------------------------------------------------------------

fn run_getcap(args: &[String]) -> i32 {
    let mut recursive = false;
    let mut verbose = false;
    let mut numeric = false;
    let mut files: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-r" => recursive = true,
            "-v" => verbose = true,
            "-n" => numeric = true,
            "--help" | "-h" => {
                println!("Usage: getcap [-r] [-v] [-n] <file...>");
                println!("  -r  Recursive");
                println!("  -v  Verbose");
                println!("  -n  Numeric output");
                return 0;
            }
            other => files.push(other.to_string()),
        }
        i += 1;
    }

    if files.is_empty() {
        eprintln!("getcap: missing file argument");
        return 1;
    }

    let mut exit_code = 0;
    for file in &files {
        let path = Path::new(file);
        if recursive && path.is_dir() {
            if let Err(e) = getcap_recursive(path, verbose, numeric) {
                eprintln!("getcap: {}: {}", file, e);
                exit_code = 1;
            }
        } else {
            match getcap_one(path, verbose, numeric) {
                Ok(()) => {}
                Err(e) => {
                    if verbose {
                        eprintln!("getcap: {}: {}", file, e);
                    }
                    exit_code = 1;
                }
            }
        }
    }

    exit_code
}

fn getcap_one(path: &Path, verbose: bool, numeric: bool) -> io::Result<()> {
    match read_file_caps(path)? {
        Some(caps) if !caps.is_empty() => {
            if numeric {
                println!(
                    "{} p={} i={} e={}",
                    path.display(),
                    caps.permitted.to_hex(),
                    caps.inheritable.to_hex(),
                    if caps.effective { "1" } else { "0" }
                );
            } else {
                println!("{} {}", path.display(), caps);
            }
            Ok(())
        }
        Some(_) | None => {
            if verbose {
                println!("{} (no capabilities)", path.display());
            }
            Ok(())
        }
    }
}

fn getcap_recursive(dir: &Path, verbose: bool, numeric: bool) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            getcap_recursive(&path, verbose, numeric)?;
        } else {
            // Ignore errors on individual files in recursive mode.
            let _ = getcap_one(&path, verbose, numeric);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Personality: setcap
// ---------------------------------------------------------------------------

fn run_setcap(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: setcap [-r] <capspec> <file>");
        return 1;
    }

    // Check for -r (remove) mode.
    if args.first().map(|s| s.as_str()) == Some("-r") {
        if args.len() < 2 {
            eprintln!("setcap: -r requires a file argument");
            return 1;
        }
        let mut exit_code = 0;
        for file in &args[1..] {
            let path = Path::new(file);
            if let Err(e) = remove_file_caps(path) {
                eprintln!("setcap: {}: {}", file, e);
                exit_code = 1;
            } else {
                println!("setcap: removed capabilities from {}", file);
            }
        }
        return exit_code;
    }

    if args.first().map(|s| s.as_str()) == Some("--help")
        || args.first().map(|s| s.as_str()) == Some("-h")
    {
        println!("Usage: setcap <capspec> <file>");
        println!("       setcap -r <file>        Remove capabilities");
        println!();
        println!("Cap spec examples:");
        println!("  cap_net_bind_service=ep");
        println!("  cap_sys_admin+eip");
        println!("  =ep cap_chown-p");
        return 0;
    }

    if args.len() < 2 {
        eprintln!("setcap: need <capspec> and <file>");
        return 1;
    }

    // The last argument is the file; everything before is the cap spec.
    let file = &args[args.len() - 1];
    let spec_parts: Vec<&str> = args[..args.len() - 1].iter().map(|s| s.as_str()).collect();
    let spec_str = spec_parts.join(" ");

    let specs = match parse_cap_spec(&spec_str) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("setcap: {}", e);
            return 1;
        }
    };

    let path = Path::new(file);
    if !path.exists() {
        eprintln!("setcap: {}: No such file", file);
        return 1;
    }

    // Read existing caps, apply spec, write back.
    let mut fcaps = match read_file_caps(path) {
        Ok(Some(c)) => c,
        Ok(None) => FileCaps::default(),
        Err(e) => {
            eprintln!("setcap: {}: {}", file, e);
            return 1;
        }
    };

    apply_cap_specs(&specs, &mut fcaps);

    if let Err(e) = write_file_caps(path, &fcaps) {
        eprintln!("setcap: {}: {}", file, e);
        return 1;
    }

    0
}

// ---------------------------------------------------------------------------
// Personality: getpcaps
// ---------------------------------------------------------------------------

fn run_getpcaps(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: getpcaps <pid...>");
        return 1;
    }

    if args.first().map(|s| s.as_str()) == Some("--help")
        || args.first().map(|s| s.as_str()) == Some("-h")
    {
        println!("Usage: getpcaps <pid...>");
        println!("Display capabilities for one or more processes.");
        return 0;
    }

    let mut exit_code = 0;
    for pid in args {
        // Validate pid is numeric.
        if pid.parse::<u64>().is_err() {
            eprintln!("getpcaps: invalid pid '{}'", pid);
            exit_code = 1;
            continue;
        }

        match read_proc_caps(pid) {
            Ok(caps) => {
                println!("Process {} capabilities:", pid);
                println!("{}", caps);
                println!();
            }
            Err(e) => {
                eprintln!("getpcaps: pid {}: {}", pid, e);
                exit_code = 1;
            }
        }
    }

    exit_code
}

// ---------------------------------------------------------------------------
// Personality: captest
// ---------------------------------------------------------------------------

/// Capability test result.
struct CapTestResult {
    cap: Cap,
    has_it: bool,
    test_desc: &'static str,
}

fn run_captest(args: &[String]) -> i32 {
    if args.first().map(|s| s.as_str()) == Some("--help")
        || args.first().map(|s| s.as_str()) == Some("-h")
    {
        println!("Usage: captest");
        println!("Test capability support and report which capabilities are available.");
        return 0;
    }

    println!("=== Capability Support Test ===");
    println!();

    // On SlateOS, we test against the simulated process capability state.
    // In a real deployment this would use actual syscalls.
    let caps = ProcessCaps::new_full();

    let tests = build_cap_tests(&caps);

    let mut pass_count = 0;
    let mut fail_count = 0;

    for result in &tests {
        if result.has_it {
            println!("PASS: {} — {}", result.cap.name(), result.test_desc);
            pass_count += 1;
        } else {
            println!("FAIL: {} — {}", result.cap.name(), result.test_desc);
            fail_count += 1;
        }
    }

    println!();
    println!("Summary: {} passed, {} failed, {} total", pass_count, fail_count, tests.len());

    if fail_count > 0 { 1 } else { 0 }
}

fn build_cap_tests(caps: &ProcessCaps) -> Vec<CapTestResult> {
    let descs: [(Cap, &'static str); CAP_COUNT] = [
        (Cap::Chown, "change file ownership"),
        (Cap::DacOverride, "override file read/write/execute permission checks"),
        (Cap::DacReadSearch, "override read/search permission on directories"),
        (Cap::Fowner, "bypass permission checks on file owner operations"),
        (Cap::Fsetid, "set setuid/setgid bits on files"),
        (Cap::Kill, "send signals to arbitrary processes"),
        (Cap::Setgid, "set process GID"),
        (Cap::Setuid, "set process UID"),
        (Cap::Setpcap, "modify process capabilities"),
        (Cap::LinuxImmutable, "set immutable file attribute"),
        (Cap::NetBindService, "bind to ports below 1024"),
        (Cap::NetBroadcast, "send broadcast/multicast packets"),
        (Cap::NetAdmin, "perform network administration tasks"),
        (Cap::NetRaw, "use raw sockets"),
        (Cap::IpcLock, "lock memory (mlock)"),
        (Cap::IpcOwner, "bypass IPC ownership checks"),
        (Cap::SysModule, "load kernel modules"),
        (Cap::SysRawio, "perform raw I/O operations"),
        (Cap::SysChroot, "call chroot()"),
        (Cap::SysPtrace, "trace arbitrary processes"),
        (Cap::SysPacct, "configure process accounting"),
        (Cap::SysAdmin, "perform system administration tasks"),
        (Cap::SysBoot, "reboot the system"),
        (Cap::SysNice, "set process scheduling priority"),
        (Cap::SysResource, "override resource limits"),
        (Cap::SysTime, "set system clock"),
        (Cap::SysTtyConfig, "configure TTY devices"),
        (Cap::Mknod, "create special device files"),
        (Cap::Lease, "establish file leases"),
        (Cap::AuditWrite, "write to audit log"),
        (Cap::AuditControl, "configure audit subsystem"),
        (Cap::Setfcap, "set file capabilities"),
        (Cap::MacOverride, "override MAC policy"),
        (Cap::MacAdmin, "administer MAC configuration"),
        (Cap::Syslog, "access kernel syslog"),
        (Cap::WakeAlarm, "set wake alarm"),
        (Cap::BlockSuspend, "block system suspend"),
        (Cap::AuditRead, "read audit log"),
        (Cap::Perfmon, "access performance monitoring"),
        (Cap::Bpf, "use BPF operations"),
        (Cap::CheckpointRestore, "checkpoint/restore operations"),
    ];

    descs
        .iter()
        .map(|&(cap, desc)| CapTestResult {
            cap,
            has_it: caps.effective.has(cap),
            test_desc: desc,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Personality detection and main dispatch
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("capsh");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match prog_name.as_str() {
        "getcap" => run_getcap(&rest),
        "setcap" => run_setcap(&rest),
        "getpcaps" => run_getpcaps(&rest),
        "captest" => run_captest(&rest),
        _ => run_capsh(&rest), // default: capsh
    };

    process::exit(exit_code);
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Cap enum tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_cap_count() {
        assert_eq!(ALL_CAPS.len(), CAP_COUNT);
    }

    #[test]
    fn test_cap_indices_sequential() {
        for (i, &cap) in ALL_CAPS.iter().enumerate() {
            assert_eq!(cap as u8, i as u8, "cap {:?} index mismatch", cap);
        }
    }

    #[test]
    fn test_cap_names_all_start_with_cap() {
        for &cap in &ALL_CAPS {
            assert!(
                cap.name().starts_with("cap_"),
                "cap {:?} name '{}' does not start with cap_",
                cap,
                cap.name()
            );
        }
    }

    #[test]
    fn test_cap_names_all_lowercase() {
        for &cap in &ALL_CAPS {
            assert_eq!(
                cap.name(),
                cap.name().to_ascii_lowercase(),
                "cap {:?} name is not lowercase",
                cap
            );
        }
    }

    #[test]
    fn test_cap_from_name_all() {
        for &cap in &ALL_CAPS {
            assert_eq!(
                Cap::from_name(cap.name()),
                Some(cap),
                "from_name failed for {}",
                cap.name()
            );
        }
    }

    #[test]
    fn test_cap_from_name_without_prefix() {
        assert_eq!(Cap::from_name("chown"), Some(Cap::Chown));
        assert_eq!(Cap::from_name("sys_admin"), Some(Cap::SysAdmin));
        assert_eq!(Cap::from_name("net_raw"), Some(Cap::NetRaw));
    }

    #[test]
    fn test_cap_from_name_case_insensitive() {
        assert_eq!(Cap::from_name("CAP_CHOWN"), Some(Cap::Chown));
        assert_eq!(Cap::from_name("Cap_Sys_Admin"), Some(Cap::SysAdmin));
        assert_eq!(Cap::from_name("NET_RAW"), Some(Cap::NetRaw));
    }

    #[test]
    fn test_cap_from_name_unknown() {
        assert_eq!(Cap::from_name("cap_nonexistent"), None);
        assert_eq!(Cap::from_name(""), None);
        assert_eq!(Cap::from_name("bogus"), None);
    }

    #[test]
    fn test_cap_from_index_valid() {
        for i in 0..CAP_COUNT {
            let cap = Cap::from_index(i as u8);
            assert!(cap.is_some(), "from_index({}) returned None", i);
            assert_eq!(cap.unwrap() as u8, i as u8);
        }
    }

    #[test]
    fn test_cap_from_index_invalid() {
        assert_eq!(Cap::from_index(CAP_COUNT as u8), None);
        assert_eq!(Cap::from_index(255), None);
    }

    #[test]
    fn test_cap_bit_unique() {
        let mut seen = std::collections::HashSet::new();
        for &cap in &ALL_CAPS {
            assert!(
                seen.insert(cap.bit()),
                "duplicate bit for {:?}",
                cap
            );
        }
    }

    #[test]
    fn test_cap_bit_single_bit_set() {
        for &cap in &ALL_CAPS {
            assert_eq!(
                cap.bit().count_ones(),
                1,
                "bit for {:?} is not a single bit",
                cap
            );
        }
    }

    #[test]
    fn test_cap_display() {
        assert_eq!(format!("{}", Cap::Chown), "cap_chown");
        assert_eq!(format!("{}", Cap::SysAdmin), "cap_sys_admin");
    }

    // -----------------------------------------------------------------------
    // CapMask tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_mask_empty() {
        let m = CapMask::empty();
        assert!(m.is_empty());
        assert_eq!(m.0, 0);
    }

    #[test]
    fn test_mask_full_has_all() {
        let m = CapMask::full();
        for &cap in &ALL_CAPS {
            assert!(m.has(cap), "full mask missing {:?}", cap);
        }
    }

    #[test]
    fn test_mask_full_not_empty() {
        assert!(!CapMask::full().is_empty());
    }

    #[test]
    fn test_mask_set_and_has() {
        let mut m = CapMask::empty();
        assert!(!m.has(Cap::Chown));
        m.set(Cap::Chown);
        assert!(m.has(Cap::Chown));
        assert!(!m.has(Cap::Kill));
    }

    #[test]
    fn test_mask_clear() {
        let mut m = CapMask::full();
        assert!(m.has(Cap::SysAdmin));
        m.clear(Cap::SysAdmin);
        assert!(!m.has(Cap::SysAdmin));
        // Other caps still set.
        assert!(m.has(Cap::Chown));
    }

    #[test]
    fn test_mask_set_idempotent() {
        let mut m = CapMask::empty();
        m.set(Cap::Kill);
        m.set(Cap::Kill);
        assert!(m.has(Cap::Kill));
        assert_eq!(m.0, Cap::Kill.bit());
    }

    #[test]
    fn test_mask_clear_idempotent() {
        let mut m = CapMask::empty();
        m.clear(Cap::Kill); // already empty
        assert!(!m.has(Cap::Kill));
    }

    #[test]
    fn test_mask_union() {
        let mut a = CapMask::empty();
        a.set(Cap::Chown);
        let mut b = CapMask::empty();
        b.set(Cap::Kill);
        let c = a.union(b);
        assert!(c.has(Cap::Chown));
        assert!(c.has(Cap::Kill));
    }

    #[test]
    fn test_mask_intersect() {
        let mut a = CapMask::empty();
        a.set(Cap::Chown);
        a.set(Cap::Kill);
        let mut b = CapMask::empty();
        b.set(Cap::Kill);
        b.set(Cap::Setuid);
        let c = a.intersect(b);
        assert!(!c.has(Cap::Chown));
        assert!(c.has(Cap::Kill));
        assert!(!c.has(Cap::Setuid));
    }

    #[test]
    fn test_mask_difference() {
        let mut a = CapMask::empty();
        a.set(Cap::Chown);
        a.set(Cap::Kill);
        let mut b = CapMask::empty();
        b.set(Cap::Kill);
        let c = a.difference(b);
        assert!(c.has(Cap::Chown));
        assert!(!c.has(Cap::Kill));
    }

    #[test]
    fn test_mask_to_hex_empty() {
        assert_eq!(CapMask::empty().to_hex(), "0000000000000000");
    }

    #[test]
    fn test_mask_to_hex_chown() {
        let mut m = CapMask::empty();
        m.set(Cap::Chown);
        assert_eq!(m.to_hex(), "0000000000000001");
    }

    #[test]
    fn test_mask_to_hex_full() {
        let full = CapMask::full();
        let hex = full.to_hex();
        assert_eq!(hex.len(), 16);
        // All 41 bits set: (1 << 41) - 1 = 0x1ffffffffff
        assert_eq!(hex, "000001ffffffffff");
    }

    #[test]
    fn test_mask_from_hex_zero() {
        assert_eq!(CapMask::from_hex("0"), Ok(CapMask(0)));
    }

    #[test]
    fn test_mask_from_hex_with_prefix() {
        assert_eq!(CapMask::from_hex("0x1"), Ok(CapMask(1)));
        assert_eq!(CapMask::from_hex("0X1"), Ok(CapMask(1)));
    }

    #[test]
    fn test_mask_from_hex_roundtrip() {
        let m = CapMask(0x0000_01ff_ffff_ffff);
        let hex = m.to_hex();
        assert_eq!(CapMask::from_hex(&hex), Ok(m));
    }

    #[test]
    fn test_mask_from_hex_invalid() {
        assert!(CapMask::from_hex("not_hex").is_err());
        assert!(CapMask::from_hex("gggg").is_err());
    }

    #[test]
    fn test_mask_to_cap_list_empty() {
        assert!(CapMask::empty().to_cap_list().is_empty());
    }

    #[test]
    fn test_mask_to_cap_list_single() {
        let mut m = CapMask::empty();
        m.set(Cap::NetRaw);
        let list = m.to_cap_list();
        assert_eq!(list, vec![Cap::NetRaw]);
    }

    #[test]
    fn test_mask_to_cap_list_order() {
        let mut m = CapMask::empty();
        m.set(Cap::SysAdmin);
        m.set(Cap::Chown);
        let list = m.to_cap_list();
        // Should be in enum order (Chown=0 before SysAdmin=21).
        assert_eq!(list, vec![Cap::Chown, Cap::SysAdmin]);
    }

    #[test]
    fn test_mask_to_names_empty() {
        assert_eq!(CapMask::empty().to_names(), "(none)");
    }

    #[test]
    fn test_mask_to_names_single() {
        let mut m = CapMask::empty();
        m.set(Cap::Chown);
        assert_eq!(m.to_names(), "cap_chown");
    }

    #[test]
    fn test_mask_to_names_multiple() {
        let mut m = CapMask::empty();
        m.set(Cap::Chown);
        m.set(Cap::Kill);
        assert_eq!(m.to_names(), "cap_chown,cap_kill");
    }

    #[test]
    fn test_mask_display() {
        let mut m = CapMask::empty();
        m.set(Cap::Chown);
        assert_eq!(format!("{}", m), "cap_chown");
    }

    // -----------------------------------------------------------------------
    // ProcessCaps tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_process_caps_default() {
        let p = ProcessCaps::default();
        assert!(p.effective.is_empty());
        assert!(p.permitted.is_empty());
        assert!(p.inheritable.is_empty());
        assert!(p.bounding.is_empty());
        assert!(p.ambient.is_empty());
    }

    #[test]
    fn test_process_caps_new_full() {
        let p = ProcessCaps::new_full();
        let full = CapMask::full();
        assert_eq!(p.effective, full);
        assert_eq!(p.permitted, full);
        assert!(p.inheritable.is_empty());
        assert_eq!(p.bounding, full);
        assert!(p.ambient.is_empty());
    }

    #[test]
    fn test_process_caps_display() {
        let p = ProcessCaps::default();
        let s = format!("{}", p);
        assert!(s.contains("Effective:"));
        assert!(s.contains("Permitted:"));
        assert!(s.contains("Inheritable:"));
        assert!(s.contains("Bounding:"));
        assert!(s.contains("Ambient:"));
    }

    // -----------------------------------------------------------------------
    // FileCaps tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_file_caps_empty() {
        let f = FileCaps::default();
        assert!(f.is_empty());
    }

    #[test]
    fn test_file_caps_not_empty_with_permitted() {
        let mut f = FileCaps::default();
        f.permitted.set(Cap::Chown);
        assert!(!f.is_empty());
    }

    #[test]
    fn test_file_caps_not_empty_with_inheritable() {
        let mut f = FileCaps::default();
        f.inheritable.set(Cap::Kill);
        assert!(!f.is_empty());
    }

    #[test]
    fn test_file_caps_not_empty_with_effective() {
        let f = FileCaps { effective: true, ..FileCaps::default() };
        // effective flag alone does not make it non-empty if masks are empty
        // Actually per our definition it does: effective=true counts.
        assert!(!f.is_empty());
    }

    #[test]
    fn test_file_caps_display_empty() {
        let f = FileCaps::default();
        assert_eq!(format!("{}", f), "(none)");
    }

    #[test]
    fn test_file_caps_display_with_caps() {
        let mut f = FileCaps::default();
        f.permitted.set(Cap::NetBindService);
        f.effective = true;
        let s = format!("{}", f);
        assert!(s.contains("cap_net_bind_service"));
        assert!(s.contains('e'));
        assert!(s.contains('p'));
    }

    // -----------------------------------------------------------------------
    // Securebits tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_securebits_zero() {
        assert_eq!(format_securebits(0), "0x00 (none)");
    }

    #[test]
    fn test_securebits_noroot() {
        let s = format_securebits(SECBIT_NOROOT);
        assert!(s.contains("NOROOT"));
    }

    #[test]
    fn test_securebits_keep_caps() {
        let s = format_securebits(SECBIT_KEEP_CAPS);
        assert!(s.contains("KEEP_CAPS"));
    }

    #[test]
    fn test_securebits_combined() {
        let bits = SECBIT_NOROOT | SECBIT_NO_SETUID_FIXUP;
        let s = format_securebits(bits);
        assert!(s.contains("NOROOT"));
        assert!(s.contains("NO_SETUID_FIXUP"));
    }

    #[test]
    fn test_securebits_all() {
        let bits = SECBIT_NOROOT
            | SECBIT_NOROOT_LOCKED
            | SECBIT_NO_SETUID_FIXUP
            | SECBIT_NO_SETUID_FIXUP_LOCKED
            | SECBIT_KEEP_CAPS
            | SECBIT_KEEP_CAPS_LOCKED
            | SECBIT_NO_CAP_AMBIENT_RAISE
            | SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED;
        let s = format_securebits(bits);
        assert!(s.contains("NOROOT"));
        assert!(s.contains("NOROOT_LOCKED"));
        assert!(s.contains("KEEP_CAPS"));
        assert!(s.contains("NO_CAP_AMBIENT_RAISE"));
    }

    // -----------------------------------------------------------------------
    // CapMode tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_mode_from_name_all() {
        assert_eq!(CapMode::from_name("NOPRIV"), Some(CapMode::NoPriv));
        assert_eq!(CapMode::from_name("PURE1E_INIT"), Some(CapMode::Pure1eInit));
        assert_eq!(CapMode::from_name("UNCERTAIN"), Some(CapMode::Uncertain));
    }

    #[test]
    fn test_mode_from_name_case_insensitive() {
        assert_eq!(CapMode::from_name("nopriv"), Some(CapMode::NoPriv));
        assert_eq!(CapMode::from_name("Uncertain"), Some(CapMode::Uncertain));
    }

    #[test]
    fn test_mode_from_name_unknown() {
        assert_eq!(CapMode::from_name("bogus"), None);
    }

    #[test]
    fn test_mode_names() {
        assert_eq!(CapMode::Uncertain.name(), "UNCERTAIN");
        assert_eq!(CapMode::NoPriv.name(), "NOPRIV");
        assert_eq!(CapMode::Pure1eInit.name(), "PURE1E_INIT");
    }

    #[test]
    fn test_mode_descriptions_not_empty() {
        for mode in &ALL_MODES {
            assert!(!mode.description().is_empty());
        }
    }

    #[test]
    fn test_mode_nopriv_securebits() {
        let bits = CapMode::NoPriv.securebits();
        assert!(bits & SECBIT_NOROOT != 0);
        assert!(bits & SECBIT_NOROOT_LOCKED != 0);
        assert!(bits & SECBIT_NO_SETUID_FIXUP != 0);
    }

    #[test]
    fn test_mode_uncertain_securebits_zero() {
        assert_eq!(CapMode::Uncertain.securebits(), 0);
    }

    // -----------------------------------------------------------------------
    // Cap spec parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_simple_set() {
        let specs = parse_cap_spec("cap_chown=ep").unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].caps, vec![Cap::Chown]);
        assert_eq!(specs[0].op, CapSpecOp::Set);
        assert!(specs[0].sets.effective);
        assert!(specs[0].sets.permitted);
        assert!(!specs[0].sets.inheritable);
    }

    #[test]
    fn test_parse_add() {
        let specs = parse_cap_spec("cap_sys_admin+eip").unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].caps, vec![Cap::SysAdmin]);
        assert_eq!(specs[0].op, CapSpecOp::Add);
        assert!(specs[0].sets.effective);
        assert!(specs[0].sets.inheritable);
        assert!(specs[0].sets.permitted);
    }

    #[test]
    fn test_parse_remove() {
        let specs = parse_cap_spec("cap_chown-p").unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].caps, vec![Cap::Chown]);
        assert_eq!(specs[0].op, CapSpecOp::Remove);
        assert!(specs[0].sets.permitted);
    }

    #[test]
    fn test_parse_all_caps_set() {
        let specs = parse_cap_spec("=ep").unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].caps.len(), CAP_COUNT);
        assert_eq!(specs[0].op, CapSpecOp::Set);
    }

    #[test]
    fn test_parse_multiple_caps_comma() {
        let specs = parse_cap_spec("cap_net_raw,cap_net_admin=ep").unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].caps, vec![Cap::NetRaw, Cap::NetAdmin]);
    }

    #[test]
    fn test_parse_multiple_tokens() {
        let specs = parse_cap_spec("=ep cap_chown-p").unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].op, CapSpecOp::Set);
        assert_eq!(specs[1].op, CapSpecOp::Remove);
    }

    #[test]
    fn test_parse_without_cap_prefix() {
        let specs = parse_cap_spec("chown=ep").unwrap();
        assert_eq!(specs[0].caps, vec![Cap::Chown]);
    }

    #[test]
    fn test_parse_uppercase_flags() {
        let specs = parse_cap_spec("cap_chown=EP").unwrap();
        assert!(specs[0].sets.effective);
        assert!(specs[0].sets.permitted);
    }

    #[test]
    fn test_parse_empty_error() {
        assert!(parse_cap_spec("").is_err());
    }

    #[test]
    fn test_parse_no_operator_error() {
        assert!(parse_cap_spec("cap_chown").is_err());
    }

    #[test]
    fn test_parse_unknown_cap_error() {
        assert!(parse_cap_spec("cap_bogus=ep").is_err());
    }

    #[test]
    fn test_parse_bad_flag_error() {
        assert!(parse_cap_spec("cap_chown=xyz").is_err());
    }

    #[test]
    fn test_parse_e_only() {
        let specs = parse_cap_spec("cap_kill=e").unwrap();
        assert!(specs[0].sets.effective);
        assert!(!specs[0].sets.inheritable);
        assert!(!specs[0].sets.permitted);
    }

    #[test]
    fn test_parse_i_only() {
        let specs = parse_cap_spec("cap_kill=i").unwrap();
        assert!(!specs[0].sets.effective);
        assert!(specs[0].sets.inheritable);
        assert!(!specs[0].sets.permitted);
    }

    #[test]
    fn test_parse_p_only() {
        let specs = parse_cap_spec("cap_kill=p").unwrap();
        assert!(!specs[0].sets.effective);
        assert!(!specs[0].sets.inheritable);
        assert!(specs[0].sets.permitted);
    }

    // -----------------------------------------------------------------------
    // Apply cap specs to FileCaps
    // -----------------------------------------------------------------------

    #[test]
    fn test_apply_set_to_file() {
        let specs = parse_cap_spec("cap_chown=ep").unwrap();
        let mut fcaps = FileCaps::default();
        apply_cap_specs(&specs, &mut fcaps);
        assert!(fcaps.permitted.has(Cap::Chown));
        assert!(fcaps.effective);
    }

    #[test]
    fn test_apply_add_to_file() {
        let specs = parse_cap_spec("cap_kill+p").unwrap();
        let mut fcaps = FileCaps::default();
        fcaps.permitted.set(Cap::Chown);
        apply_cap_specs(&specs, &mut fcaps);
        assert!(fcaps.permitted.has(Cap::Chown));
        assert!(fcaps.permitted.has(Cap::Kill));
    }

    #[test]
    fn test_apply_remove_from_file() {
        let mut fcaps = FileCaps::default();
        fcaps.permitted.set(Cap::Chown);
        fcaps.permitted.set(Cap::Kill);
        let specs = parse_cap_spec("cap_chown-p").unwrap();
        apply_cap_specs(&specs, &mut fcaps);
        assert!(!fcaps.permitted.has(Cap::Chown));
        assert!(fcaps.permitted.has(Cap::Kill));
    }

    #[test]
    fn test_apply_all_set() {
        let specs = parse_cap_spec("=ep").unwrap();
        let mut fcaps = FileCaps::default();
        apply_cap_specs(&specs, &mut fcaps);
        assert_eq!(fcaps.permitted, CapMask::full());
        assert!(fcaps.effective);
    }

    #[test]
    fn test_apply_set_clears_unspecified() {
        let mut fcaps = FileCaps::default();
        fcaps.inheritable.set(Cap::Chown);
        let specs = parse_cap_spec("cap_chown=p").unwrap();
        apply_cap_specs(&specs, &mut fcaps);
        assert!(fcaps.permitted.has(Cap::Chown));
        // Inheritable was not in the spec, so `=` clears it.
        assert!(!fcaps.inheritable.has(Cap::Chown));
    }

    // -----------------------------------------------------------------------
    // Apply cap specs to ProcessCaps
    // -----------------------------------------------------------------------

    #[test]
    fn test_apply_to_process_set() {
        let specs = parse_cap_spec("cap_chown=ep").unwrap();
        let mut pcaps = ProcessCaps::default();
        apply_cap_specs_to_process(&specs, &mut pcaps);
        assert!(pcaps.effective.has(Cap::Chown));
        assert!(pcaps.permitted.has(Cap::Chown));
        assert!(!pcaps.inheritable.has(Cap::Chown));
    }

    #[test]
    fn test_apply_to_process_add() {
        let specs = parse_cap_spec("cap_kill+ei").unwrap();
        let mut pcaps = ProcessCaps::default();
        pcaps.permitted.set(Cap::Chown);
        apply_cap_specs_to_process(&specs, &mut pcaps);
        assert!(pcaps.effective.has(Cap::Kill));
        assert!(pcaps.inheritable.has(Cap::Kill));
        // Original cap preserved by add.
        assert!(pcaps.permitted.has(Cap::Chown));
    }

    #[test]
    fn test_apply_to_process_remove() {
        let mut pcaps = ProcessCaps::new_full();
        let specs = parse_cap_spec("cap_sys_admin-eip").unwrap();
        apply_cap_specs_to_process(&specs, &mut pcaps);
        assert!(!pcaps.effective.has(Cap::SysAdmin));
        assert!(!pcaps.permitted.has(Cap::SysAdmin));
        assert!(!pcaps.inheritable.has(Cap::SysAdmin));
        // Others untouched.
        assert!(pcaps.effective.has(Cap::Chown));
    }

    // -----------------------------------------------------------------------
    // find_operator tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_find_operator_equals() {
        let (op, pos) = find_operator("cap_chown=ep").unwrap();
        assert_eq!(op, CapSpecOp::Set);
        assert_eq!(pos, 9);
    }

    #[test]
    fn test_find_operator_plus() {
        let (op, _) = find_operator("cap_chown+ep").unwrap();
        assert_eq!(op, CapSpecOp::Add);
    }

    #[test]
    fn test_find_operator_minus() {
        let (op, _) = find_operator("cap_chown-p").unwrap();
        assert_eq!(op, CapSpecOp::Remove);
    }

    #[test]
    fn test_find_operator_equals_at_start() {
        let (op, pos) = find_operator("=ep").unwrap();
        assert_eq!(op, CapSpecOp::Set);
        assert_eq!(pos, 0);
    }

    #[test]
    fn test_find_operator_none() {
        assert!(find_operator("cap_chown").is_err());
    }

    // -----------------------------------------------------------------------
    // parse_cap_names tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_cap_names_single() {
        let caps = parse_cap_names("cap_chown").unwrap();
        assert_eq!(caps, vec![Cap::Chown]);
    }

    #[test]
    fn test_parse_cap_names_multi() {
        let caps = parse_cap_names("cap_chown,cap_kill").unwrap();
        assert_eq!(caps, vec![Cap::Chown, Cap::Kill]);
    }

    #[test]
    fn test_parse_cap_names_error() {
        assert!(parse_cap_names("nonexistent").is_err());
    }

    #[test]
    fn test_parse_cap_names_empty_between_commas() {
        // "cap_chown,,cap_kill" — empty segments ignored.
        let caps = parse_cap_names("cap_chown,,cap_kill").unwrap();
        assert_eq!(caps, vec![Cap::Chown, Cap::Kill]);
    }

    // -----------------------------------------------------------------------
    // parse_set_flags tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_set_flags_all() {
        let s = parse_set_flags("eip").unwrap();
        assert!(s.effective);
        assert!(s.inheritable);
        assert!(s.permitted);
    }

    #[test]
    fn test_parse_set_flags_empty() {
        let s = parse_set_flags("").unwrap();
        assert!(!s.effective);
        assert!(!s.inheritable);
        assert!(!s.permitted);
    }

    #[test]
    fn test_parse_set_flags_error() {
        assert!(parse_set_flags("x").is_err());
    }

    // -----------------------------------------------------------------------
    // Hex encoding roundtrip tests for all individual caps
    // -----------------------------------------------------------------------

    #[test]
    fn test_hex_roundtrip_individual_caps() {
        for &cap in &ALL_CAPS {
            let mut m = CapMask::empty();
            m.set(cap);
            let hex = m.to_hex();
            let decoded = CapMask::from_hex(&hex).unwrap();
            assert_eq!(decoded, m, "hex roundtrip failed for {:?}", cap);
        }
    }

    #[test]
    fn test_hex_roundtrip_pairs() {
        for (i, &cap_i) in ALL_CAPS.iter().enumerate() {
            for &cap_j in &ALL_CAPS[i + 1..] {
                let mut m = CapMask::empty();
                m.set(cap_i);
                m.set(cap_j);
                let hex = m.to_hex();
                let decoded = CapMask::from_hex(&hex).unwrap();
                assert_eq!(
                    decoded, m,
                    "hex roundtrip failed for {cap_i:?}+{cap_j:?}",
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // CapMask::from_hex edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_hex_all_zeros() {
        assert_eq!(CapMask::from_hex("0000000000000000"), Ok(CapMask(0)));
    }

    #[test]
    fn test_hex_all_ones_in_range() {
        let m = CapMask::from_hex("000001ffffffffff").unwrap();
        assert_eq!(m, CapMask::full());
    }

    #[test]
    fn test_hex_single_f() {
        let m = CapMask::from_hex("f").unwrap();
        assert_eq!(m.0, 0xf);
    }

    #[test]
    fn test_hex_max_u64() {
        let m = CapMask::from_hex("ffffffffffffffff").unwrap();
        assert_eq!(m.0, u64::MAX);
    }

    // -----------------------------------------------------------------------
    // Personality detection tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_personality_capsh() {
        assert_eq!(extract_personality("capsh"), "capsh");
    }

    #[test]
    fn test_personality_getcap() {
        assert_eq!(extract_personality("getcap"), "getcap");
    }

    #[test]
    fn test_personality_setcap() {
        assert_eq!(extract_personality("setcap"), "setcap");
    }

    #[test]
    fn test_personality_getpcaps() {
        assert_eq!(extract_personality("getpcaps"), "getpcaps");
    }

    #[test]
    fn test_personality_captest() {
        assert_eq!(extract_personality("captest"), "captest");
    }

    #[test]
    fn test_personality_with_path() {
        assert_eq!(extract_personality("/usr/bin/capsh"), "capsh");
    }

    #[test]
    fn test_personality_with_backslash_path() {
        assert_eq!(extract_personality("C:\\bin\\getcap"), "getcap");
    }

    #[test]
    fn test_personality_with_exe_suffix() {
        assert_eq!(extract_personality("capsh.exe"), "capsh");
    }

    #[test]
    fn test_personality_with_path_and_exe() {
        assert_eq!(extract_personality("/usr/bin/setcap.exe"), "setcap");
    }

    #[test]
    fn test_personality_unknown_falls_to_capsh() {
        // Unknown names are handled by the match default arm in main,
        // which dispatches to run_capsh.
        assert_eq!(extract_personality("something_else"), "something_else");
    }

    /// Helper that mirrors the personality extraction from main().
    fn extract_personality(argv0: &str) -> String {
        let s = argv0;
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    }

    // -----------------------------------------------------------------------
    // CapshState tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_capsh_state_initial() {
        let s = CapshState::new();
        assert_eq!(s.uid, 0);
        assert_eq!(s.gid, 0);
        assert!(s.groups.is_empty());
        assert!(s.chroot.is_none());
        assert!(s.user.is_none());
        assert_eq!(s.securebits, 0);
        assert_eq!(s.mode, CapMode::Uncertain);
    }

    #[test]
    fn test_capsh_state_initial_caps_full() {
        let s = CapshState::new();
        assert_eq!(s.caps.effective, CapMask::full());
        assert_eq!(s.caps.permitted, CapMask::full());
        assert_eq!(s.caps.bounding, CapMask::full());
    }

    // -----------------------------------------------------------------------
    // captest: build_cap_tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_cap_tests_full_caps() {
        let caps = ProcessCaps::new_full();
        let results = build_cap_tests(&caps);
        assert_eq!(results.len(), CAP_COUNT);
        for r in &results {
            assert!(r.has_it, "full caps should have {:?}", r.cap);
        }
    }

    #[test]
    fn test_build_cap_tests_empty_caps() {
        let caps = ProcessCaps::default();
        let results = build_cap_tests(&caps);
        assert_eq!(results.len(), CAP_COUNT);
        for r in &results {
            assert!(!r.has_it, "empty caps should not have {:?}", r.cap);
        }
    }

    #[test]
    fn test_build_cap_tests_partial() {
        let mut caps = ProcessCaps::default();
        caps.effective.set(Cap::Chown);
        caps.effective.set(Cap::Kill);
        let results = build_cap_tests(&caps);
        let chown_r = results.iter().find(|r| r.cap == Cap::Chown).unwrap();
        assert!(chown_r.has_it);
        let kill_r = results.iter().find(|r| r.cap == Cap::Kill).unwrap();
        assert!(kill_r.has_it);
        let admin_r = results.iter().find(|r| r.cap == Cap::SysAdmin).unwrap();
        assert!(!admin_r.has_it);
    }

    #[test]
    fn test_build_cap_tests_descriptions_not_empty() {
        let caps = ProcessCaps::default();
        let results = build_cap_tests(&caps);
        for r in &results {
            assert!(!r.test_desc.is_empty(), "empty description for {:?}", r.cap);
        }
    }

    // -----------------------------------------------------------------------
    // Complex cap spec parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_spec_three_caps_comma_add() {
        let specs = parse_cap_spec("cap_chown,cap_kill,cap_setuid+eip").unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].caps.len(), 3);
        assert_eq!(specs[0].op, CapSpecOp::Add);
    }

    #[test]
    fn test_spec_multiple_whitespace_tokens() {
        let specs = parse_cap_spec("cap_chown=ep cap_kill+i cap_setuid-p").unwrap();
        assert_eq!(specs.len(), 3);
        assert_eq!(specs[0].op, CapSpecOp::Set);
        assert_eq!(specs[1].op, CapSpecOp::Add);
        assert_eq!(specs[2].op, CapSpecOp::Remove);
    }

    #[test]
    fn test_spec_all_set_then_remove() {
        // "=eip cap_chown-ep" — set all to eip, then remove chown from ep.
        let specs = parse_cap_spec("=eip cap_chown-ep").unwrap();
        let mut fcaps = FileCaps::default();
        apply_cap_specs(&specs, &mut fcaps);
        // All caps should be in permitted except chown.
        assert!(!fcaps.permitted.has(Cap::Chown));
        assert!(fcaps.permitted.has(Cap::Kill));
        assert!(fcaps.inheritable.has(Cap::Kill));
    }

    // -----------------------------------------------------------------------
    // xattr path encoding tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_xattr_path_encoding_no_crash() {
        // Just verify it does not panic with various inputs.
        let _ = xattr_path_for(Path::new("/some/file"));
        let _ = xattr_path_for(Path::new("relative"));
        let _ = xattr_path_for(Path::new("C:\\Windows\\file.exe"));
    }

    // -----------------------------------------------------------------------
    // Decode hex mask — more exhaustive
    // -----------------------------------------------------------------------

    #[test]
    fn test_decode_chown_only() {
        let m = CapMask(1);
        assert_eq!(m.to_names(), "cap_chown");
    }

    #[test]
    fn test_decode_last_cap() {
        let mut m = CapMask::empty();
        m.set(Cap::CheckpointRestore);
        let names = m.to_names();
        assert!(names.contains("cap_checkpoint_restore"));
    }

    #[test]
    fn test_decode_all_caps_count() {
        let full = CapMask::full();
        let list = full.to_cap_list();
        assert_eq!(list.len(), CAP_COUNT);
    }

    // -----------------------------------------------------------------------
    // Specific capability name lookups
    // -----------------------------------------------------------------------

    #[test]
    fn test_lookup_dac_override() {
        assert_eq!(Cap::from_name("cap_dac_override"), Some(Cap::DacOverride));
    }

    #[test]
    fn test_lookup_dac_read_search() {
        assert_eq!(Cap::from_name("cap_dac_read_search"), Some(Cap::DacReadSearch));
    }

    #[test]
    fn test_lookup_fowner() {
        assert_eq!(Cap::from_name("cap_fowner"), Some(Cap::Fowner));
    }

    #[test]
    fn test_lookup_fsetid() {
        assert_eq!(Cap::from_name("cap_fsetid"), Some(Cap::Fsetid));
    }

    #[test]
    fn test_lookup_setpcap() {
        assert_eq!(Cap::from_name("cap_setpcap"), Some(Cap::Setpcap));
    }

    #[test]
    fn test_lookup_linux_immutable() {
        assert_eq!(Cap::from_name("cap_linux_immutable"), Some(Cap::LinuxImmutable));
    }

    #[test]
    fn test_lookup_net_bind_service() {
        assert_eq!(Cap::from_name("cap_net_bind_service"), Some(Cap::NetBindService));
    }

    #[test]
    fn test_lookup_net_broadcast() {
        assert_eq!(Cap::from_name("cap_net_broadcast"), Some(Cap::NetBroadcast));
    }

    #[test]
    fn test_lookup_net_admin() {
        assert_eq!(Cap::from_name("cap_net_admin"), Some(Cap::NetAdmin));
    }

    #[test]
    fn test_lookup_ipc_lock() {
        assert_eq!(Cap::from_name("cap_ipc_lock"), Some(Cap::IpcLock));
    }

    #[test]
    fn test_lookup_ipc_owner() {
        assert_eq!(Cap::from_name("cap_ipc_owner"), Some(Cap::IpcOwner));
    }

    #[test]
    fn test_lookup_sys_module() {
        assert_eq!(Cap::from_name("cap_sys_module"), Some(Cap::SysModule));
    }

    #[test]
    fn test_lookup_sys_rawio() {
        assert_eq!(Cap::from_name("cap_sys_rawio"), Some(Cap::SysRawio));
    }

    #[test]
    fn test_lookup_sys_chroot() {
        assert_eq!(Cap::from_name("cap_sys_chroot"), Some(Cap::SysChroot));
    }

    #[test]
    fn test_lookup_sys_ptrace() {
        assert_eq!(Cap::from_name("cap_sys_ptrace"), Some(Cap::SysPtrace));
    }

    #[test]
    fn test_lookup_sys_pacct() {
        assert_eq!(Cap::from_name("cap_sys_pacct"), Some(Cap::SysPacct));
    }

    #[test]
    fn test_lookup_sys_boot() {
        assert_eq!(Cap::from_name("cap_sys_boot"), Some(Cap::SysBoot));
    }

    #[test]
    fn test_lookup_sys_nice() {
        assert_eq!(Cap::from_name("cap_sys_nice"), Some(Cap::SysNice));
    }

    #[test]
    fn test_lookup_sys_resource() {
        assert_eq!(Cap::from_name("cap_sys_resource"), Some(Cap::SysResource));
    }

    #[test]
    fn test_lookup_sys_time() {
        assert_eq!(Cap::from_name("cap_sys_time"), Some(Cap::SysTime));
    }

    #[test]
    fn test_lookup_sys_tty_config() {
        assert_eq!(Cap::from_name("cap_sys_tty_config"), Some(Cap::SysTtyConfig));
    }

    #[test]
    fn test_lookup_mknod() {
        assert_eq!(Cap::from_name("cap_mknod"), Some(Cap::Mknod));
    }

    #[test]
    fn test_lookup_lease() {
        assert_eq!(Cap::from_name("cap_lease"), Some(Cap::Lease));
    }

    #[test]
    fn test_lookup_audit_write() {
        assert_eq!(Cap::from_name("cap_audit_write"), Some(Cap::AuditWrite));
    }

    #[test]
    fn test_lookup_audit_control() {
        assert_eq!(Cap::from_name("cap_audit_control"), Some(Cap::AuditControl));
    }

    #[test]
    fn test_lookup_setfcap() {
        assert_eq!(Cap::from_name("cap_setfcap"), Some(Cap::Setfcap));
    }

    #[test]
    fn test_lookup_mac_override() {
        assert_eq!(Cap::from_name("cap_mac_override"), Some(Cap::MacOverride));
    }

    #[test]
    fn test_lookup_mac_admin() {
        assert_eq!(Cap::from_name("cap_mac_admin"), Some(Cap::MacAdmin));
    }

    #[test]
    fn test_lookup_syslog() {
        assert_eq!(Cap::from_name("cap_syslog"), Some(Cap::Syslog));
    }

    #[test]
    fn test_lookup_wake_alarm() {
        assert_eq!(Cap::from_name("cap_wake_alarm"), Some(Cap::WakeAlarm));
    }

    #[test]
    fn test_lookup_block_suspend() {
        assert_eq!(Cap::from_name("cap_block_suspend"), Some(Cap::BlockSuspend));
    }

    #[test]
    fn test_lookup_audit_read() {
        assert_eq!(Cap::from_name("cap_audit_read"), Some(Cap::AuditRead));
    }

    #[test]
    fn test_lookup_perfmon() {
        assert_eq!(Cap::from_name("cap_perfmon"), Some(Cap::Perfmon));
    }

    #[test]
    fn test_lookup_bpf() {
        assert_eq!(Cap::from_name("cap_bpf"), Some(Cap::Bpf));
    }

    #[test]
    fn test_lookup_checkpoint_restore() {
        assert_eq!(Cap::from_name("cap_checkpoint_restore"), Some(Cap::CheckpointRestore));
    }

    // -----------------------------------------------------------------------
    // Integration-style cap spec + apply tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_integration_set_net_bind_to_file() {
        let specs = parse_cap_spec("cap_net_bind_service=ep").unwrap();
        let mut fcaps = FileCaps::default();
        apply_cap_specs(&specs, &mut fcaps);
        assert!(fcaps.permitted.has(Cap::NetBindService));
        assert!(fcaps.effective);
        assert!(!fcaps.inheritable.has(Cap::NetBindService));
    }

    #[test]
    fn test_integration_full_then_drop_admin() {
        let specs = parse_cap_spec("=eip cap_sys_admin-eip").unwrap();
        let mut pcaps = ProcessCaps::default();
        apply_cap_specs_to_process(&specs, &mut pcaps);
        assert!(!pcaps.effective.has(Cap::SysAdmin));
        assert!(!pcaps.permitted.has(Cap::SysAdmin));
        assert!(!pcaps.inheritable.has(Cap::SysAdmin));
        // Others remain.
        assert!(pcaps.effective.has(Cap::Chown));
        assert!(pcaps.permitted.has(Cap::Chown));
        assert!(pcaps.inheritable.has(Cap::Chown));
    }

    #[test]
    fn test_integration_add_multiple_then_remove_one() {
        let specs = parse_cap_spec("cap_net_raw,cap_net_admin+ep cap_net_raw-p").unwrap();
        let mut fcaps = FileCaps::default();
        apply_cap_specs(&specs, &mut fcaps);
        assert!(!fcaps.permitted.has(Cap::NetRaw));
        assert!(fcaps.permitted.has(Cap::NetAdmin));
        assert!(fcaps.effective);
    }

    // -----------------------------------------------------------------------
    // run_capsh argument handling tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_capsh_help_returns_zero() {
        assert_eq!(run_capsh(&[String::from("--help")]), 0);
    }

    #[test]
    fn test_capsh_print_returns_zero() {
        assert_eq!(run_capsh(&[String::from("--print")]), 0);
    }

    #[test]
    fn test_capsh_decode_valid() {
        assert_eq!(run_capsh(&[String::from("--decode=1")]), 0);
    }

    #[test]
    fn test_capsh_decode_invalid() {
        assert_eq!(run_capsh(&[String::from("--decode=zzz")]), 1);
    }

    #[test]
    fn test_capsh_unknown_option() {
        assert_eq!(run_capsh(&[String::from("--bogus")]), 1);
    }

    #[test]
    fn test_capsh_modes_returns_zero() {
        assert_eq!(run_capsh(&[String::from("--modes")]), 0);
    }

    #[test]
    fn test_capsh_mode_nopriv() {
        assert_eq!(run_capsh(&[String::from("--mode=NOPRIV")]), 0);
    }

    #[test]
    fn test_capsh_mode_invalid() {
        assert_eq!(run_capsh(&[String::from("--mode=BOGUS")]), 1);
    }

    #[test]
    fn test_capsh_noamb() {
        assert_eq!(run_capsh(&[String::from("--noamb"), String::from("--print")]), 0);
    }

    #[test]
    fn test_capsh_uid() {
        assert_eq!(
            run_capsh(&[String::from("--uid=1000"), String::from("--print")]),
            0
        );
    }

    #[test]
    fn test_capsh_uid_invalid() {
        assert_eq!(run_capsh(&[String::from("--uid=abc")]), 1);
    }

    #[test]
    fn test_capsh_gid() {
        assert_eq!(
            run_capsh(&[String::from("--gid=100"), String::from("--print")]),
            0
        );
    }

    #[test]
    fn test_capsh_gid_invalid() {
        assert_eq!(run_capsh(&[String::from("--gid=abc")]), 1);
    }

    #[test]
    fn test_capsh_keep_1() {
        assert_eq!(run_capsh(&[String::from("--keep=1")]), 0);
    }

    #[test]
    fn test_capsh_keep_0() {
        assert_eq!(run_capsh(&[String::from("--keep=0")]), 0);
    }

    #[test]
    fn test_capsh_keep_invalid() {
        assert_eq!(run_capsh(&[String::from("--keep=2")]), 1);
    }

    #[test]
    fn test_capsh_secbits() {
        assert_eq!(run_capsh(&[String::from("--secbits=0x10")]), 0);
    }

    #[test]
    fn test_capsh_secbits_invalid() {
        assert_eq!(run_capsh(&[String::from("--secbits=zzz")]), 1);
    }

    #[test]
    fn test_capsh_drop_known() {
        assert_eq!(
            run_capsh(&[String::from("--drop=cap_sys_admin"), String::from("--print")]),
            0
        );
    }

    #[test]
    fn test_capsh_drop_unknown() {
        assert_eq!(run_capsh(&[String::from("--drop=bogus")]), 1);
    }

    #[test]
    fn test_capsh_addamb_known() {
        // Must set inheritable first for addamb to succeed.
        assert_eq!(
            run_capsh(&[
                String::from("--inh=cap_chown=eip"),
                String::from("--addamb=cap_chown"),
                String::from("--print"),
            ]),
            0
        );
    }

    #[test]
    fn test_capsh_addamb_without_inheritable() {
        // Should fail — ambient requires permitted AND inheritable.
        // Default state has full permitted but empty inheritable.
        let state = CapshState::new();
        // inheritable is empty by default.
        assert!(!state.caps.inheritable.has(Cap::Chown));
        // So adding to ambient should fail.
        assert_eq!(run_capsh(&[String::from("--addamb=cap_chown")]), 1);
    }

    #[test]
    fn test_capsh_addamb_unknown() {
        assert_eq!(run_capsh(&[String::from("--addamb=bogus")]), 1);
    }

    #[test]
    fn test_capsh_delamb_known() {
        assert_eq!(run_capsh(&[String::from("--delamb=cap_chown")]), 0);
    }

    #[test]
    fn test_capsh_delamb_unknown() {
        assert_eq!(run_capsh(&[String::from("--delamb=bogus")]), 1);
    }

    #[test]
    fn test_capsh_exec_no_command() {
        assert_eq!(run_capsh(&[String::from("--")]), 1);
    }

    #[test]
    fn test_capsh_exec_with_command() {
        assert_eq!(
            run_capsh(&[
                String::from("--"),
                String::from("/bin/sh"),
                String::from("-c"),
                String::from("echo hello"),
            ]),
            0
        );
    }

    #[test]
    fn test_capsh_empty_args() {
        assert_eq!(run_capsh(&[]), 1);
    }

    #[test]
    fn test_capsh_caps_valid() {
        assert_eq!(
            run_capsh(&[String::from("--caps=cap_chown=ep"), String::from("--print")]),
            0
        );
    }

    #[test]
    fn test_capsh_caps_invalid() {
        assert_eq!(run_capsh(&[String::from("--caps=bogus=ep")]), 1);
    }

    #[test]
    fn test_capsh_inh_valid() {
        assert_eq!(
            run_capsh(&[String::from("--inh=cap_kill=i"), String::from("--print")]),
            0
        );
    }

    #[test]
    fn test_capsh_inh_invalid() {
        assert_eq!(run_capsh(&[String::from("--inh=bogus=i")]), 1);
    }

    #[test]
    fn test_capsh_user() {
        assert_eq!(
            run_capsh(&[String::from("--user=testuser"), String::from("--print")]),
            0
        );
    }

    #[test]
    fn test_capsh_chroot() {
        assert_eq!(
            run_capsh(&[String::from("--chroot=/tmp"), String::from("--print")]),
            0
        );
    }
}
