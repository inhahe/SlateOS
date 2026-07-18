//! Share Sheet — cross-app content sharing dialog.
//!
//! Provides a unified share interface where apps register as share
//! targets (by supported content types), and users can share content
//! (text, URLs, files, images) from any app to any compatible target.
//!
//! ## Architecture
//!
//! ```text
//! App shares content
//!   → sharesheet::share(content_type, data)
//!     → finds matching targets
//!     → presents target list to user
//!     → dispatches to chosen target
//!
//! Integration:
//!   → appregistry (registered apps)
//!   → contextmenu (share menu item)
//!   → clipboard (copy-to-clipboard target)
//!   → fileshare (network share target)
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

/// Content type for sharing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Text,
    Url,
    Image,
    File,
    Video,
    Audio,
    Contact,
    Location,
}

impl ContentType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Url => "URL",
            Self::Image => "Image",
            Self::File => "File",
            Self::Video => "Video",
            Self::Audio => "Audio",
            Self::Contact => "Contact",
            Self::Location => "Location",
        }
    }
}

/// A registered share target.
#[derive(Debug, Clone)]
pub struct ShareTarget {
    pub id: u32,
    pub app_name: String,
    pub display_name: String,
    pub supported_types: Vec<ContentType>,
    pub priority: u32,
    pub use_count: u64,
    pub registered_ns: u64,
}

/// A share action (completed share).
#[derive(Debug, Clone)]
pub struct ShareAction {
    pub id: u32,
    pub content_type: ContentType,
    pub target_id: u32,
    pub target_name: String,
    pub data_preview: String,
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_TARGETS: usize = 50;
const MAX_HISTORY: usize = 200;

struct State {
    targets: Vec<ShareTarget>,
    history: Vec<ShareAction>,
    next_target_id: u32,
    next_action_id: u32,
    total_shares: u64,
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

    let targets = alloc::vec![
        ShareTarget { id: 1, app_name: String::from("clipboard"), display_name: String::from("Copy to Clipboard"),
            supported_types: alloc::vec![ContentType::Text, ContentType::Url, ContentType::Image],
            priority: 100, use_count: 0, registered_ns: crate::hpet::elapsed_ns() },
        ShareTarget { id: 2, app_name: String::from("email"), display_name: String::from("Email"),
            supported_types: alloc::vec![ContentType::Text, ContentType::Url, ContentType::File, ContentType::Image],
            priority: 90, use_count: 0, registered_ns: crate::hpet::elapsed_ns() },
        ShareTarget { id: 3, app_name: String::from("fileshare"), display_name: String::from("Nearby Share"),
            supported_types: alloc::vec![ContentType::File, ContentType::Image, ContentType::Video, ContentType::Audio],
            priority: 80, use_count: 0, registered_ns: crate::hpet::elapsed_ns() },
    ];

    *guard = Some(State {
        targets,
        history: Vec::new(),
        next_target_id: 4,
        next_action_id: 1,
        total_shares: 0,
        ops: 0,
    });
}

/// Register a share target.
pub fn register_target(app_name: &str, display_name: &str, types: Vec<ContentType>) -> KernelResult<u32> {
    with_state(|state| {
        if state.targets.len() >= MAX_TARGETS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_target_id;
        state.next_target_id += 1;
        state.targets.push(ShareTarget {
            id, app_name: String::from(app_name),
            display_name: String::from(display_name),
            supported_types: types, priority: 50,
            use_count: 0, registered_ns: crate::hpet::elapsed_ns(),
        });
        Ok(id)
    })
}

/// Unregister a share target.
pub fn unregister_target(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.targets.iter().position(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        state.targets.remove(pos);
        Ok(())
    })
}

/// Get targets that support a content type (sorted by use count then priority).
pub fn get_targets(content_type: ContentType) -> Vec<ShareTarget> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut matches: Vec<ShareTarget> = s.targets.iter()
            .filter(|t| t.supported_types.contains(&content_type))
            .cloned()
            .collect();
        // Sort: most used first, then by priority.
        matches.sort_by(|a, b| b.use_count.cmp(&a.use_count)
            .then(b.priority.cmp(&a.priority)));
        matches
    })
}

/// Share content to a specific target.
pub fn share_to(target_id: u32, content_type: ContentType, data: &str) -> KernelResult<u32> {
    with_state(|state| {
        let target = state.targets.iter_mut().find(|t| t.id == target_id)
            .ok_or(KernelError::NotFound)?;
        if !target.supported_types.contains(&content_type) {
            return Err(KernelError::InvalidArgument);
        }
        target.use_count += 1;
        let target_name = target.display_name.clone();
        state.total_shares += 1;

        let id = state.next_action_id;
        state.next_action_id += 1;

        let preview: String = data.chars().take(50).collect();
        if state.history.len() >= MAX_HISTORY {
            state.history.remove(0);
        }
        state.history.push(ShareAction {
            id, content_type, target_id,
            target_name,
            data_preview: preview,
            timestamp_ns: crate::hpet::elapsed_ns(),
        });
        Ok(id)
    })
}

/// List all targets.
pub fn list_targets() -> Vec<ShareTarget> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.targets.clone())
}

/// Recent share history.
pub fn list_history(count: usize) -> Vec<ShareAction> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = s.history.len().saturating_sub(count);
        s.history[start..].to_vec()
    })
}

/// Statistics: (target_count, total_shares, ops).
pub fn stats() -> (usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.targets.len(), s.total_shares, s.ops),
        None => (0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("sharesheet::self_test() — running tests...");
    init_defaults();

    // 1: Default targets.
    let targets = list_targets();
    assert_eq!(targets.len(), 3);
    crate::serial_println!("  [1/8] default targets: OK");

    // 2: Get targets for text.
    let text_targets = get_targets(ContentType::Text);
    assert!(text_targets.len() >= 2); // clipboard + email
    crate::serial_println!("  [2/8] text targets: OK");

    // 3: Share to clipboard.
    let aid = share_to(1, ContentType::Text, "Hello World").expect("share");
    assert!(aid > 0);
    crate::serial_println!("  [3/8] share text: OK");

    // 4: Wrong type rejected.
    let result = share_to(1, ContentType::Video, "test.mp4");
    assert!(result.is_err());
    crate::serial_println!("  [4/8] type mismatch: OK");

    // 5: Register custom target.
    let tid = register_target("notes", "Quick Note", alloc::vec![ContentType::Text, ContentType::Url]).expect("reg");
    assert_eq!(list_targets().len(), 4);
    crate::serial_println!("  [5/8] register target: OK");

    // 6: Share to new target.
    share_to(tid, ContentType::Url, "https://example.com").expect("share2");
    crate::serial_println!("  [6/8] share URL: OK");

    // 7: History.
    let history = list_history(10);
    assert_eq!(history.len(), 2);
    crate::serial_println!("  [7/8] history: OK");

    // 8: Stats.
    let (targets, shares, ops) = stats();
    assert_eq!(targets, 4);
    assert_eq!(shares, 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("sharesheet::self_test() — all 8 tests passed");
}
