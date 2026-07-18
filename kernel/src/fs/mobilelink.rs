//! Mobile Link — phone/mobile device linking and integration.
//!
//! Pairs a mobile device with the desktop, enabling cross-device
//! features: notification mirroring, SMS/MMS from desktop, photo
//! transfer, clipboard sync, and phone call handling.
//!
//! ## Architecture
//!
//! ```text
//! Mobile device discovery
//!   → mobilelink::start_pairing() → generates pairing code
//!   → mobilelink::confirm_pairing(code) → device registered
//!
//! Connected device
//!   → mobilelink::mirror_notification(device, notif)
//!   → mobilelink::send_sms(device, to, message)
//!   → mobilelink::transfer_file(device, path)
//!
//! Integration:
//!   → notifcenter (mirrored notifications)
//!   → clipboard (cross-device clipboard sync)
//!   → bluetooth (BLE discovery and transport)
//!   → fileshare (file transfer)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Mobile device platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobilePlatform {
    Android,
    Ios,
    Other,
}

impl MobilePlatform {
    pub fn label(self) -> &'static str {
        match self {
            Self::Android => "Android",
            Self::Ios => "iOS",
            Self::Other => "Other",
        }
    }
}

/// Connection state of a linked device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Disconnected,
    Pairing,
    Connected,
    Syncing,
}

impl LinkState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Pairing => "Pairing",
            Self::Connected => "Connected",
            Self::Syncing => "Syncing",
        }
    }
}

/// Feature flags for linked device capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFeature {
    NotificationMirror,
    Sms,
    PhoneCalls,
    FileTransfer,
    ClipboardSync,
    PhotoStream,
    BatteryStatus,
}

impl LinkFeature {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotificationMirror => "Notifications",
            Self::Sms => "SMS/MMS",
            Self::PhoneCalls => "Phone Calls",
            Self::FileTransfer => "File Transfer",
            Self::ClipboardSync => "Clipboard Sync",
            Self::PhotoStream => "Photo Stream",
            Self::BatteryStatus => "Battery Status",
        }
    }
}

/// A linked mobile device.
#[derive(Debug, Clone)]
pub struct LinkedDevice {
    pub id: u32,
    pub name: String,
    pub platform: MobilePlatform,
    pub model: String,
    pub state: LinkState,
    pub pairing_code: u32,
    pub features: Vec<LinkFeature>,
    /// Battery level 0-100 (0 = unknown).
    pub battery_percent: u8,
    pub paired_ns: u64,
    pub last_seen_ns: u64,
}

/// A mirrored notification from a mobile device.
#[derive(Debug, Clone)]
pub struct MirroredNotification {
    pub id: u32,
    pub device_id: u32,
    pub app_name: String,
    pub title: String,
    pub body: String,
    pub timestamp_ns: u64,
    pub dismissed: bool,
}

/// An SMS message.
#[derive(Debug, Clone)]
pub struct SmsMessage {
    pub id: u32,
    pub device_id: u32,
    pub recipient: String,
    pub body: String,
    pub sent: bool,
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 10;
const MAX_NOTIFICATIONS: usize = 200;
const MAX_MESSAGES: usize = 500;

struct State {
    devices: Vec<LinkedDevice>,
    notifications: Vec<MirroredNotification>,
    messages: Vec<SmsMessage>,
    next_device_id: u32,
    next_notif_id: u32,
    next_msg_id: u32,
    total_paired: u64,
    total_notifications: u64,
    total_messages: u64,
    total_transfers: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        devices: Vec::new(),
        notifications: Vec::new(),
        messages: Vec::new(),
        next_device_id: 1,
        next_notif_id: 1,
        next_msg_id: 1,
        total_paired: 0,
        total_notifications: 0,
        total_messages: 0,
        total_transfers: 0,
        ops: 0,
    });
}

/// Start pairing a new device. Returns (device_id, pairing_code).
pub fn start_pairing(name: &str, platform: MobilePlatform, model: &str) -> KernelResult<(u32, u32)> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_device_id;
        state.next_device_id += 1;

        // Generate a 6-digit pairing code from timestamp.
        let now = crate::hpet::elapsed_ns();
        let code = ((now / 1000) % 900000 + 100000) as u32;

        state.devices.push(LinkedDevice {
            id,
            name: String::from(name),
            platform,
            model: String::from(model),
            state: LinkState::Pairing,
            pairing_code: code,
            features: alloc::vec![
                LinkFeature::NotificationMirror,
                LinkFeature::FileTransfer,
                LinkFeature::BatteryStatus,
            ],
            battery_percent: 0,
            paired_ns: now,
            last_seen_ns: now,
        });
        Ok((id, code))
    })
}

/// Confirm pairing with code. Changes state from Pairing to Connected.
pub fn confirm_pairing(device_id: u32, code: u32) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        if dev.state != LinkState::Pairing {
            return Err(KernelError::InvalidArgument);
        }
        if dev.pairing_code != code {
            return Err(KernelError::PermissionDenied);
        }
        dev.state = LinkState::Connected;
        dev.last_seen_ns = crate::hpet::elapsed_ns();
        state.total_paired += 1;
        Ok(())
    })
}

/// Disconnect a device (keeps it paired for reconnection).
pub fn disconnect(device_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        dev.state = LinkState::Disconnected;
        Ok(())
    })
}

/// Reconnect a previously paired device.
pub fn reconnect(device_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        if dev.state != LinkState::Disconnected {
            return Err(KernelError::InvalidArgument);
        }
        dev.state = LinkState::Connected;
        dev.last_seen_ns = crate::hpet::elapsed_ns();
        Ok(())
    })
}

/// Remove (unpair) a device entirely.
pub fn remove_device(device_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.devices.iter().position(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        state.devices.remove(pos);
        // Clean up notifications and messages from this device.
        state.notifications.retain(|n| n.device_id != device_id);
        state.messages.retain(|m| m.device_id != device_id);
        Ok(())
    })
}

/// Enable/disable a feature on a device.
pub fn set_feature(device_id: u32, feature: LinkFeature, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        if enabled {
            if !dev.features.contains(&feature) {
                dev.features.push(feature);
            }
        } else {
            dev.features.retain(|f| *f != feature);
        }
        Ok(())
    })
}

/// Update device battery level.
pub fn update_battery(device_id: u32, percent: u8) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        dev.battery_percent = percent.min(100);
        dev.last_seen_ns = crate::hpet::elapsed_ns();
        Ok(())
    })
}

/// Mirror a notification from the mobile device.
pub fn mirror_notification(device_id: u32, app_name: &str, title: &str, body: &str) -> KernelResult<u32> {
    with_state(|state| {
        // Verify device exists and is connected.
        let dev = state.devices.iter().find(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        if dev.state != LinkState::Connected && dev.state != LinkState::Syncing {
            return Err(KernelError::InvalidArgument);
        }
        if !dev.features.contains(&LinkFeature::NotificationMirror) {
            return Err(KernelError::PermissionDenied);
        }

        if state.notifications.len() >= MAX_NOTIFICATIONS {
            // Evict oldest dismissed.
            if let Some(pos) = state.notifications.iter().position(|n| n.dismissed) {
                state.notifications.remove(pos);
            } else {
                state.notifications.remove(0);
            }
        }

        let id = state.next_notif_id;
        state.next_notif_id += 1;
        state.total_notifications += 1;

        state.notifications.push(MirroredNotification {
            id,
            device_id,
            app_name: String::from(app_name),
            title: String::from(title),
            body: String::from(body),
            timestamp_ns: crate::hpet::elapsed_ns(),
            dismissed: false,
        });
        Ok(id)
    })
}

/// Dismiss a mirrored notification.
pub fn dismiss_notification(notif_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let notif = state.notifications.iter_mut().find(|n| n.id == notif_id)
            .ok_or(KernelError::NotFound)?;
        notif.dismissed = true;
        Ok(())
    })
}

/// Send an SMS message through the linked device.
pub fn send_sms(device_id: u32, recipient: &str, body: &str) -> KernelResult<u32> {
    with_state(|state| {
        let dev = state.devices.iter().find(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        if dev.state != LinkState::Connected && dev.state != LinkState::Syncing {
            return Err(KernelError::InvalidArgument);
        }
        if !dev.features.contains(&LinkFeature::Sms) {
            return Err(KernelError::PermissionDenied);
        }

        if state.messages.len() >= MAX_MESSAGES {
            state.messages.remove(0);
        }

        let id = state.next_msg_id;
        state.next_msg_id += 1;
        state.total_messages += 1;

        state.messages.push(SmsMessage {
            id,
            device_id,
            recipient: String::from(recipient),
            body: String::from(body),
            sent: true,
            timestamp_ns: crate::hpet::elapsed_ns(),
        });
        Ok(id)
    })
}

/// Record a file transfer (increments counter).
pub fn record_transfer(device_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.devices.iter().any(|d| d.id == device_id) {
            return Err(KernelError::NotFound);
        }
        state.total_transfers += 1;
        Ok(())
    })
}

/// List all linked devices.
pub fn list_devices() -> Vec<LinkedDevice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// List notifications for a device (or all if device_id is 0).
pub fn list_notifications(device_id: u32) -> Vec<MirroredNotification> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        if device_id == 0 {
            s.notifications.clone()
        } else {
            s.notifications.iter().filter(|n| n.device_id == device_id).cloned().collect()
        }
    })
}

/// List SMS messages.
pub fn list_messages(device_id: u32) -> Vec<SmsMessage> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        if device_id == 0 {
            s.messages.clone()
        } else {
            s.messages.iter().filter(|m| m.device_id == device_id).cloned().collect()
        }
    })
}

/// Statistics: (device_count, total_paired, total_notifications, total_messages, total_transfers, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.total_paired, s.total_notifications, s.total_messages, s.total_transfers, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("mobilelink::self_test() — running tests...");
    init_defaults();

    // 1: No devices initially.
    assert!(list_devices().is_empty());
    crate::serial_println!("  [1/10] empty initial: OK");

    // 2: Start pairing.
    let (dev_id, code) = start_pairing("My Phone", MobilePlatform::Android, "Pixel 8").expect("pair");
    assert!(dev_id > 0);
    assert!(code >= 100000 && code <= 999999);
    let devs = list_devices();
    assert_eq!(devs.len(), 1);
    assert_eq!(devs[0].state, LinkState::Pairing);
    crate::serial_println!("  [2/10] start pairing: OK");

    // 3: Wrong code rejected.
    let bad = confirm_pairing(dev_id, code + 1);
    assert!(bad.is_err());
    crate::serial_println!("  [3/10] wrong code: OK");

    // 4: Correct code confirms.
    confirm_pairing(dev_id, code).expect("confirm");
    let devs = list_devices();
    assert_eq!(devs[0].state, LinkState::Connected);
    crate::serial_println!("  [4/10] confirm pairing: OK");

    // 5: Mirror notification.
    let nid = mirror_notification(dev_id, "WhatsApp", "New message", "Hello!").expect("mirror");
    assert!(nid > 0);
    assert_eq!(list_notifications(dev_id).len(), 1);
    crate::serial_println!("  [5/10] mirror notification: OK");

    // 6: Enable SMS and send.
    set_feature(dev_id, LinkFeature::Sms, true).expect("enable sms");
    let mid = send_sms(dev_id, "+1234567890", "Hi there").expect("sms");
    assert!(mid > 0);
    crate::serial_println!("  [6/10] send SMS: OK");

    // 7: Battery update.
    update_battery(dev_id, 75).expect("battery");
    let devs = list_devices();
    assert_eq!(devs[0].battery_percent, 75);
    crate::serial_println!("  [7/10] battery update: OK");

    // 8: Disconnect and reconnect.
    disconnect(dev_id).expect("disconnect");
    let devs = list_devices();
    assert_eq!(devs[0].state, LinkState::Disconnected);
    reconnect(dev_id).expect("reconnect");
    let devs = list_devices();
    assert_eq!(devs[0].state, LinkState::Connected);
    crate::serial_println!("  [8/10] disconnect/reconnect: OK");

    // 9: File transfer.
    record_transfer(dev_id).expect("transfer");
    crate::serial_println!("  [9/10] file transfer: OK");

    // 10: Remove device.
    remove_device(dev_id).expect("remove");
    assert!(list_devices().is_empty());
    assert!(list_notifications(dev_id).is_empty());
    let (_, paired, notifs, msgs, transfers, ops) = stats();
    assert_eq!(paired, 1);
    assert_eq!(notifs, 1);
    assert_eq!(msgs, 1);
    assert_eq!(transfers, 1);
    assert!(ops > 0);
    crate::serial_println!("  [10/10] remove device: OK");

    crate::serial_println!("mobilelink::self_test() — all 10 tests passed");
}
