//! Kernel / OS component build configuration.
//!
//! Allows recompiling the kernel or individual OS components with
//! specified parameters.  Tracks source changes and provides a settings
//! UI for build configuration.
//!
//! ## Design Reference
//!
//! design.txt line 1300: "recompile kernel or os component with
//!   specified parameters??? - detect if any changes to source since
//!   last compile"
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Build Configuration
//!   → kernelbuild::list_components()
//!   → kernelbuild::set_param("kernel", "page_size", "16384")
//!   → kernelbuild::check_source_changed("kernel")
//!   → kernelbuild::build("kernel")
//!
//! Automatic rebuild watcher
//!   → kernelbuild::scan_changed()
//!   → returns list of components with modified sources
//! ```

#![allow(dead_code)]

use crate::fs::path::{Path, PathBuf};
use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Build target type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentType {
    /// The kernel itself.
    Kernel,
    /// A kernel module (e.g. filesystem driver).
    KernelModule,
    /// A system service (e.g. compositor, IPC daemon).
    SystemService,
    /// A core utility (e.g. coreutils, shell).
    CoreUtility,
    /// A library shared by multiple components.
    SharedLibrary,
    /// A bootloader component.
    Bootloader,
}

/// Build optimisation level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptLevel {
    /// No optimisation (debug).
    Debug,
    /// Basic optimisation (-O1).
    O1,
    /// Standard optimisation (-O2).
    O2,
    /// Full optimisation (-O3 / --release).
    Release,
    /// Size optimisation (-Os).
    Size,
}

/// Build status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildStatus {
    /// Never built.
    NeverBuilt,
    /// Built and up to date.
    UpToDate,
    /// Source changed since last build.
    SourceChanged,
    /// Currently building.
    Building,
    /// Last build failed.
    Failed,
}

/// A build parameter (key-value).
#[derive(Debug, Clone)]
pub struct BuildParam {
    /// Parameter key (e.g. "page_size").
    pub key: String,
    /// Human-readable description.
    pub description: String,
    /// Current value.
    pub value: String,
    /// Default value.
    pub default_value: String,
    /// Allowed values (empty = freeform).
    pub allowed: Vec<String>,
    /// Whether changing this requires a full rebuild.
    pub requires_full_rebuild: bool,
}

/// A buildable OS component.
#[derive(Debug, Clone)]
pub struct Component {
    /// Unique identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Component type.
    pub comp_type: ComponentType,
    /// Source directory path.
    ///
    /// `PathBuf`, not `String`: a path is a byte string with only `/` and NUL
    /// forbidden, and this one is also the input to the change-detection hash.
    /// Under the old `String` typing two source trees whose names differed
    /// only in a byte with no UTF-8 spelling hashed identically, so a rebuild
    /// of one was silently suppressed as "up to date" because the *other* had
    /// been built. See design-decisions.md §261.
    pub source_dir: PathBuf,
    /// Output path (binary / library).
    ///
    /// `PathBuf` for the same reason, with a sharper edge: this is where the
    /// build artefact is written, so a lossily decoded spelling overwrites a
    /// different file than the one the user named.
    pub output_path: PathBuf,
    /// Build parameters.
    pub params: Vec<BuildParam>,
    /// Build status.
    pub status: BuildStatus,
    /// Source hash at last build.
    pub last_source_hash: u64,
    /// Current source hash.
    pub current_source_hash: u64,
    /// Last build timestamp (ns).
    pub last_build_ns: u64,
    /// Last build duration (ms).
    pub last_build_duration_ms: u64,
    /// Build count.
    pub build_count: u64,
    /// Whether this component is system-critical.
    pub system_critical: bool,
    /// Dependencies (other component IDs).
    pub dependencies: Vec<String>,
    /// Whether auto-rebuild on source change is enabled.
    pub auto_rebuild: bool,
    /// Optimisation level.
    pub opt_level: OptLevel,
}

/// Build log entry.
#[derive(Debug, Clone)]
pub struct BuildLog {
    /// Component ID.
    pub component_id: String,
    /// Timestamp (ns).
    pub timestamp_ns: u64,
    /// Whether the build succeeded.
    pub success: bool,
    /// Duration in ms.
    pub duration_ms: u64,
    /// Output / error messages.
    pub output: String,
    /// Parameters used.
    pub params_snapshot: Vec<(String, String)>,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_COMPONENTS: usize = 128;
const MAX_PARAMS_PER_COMPONENT: usize = 64;
const MAX_BUILD_LOGS: usize = 256;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    components: Vec<Component>,
    build_logs: Vec<BuildLog>,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    components: Vec::new(),
    build_logs: Vec::new(),
    changes: 0,
});

static OP_COUNT: AtomicU64 = AtomicU64::new(0);

/// FNV-1a over raw bytes.
///
/// Takes `&[u8]` rather than `&str` so that a source directory whose name is
/// not valid UTF-8 hashes as itself. The values are unchanged for names that
/// *are* UTF-8 -- this hashed `s.as_bytes()` before too.
fn simple_hash(s: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ---------------------------------------------------------------------------
// Component management
// ---------------------------------------------------------------------------

/// Register a buildable component.
pub fn register_component(
    id: &str,
    name: &str,
    comp_type: ComponentType,
    source_dir: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
) -> KernelResult<()> {
    let (source_dir, output_path) = (source_dir.as_ref(), output_path.as_ref());
    let mut state = STATE.lock();
    if state.components.len() >= MAX_COMPONENTS {
        return Err(KernelError::ResourceExhausted);
    }
    if state.components.iter().any(|c| c.id == id) {
        return Err(KernelError::AlreadyExists);
    }
    let hash = simple_hash(source_dir.as_bytes());
    state.components.push(Component {
        id: String::from(id),
        name: String::from(name),
        comp_type,
        source_dir: source_dir.to_path_buf(),
        output_path: output_path.to_path_buf(),
        params: Vec::new(),
        status: BuildStatus::NeverBuilt,
        last_source_hash: 0,
        current_source_hash: hash,
        last_build_ns: 0,
        last_build_duration_ms: 0,
        build_count: 0,
        system_critical: matches!(comp_type, ComponentType::Kernel | ComponentType::Bootloader),
        dependencies: Vec::new(),
        auto_rebuild: false,
        opt_level: OptLevel::Release,
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Remove a component.
pub fn remove_component(id: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let idx = state
        .components
        .iter()
        .position(|c| c.id == id)
        .ok_or(KernelError::NotFound)?;
    if state.components[idx].system_critical {
        return Err(KernelError::PermissionDenied);
    }
    state.components.remove(idx);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get a component.
pub fn get_component(id: &str) -> KernelResult<Component> {
    let state = STATE.lock();
    state
        .components
        .iter()
        .find(|c| c.id == id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all components.
pub fn list_components() -> Vec<Component> {
    STATE.lock().components.clone()
}

// ---------------------------------------------------------------------------
// Build parameters
// ---------------------------------------------------------------------------

/// Add a build parameter to a component.
pub fn add_param(
    component_id: &str,
    key: &str,
    description: &str,
    default_value: &str,
    allowed: &[&str],
    requires_full_rebuild: bool,
) -> KernelResult<()> {
    let mut state = STATE.lock();
    let comp = state
        .components
        .iter_mut()
        .find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;
    if comp.params.len() >= MAX_PARAMS_PER_COMPONENT {
        return Err(KernelError::ResourceExhausted);
    }
    if comp.params.iter().any(|p| p.key == key) {
        return Err(KernelError::AlreadyExists);
    }
    comp.params.push(BuildParam {
        key: String::from(key),
        description: String::from(description),
        value: String::from(default_value),
        default_value: String::from(default_value),
        allowed: allowed.iter().map(|s| String::from(*s)).collect(),
        requires_full_rebuild,
    });
    state.changes += 1;
    Ok(())
}

/// Set a build parameter value.
pub fn set_param(component_id: &str, key: &str, value: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let comp = state
        .components
        .iter_mut()
        .find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;
    let param = comp
        .params
        .iter_mut()
        .find(|p| p.key == key)
        .ok_or(KernelError::NotFound)?;
    // Validate against allowed values if restricted.
    if !param.allowed.is_empty() && !param.allowed.iter().any(|a| a == value) {
        return Err(KernelError::InvalidArgument);
    }
    param.value = String::from(value);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Reset a parameter to its default.
pub fn reset_param(component_id: &str, key: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let comp = state
        .components
        .iter_mut()
        .find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;
    let param = comp
        .params
        .iter_mut()
        .find(|p| p.key == key)
        .ok_or(KernelError::NotFound)?;
    let default = param.default_value.clone();
    param.value = default;
    state.changes += 1;
    Ok(())
}

/// Reset all parameters to defaults.
pub fn reset_all_params(component_id: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let comp = state
        .components
        .iter_mut()
        .find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;
    for param in &mut comp.params {
        let default = param.default_value.clone();
        param.value = default;
    }
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Build operations
// ---------------------------------------------------------------------------

/// Set optimisation level.
pub fn set_opt_level(component_id: &str, level: OptLevel) -> KernelResult<()> {
    let mut state = STATE.lock();
    let comp = state
        .components
        .iter_mut()
        .find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;
    comp.opt_level = level;
    state.changes += 1;
    Ok(())
}

/// Set auto-rebuild flag.
pub fn set_auto_rebuild(component_id: &str, auto_rebuild: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let comp = state
        .components
        .iter_mut()
        .find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;
    comp.auto_rebuild = auto_rebuild;
    state.changes += 1;
    Ok(())
}

/// Add a dependency.
pub fn add_dependency(component_id: &str, dep_id: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    // Verify dependency exists.
    if !state.components.iter().any(|c| c.id == dep_id) {
        return Err(KernelError::NotFound);
    }
    let comp = state
        .components
        .iter_mut()
        .find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;
    if comp.dependencies.iter().any(|d| d == dep_id) {
        return Err(KernelError::AlreadyExists);
    }
    comp.dependencies.push(String::from(dep_id));
    state.changes += 1;
    Ok(())
}

/// Simulate source change detection.
pub fn detect_source_change(component_id: &str, new_hash: u64) -> KernelResult<bool> {
    let mut state = STATE.lock();
    let comp = state
        .components
        .iter_mut()
        .find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;
    comp.current_source_hash = new_hash;
    let changed =
        comp.current_source_hash != comp.last_source_hash && comp.status != BuildStatus::NeverBuilt;
    if changed {
        comp.status = BuildStatus::SourceChanged;
    }
    Ok(changed)
}

/// Check if source has changed since last build.
pub fn source_changed(component_id: &str) -> KernelResult<bool> {
    let state = STATE.lock();
    let comp = state
        .components
        .iter()
        .find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;
    Ok(comp.status == BuildStatus::SourceChanged
        || (comp.status != BuildStatus::NeverBuilt
            && comp.current_source_hash != comp.last_source_hash))
}

/// Scan all components for source changes.
pub fn scan_changed() -> Vec<String> {
    let state = STATE.lock();
    state
        .components
        .iter()
        .filter(|c| {
            c.status == BuildStatus::SourceChanged
                || (c.status != BuildStatus::NeverBuilt
                    && c.current_source_hash != c.last_source_hash)
        })
        .map(|c| c.id.clone())
        .collect()
}

/// Simulate building a component.
pub fn build(component_id: &str) -> KernelResult<()> {
    let timestamp = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();
    let comp = state
        .components
        .iter_mut()
        .find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;

    // Capture params snapshot.
    let params_snapshot: Vec<(String, String)> = comp
        .params
        .iter()
        .map(|p| (p.key.clone(), p.value.clone()))
        .collect();
    let cid = comp.id.clone();

    // Simulate build.
    comp.status = BuildStatus::Building;
    let duration_ms = 50; // Simulated build time.
    comp.last_source_hash = comp.current_source_hash;
    comp.last_build_ns = timestamp;
    comp.last_build_duration_ms = duration_ms;
    comp.build_count += 1;
    comp.status = BuildStatus::UpToDate;

    // Record log.
    if state.build_logs.len() >= MAX_BUILD_LOGS {
        state.build_logs.remove(0);
    }
    state.build_logs.push(BuildLog {
        component_id: cid,
        timestamp_ns: timestamp,
        success: true,
        duration_ms,
        output: String::from("Build successful"),
        params_snapshot,
    });

    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Simulate a failed build.
pub fn record_build_failure(component_id: &str, error_msg: &str) -> KernelResult<()> {
    let timestamp = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();
    let comp = state
        .components
        .iter_mut()
        .find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;

    let params_snapshot: Vec<(String, String)> = comp
        .params
        .iter()
        .map(|p| (p.key.clone(), p.value.clone()))
        .collect();
    let cid = comp.id.clone();

    comp.status = BuildStatus::Failed;
    comp.last_build_ns = timestamp;

    if state.build_logs.len() >= MAX_BUILD_LOGS {
        state.build_logs.remove(0);
    }
    state.build_logs.push(BuildLog {
        component_id: cid,
        timestamp_ns: timestamp,
        success: false,
        duration_ms: 0,
        output: String::from(error_msg),
        params_snapshot,
    });

    state.changes += 1;
    Ok(())
}

/// Get build logs for a component.
pub fn build_logs(component_id: &str) -> Vec<BuildLog> {
    STATE
        .lock()
        .build_logs
        .iter()
        .filter(|l| l.component_id == component_id)
        .cloned()
        .collect()
}

/// Get all build logs.
pub fn all_build_logs() -> Vec<BuildLog> {
    STATE.lock().build_logs.clone()
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

/// Initialise with default OS components.
pub fn init_defaults() {
    // Seed the real OS component taxonomy and its legitimate build-parameter
    // configuration. Per-component `build_count` is 0 and `last_build_ns` is 0:
    // these components have never been built in this session, so any non-zero
    // build_count would be a fabricated claim (kshell surfaces it as "Builds: N").
    let mut state = STATE.lock();

    let hash_kernel = simple_hash(b"/src/kernel");
    let hash_drivers = simple_hash(b"/src/drivers");
    let hash_compositor = simple_hash(b"/src/compositor");
    let hash_shell = simple_hash(b"/src/shell");
    let hash_init = simple_hash(b"/src/init");
    let hash_boot = simple_hash(b"/src/boot");

    state.components = vec![
        Component {
            id: String::from("kernel"),
            name: String::from("Kernel"),
            comp_type: ComponentType::Kernel,
            source_dir: PathBuf::from("/src/kernel"),
            output_path: PathBuf::from("/boot/kernel"),
            params: vec![
                BuildParam {
                    key: String::from("page_size"),
                    description: String::from("Page size in bytes"),
                    value: String::from("16384"),
                    default_value: String::from("16384"),
                    allowed: vec![
                        String::from("4096"),
                        String::from("16384"),
                        String::from("65536"),
                    ],
                    requires_full_rebuild: true,
                },
                BuildParam {
                    key: String::from("max_cpus"),
                    description: String::from("Maximum CPU count"),
                    value: String::from("256"),
                    default_value: String::from("256"),
                    allowed: Vec::new(),
                    requires_full_rebuild: true,
                },
                BuildParam {
                    key: String::from("preempt_model"),
                    description: String::from("Preemption model"),
                    value: String::from("full"),
                    default_value: String::from("full"),
                    allowed: vec![
                        String::from("none"),
                        String::from("voluntary"),
                        String::from("full"),
                        String::from("realtime"),
                    ],
                    requires_full_rebuild: true,
                },
                BuildParam {
                    key: String::from("debug_assertions"),
                    description: String::from("Enable debug assertions"),
                    value: String::from("false"),
                    default_value: String::from("false"),
                    allowed: vec![String::from("true"), String::from("false")],
                    requires_full_rebuild: false,
                },
            ],
            status: BuildStatus::UpToDate,
            last_source_hash: hash_kernel,
            current_source_hash: hash_kernel,
            last_build_ns: 0,
            last_build_duration_ms: 0,
            build_count: 0,
            system_critical: true,
            dependencies: Vec::new(),
            auto_rebuild: false,
            opt_level: OptLevel::Release,
        },
        Component {
            id: String::from("drivers"),
            name: String::from("Userspace Drivers"),
            comp_type: ComponentType::KernelModule,
            source_dir: PathBuf::from("/src/drivers"),
            output_path: PathBuf::from("/lib/drivers/"),
            params: vec![BuildParam {
                key: String::from("virtio"),
                description: String::from("Enable virtio drivers"),
                value: String::from("true"),
                default_value: String::from("true"),
                allowed: vec![String::from("true"), String::from("false")],
                requires_full_rebuild: false,
            }],
            status: BuildStatus::UpToDate,
            last_source_hash: hash_drivers,
            current_source_hash: hash_drivers,
            last_build_ns: 0,
            last_build_duration_ms: 0,
            build_count: 0,
            system_critical: false,
            dependencies: vec![String::from("kernel")],
            auto_rebuild: false,
            opt_level: OptLevel::Release,
        },
        Component {
            id: String::from("compositor"),
            name: String::from("Compositor"),
            comp_type: ComponentType::SystemService,
            source_dir: PathBuf::from("/src/compositor"),
            output_path: PathBuf::from("/usr/lib/compositor"),
            params: vec![BuildParam {
                key: String::from("gpu_backend"),
                description: String::from("GPU rendering backend"),
                value: String::from("vulkan"),
                default_value: String::from("vulkan"),
                allowed: vec![
                    String::from("vulkan"),
                    String::from("opengl"),
                    String::from("software"),
                ],
                requires_full_rebuild: true,
            }],
            status: BuildStatus::UpToDate,
            last_source_hash: hash_compositor,
            current_source_hash: hash_compositor,
            last_build_ns: 0,
            last_build_duration_ms: 0,
            build_count: 0,
            system_critical: true,
            dependencies: vec![String::from("kernel")],
            auto_rebuild: false,
            opt_level: OptLevel::Release,
        },
        Component {
            id: String::from("shell"),
            name: String::from("Shell"),
            comp_type: ComponentType::CoreUtility,
            source_dir: PathBuf::from("/src/shell"),
            output_path: PathBuf::from("/usr/bin/shell"),
            params: Vec::new(),
            status: BuildStatus::UpToDate,
            last_source_hash: hash_shell,
            current_source_hash: hash_shell,
            last_build_ns: 0,
            last_build_duration_ms: 0,
            build_count: 0,
            system_critical: false,
            dependencies: Vec::new(),
            auto_rebuild: false,
            opt_level: OptLevel::Release,
        },
        Component {
            id: String::from("init"),
            name: String::from("Init System"),
            comp_type: ComponentType::SystemService,
            source_dir: PathBuf::from("/src/init"),
            output_path: PathBuf::from("/sbin/init"),
            params: Vec::new(),
            status: BuildStatus::UpToDate,
            last_source_hash: hash_init,
            current_source_hash: hash_init,
            last_build_ns: 0,
            last_build_duration_ms: 0,
            build_count: 0,
            system_critical: true,
            dependencies: vec![String::from("kernel")],
            auto_rebuild: false,
            opt_level: OptLevel::Release,
        },
        Component {
            id: String::from("bootloader"),
            name: String::from("Bootloader"),
            comp_type: ComponentType::Bootloader,
            source_dir: PathBuf::from("/src/boot"),
            output_path: PathBuf::from("/boot/efi/boot.efi"),
            params: vec![BuildParam {
                key: String::from("secure_boot"),
                description: String::from("Sign for Secure Boot"),
                value: String::from("false"),
                default_value: String::from("false"),
                allowed: vec![String::from("true"), String::from("false")],
                requires_full_rebuild: true,
            }],
            status: BuildStatus::UpToDate,
            last_source_hash: hash_boot,
            current_source_hash: hash_boot,
            last_build_ns: 0,
            last_build_duration_ms: 0,
            build_count: 0,
            system_critical: true,
            dependencies: Vec::new(),
            auto_rebuild: false,
            opt_level: OptLevel::Release,
        },
    ];

    state.build_logs.clear();
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Return (component_count, built_count, changed_count, ops).
pub fn stats() -> (usize, usize, usize, u64) {
    let state = STATE.lock();
    let built = state
        .components
        .iter()
        .filter(|c| c.status == BuildStatus::UpToDate)
        .count();
    let changed = state
        .components
        .iter()
        .filter(|c| c.status == BuildStatus::SourceChanged)
        .count();
    (
        state.components.len(),
        built,
        changed,
        OP_COUNT.load(Ordering::Relaxed),
    )
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.components.clear();
    state.build_logs.clear();
    state.changes = 0;
    OP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Decline to run against a populated registry rather than emptying it.
    //
    // This suite used to open with `clear_all()`. That did make it idempotent,
    // and it did make test 1's implicit "the fixture is not already here" true
    // by construction -- at the price of `kbuild test` deleting every
    // component the user had registered, plus the whole build history, from a
    // shell command whose name promises a report rather than a wipe.
    //
    // The assertions here cannot be restated relative to a baseline: test 10
    // registers a *system-critical* component that `remove_component` then
    // correctly refuses to delete, so the suite has no way to clean up after
    // itself. Declining is therefore the only honest option. At boot the
    // registry is genuinely empty -- nothing calls `init_defaults()` outside
    // the `kbuild` shell commands -- so full coverage is retained where it
    // matters.
    if !list_components().is_empty() {
        serial_println!(
            "[kernelbuild] self-test skipped: {} component(s) already registered",
            list_components().len()
        );
        return Ok(());
    }
    reset_stats();

    // Test 1: register component.
    serial_println!("kernelbuild::self_test 1: register");
    register_component(
        "kbuild-selftest-kern",
        "Test Kernel",
        ComponentType::Kernel,
        "/tmp/.kernelbuild-selftest/src",
        "/tmp/.kernelbuild-selftest/out",
    )?;
    let comp = get_component("kbuild-selftest-kern")?;
    assert_eq!(comp.name, "Test Kernel");
    assert_eq!(comp.status, BuildStatus::NeverBuilt);
    assert!(comp.system_critical);

    // Test 2: duplicate registration.
    serial_println!("kernelbuild::self_test 2: duplicate");
    assert!(
        register_component(
            "kbuild-selftest-kern",
            "Dup",
            ComponentType::Kernel,
            "/tmp/.kernelbuild-selftest/dup",
            "/tmp/.kernelbuild-selftest/dupout"
        )
        .is_err()
    );

    // Test 3: add parameter.
    serial_println!("kernelbuild::self_test 3: parameters");
    add_param(
        "kbuild-selftest-kern",
        "page_size",
        "Page size",
        "16384",
        &["4096", "16384", "65536"],
        true,
    )?;
    let comp = get_component("kbuild-selftest-kern")?;
    assert_eq!(comp.params.len(), 1);
    assert_eq!(comp.params[0].value, "16384");

    // Test 4: set parameter.
    serial_println!("kernelbuild::self_test 4: set param");
    set_param("kbuild-selftest-kern", "page_size", "4096")?;
    let comp = get_component("kbuild-selftest-kern")?;
    assert_eq!(comp.params[0].value, "4096");
    // Invalid value.
    assert!(set_param("kbuild-selftest-kern", "page_size", "1234").is_err());

    // Test 5: reset parameter.
    serial_println!("kernelbuild::self_test 5: reset param");
    reset_param("kbuild-selftest-kern", "page_size")?;
    let comp = get_component("kbuild-selftest-kern")?;
    assert_eq!(comp.params[0].value, "16384");

    // Test 6: build.
    serial_println!("kernelbuild::self_test 6: build");
    build("kbuild-selftest-kern")?;
    let comp = get_component("kbuild-selftest-kern")?;
    assert_eq!(comp.status, BuildStatus::UpToDate);
    assert_eq!(comp.build_count, 1);
    let logs = build_logs("kbuild-selftest-kern");
    assert_eq!(logs.len(), 1);
    assert!(logs[0].success);

    // Test 7: source change detection.
    serial_println!("kernelbuild::self_test 7: source change");
    detect_source_change("kbuild-selftest-kern", 999)?;
    let comp = get_component("kbuild-selftest-kern")?;
    assert_eq!(comp.status, BuildStatus::SourceChanged);
    assert!(source_changed("kbuild-selftest-kern")?);
    let changed = scan_changed();
    assert!(changed.contains(&String::from("kbuild-selftest-kern")));

    // Test 8: rebuild after change.
    serial_println!("kernelbuild::self_test 8: rebuild");
    build("kbuild-selftest-kern")?;
    let comp = get_component("kbuild-selftest-kern")?;
    assert_eq!(comp.status, BuildStatus::UpToDate);
    assert_eq!(comp.build_count, 2);

    // Test 9: build failure.
    serial_println!("kernelbuild::self_test 9: build failure");
    record_build_failure("kbuild-selftest-kern", "missing dependency")?;
    let comp = get_component("kbuild-selftest-kern")?;
    assert_eq!(comp.status, BuildStatus::Failed);
    let logs = build_logs("kbuild-selftest-kern");
    assert_eq!(logs.len(), 3);
    assert!(!logs[2].success);

    // Test 10: remove (system_critical fails).
    serial_println!("kernelbuild::self_test 10: remove");
    assert!(remove_component("kbuild-selftest-kern").is_err());
    // Register and remove non-critical.
    register_component(
        "kbuild-selftest-util",
        "Util",
        ComponentType::CoreUtility,
        "/tmp/.kernelbuild-selftest/util",
        "/tmp/.kernelbuild-selftest/utilout",
    )?;
    remove_component("kbuild-selftest-util")?;
    assert!(get_component("kbuild-selftest-util").is_err());

    // Test 11: init_defaults.
    serial_println!("kernelbuild::self_test 11: defaults");
    init_defaults();
    let comps = list_components();
    assert!(comps.len() >= 5);
    clear_all();

    // Test 12: non-UTF-8 source and output paths survive registration, and
    // two source trees differing only in an unencodable byte are distinct.
    //
    // `\xFF` and `\xFE` have no UTF-8 spelling in any position, so under the
    // old `String` typing both folded to the same U+FFFD-bearing name. Two
    // consequences, neither of them cosmetic: the change-detection hash is
    // computed over the source directory, so the two trees hashed identically
    // and a rebuild of one was suppressed as "up to date" because the other
    // had been built; and the output path is where the artefact is written,
    // so a build wrote over a file the user never named.
    // See design-decisions.md §261.
    serial_println!("kernelbuild::self_test 12: non-UTF-8 paths");
    let raw_src_a = Path::new(b"/tmp/.kernelbuild-selftest/s\xFFrc");
    let raw_src_b = Path::new(b"/tmp/.kernelbuild-selftest/s\xFErc");
    let raw_out = Path::new(b"/tmp/.kernelbuild-selftest/o\xFFut/img.bin");
    register_component(
        "kbuild-selftest-raw-a",
        "Raw A",
        ComponentType::CoreUtility,
        raw_src_a,
        raw_out,
    )?;
    register_component(
        "kbuild-selftest-raw-b",
        "Raw B",
        ComponentType::CoreUtility,
        raw_src_b,
        raw_out,
    )?;
    let a = get_component("kbuild-selftest-raw-a")?;
    let b = get_component("kbuild-selftest-raw-b")?;
    assert_eq!(
        a.source_dir.as_path().as_bytes(),
        &b"/tmp/.kernelbuild-selftest/s\xFFrc"[..],
        "the source directory must be stored byte-for-byte"
    );
    assert_eq!(
        a.output_path.as_path().as_bytes(),
        &b"/tmp/.kernelbuild-selftest/o\xFFut/img.bin"[..],
        "the output path must be stored byte-for-byte"
    );
    assert_ne!(
        a.source_dir, b.source_dir,
        "two source trees differing only in an unencodable byte must stay distinct"
    );
    assert_ne!(
        a.current_source_hash, b.current_source_hash,
        "the change-detection hash must distinguish them, or one build masks the other"
    );
    // The payoff: once both are built, editing A's tree must mark A alone
    // stale. `source_changed` reports nothing for a `NeverBuilt` component,
    // so both have to be built before the question is even meaningful.
    build("kbuild-selftest-raw-a")?;
    build("kbuild-selftest-raw-b")?;
    assert!(!source_changed("kbuild-selftest-raw-a")?);
    assert!(!source_changed("kbuild-selftest-raw-b")?);
    detect_source_change("kbuild-selftest-raw-a", 1234)?;
    assert!(source_changed("kbuild-selftest-raw-a")?);
    assert!(
        !source_changed("kbuild-selftest-raw-b")?,
        "the sibling spelling must not be dragged into a rebuild"
    );
    remove_component("kbuild-selftest-raw-a")?;
    remove_component("kbuild-selftest-raw-b")?;

    // Back to the empty registry we insisted on at entry -- test 11's defaults
    // and test 12's build logs included.
    clear_all();
    serial_println!("kernelbuild::self_test: all 12 tests passed");
    Ok(())
}
