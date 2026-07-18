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

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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
    pub source_dir: String,
    /// Output path (binary / library).
    pub output_path: String,
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

fn simple_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
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
    source_dir: &str,
    output_path: &str,
) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.components.len() >= MAX_COMPONENTS {
        return Err(KernelError::ResourceExhausted);
    }
    if state.components.iter().any(|c| c.id == id) {
        return Err(KernelError::AlreadyExists);
    }
    let hash = simple_hash(source_dir);
    state.components.push(Component {
        id: String::from(id),
        name: String::from(name),
        comp_type,
        source_dir: String::from(source_dir),
        output_path: String::from(output_path),
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
    let idx = state.components.iter().position(|c| c.id == id)
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
    state.components.iter().find(|c| c.id == id)
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
    let comp = state.components.iter_mut().find(|c| c.id == component_id)
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
    let comp = state.components.iter_mut().find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;
    let param = comp.params.iter_mut().find(|p| p.key == key)
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
    let comp = state.components.iter_mut().find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;
    let param = comp.params.iter_mut().find(|p| p.key == key)
        .ok_or(KernelError::NotFound)?;
    let default = param.default_value.clone();
    param.value = default;
    state.changes += 1;
    Ok(())
}

/// Reset all parameters to defaults.
pub fn reset_all_params(component_id: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let comp = state.components.iter_mut().find(|c| c.id == component_id)
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
    let comp = state.components.iter_mut().find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;
    comp.opt_level = level;
    state.changes += 1;
    Ok(())
}

/// Set auto-rebuild flag.
pub fn set_auto_rebuild(component_id: &str, auto_rebuild: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let comp = state.components.iter_mut().find(|c| c.id == component_id)
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
    let comp = state.components.iter_mut().find(|c| c.id == component_id)
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
    let comp = state.components.iter_mut().find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;
    comp.current_source_hash = new_hash;
    let changed = comp.current_source_hash != comp.last_source_hash
        && comp.status != BuildStatus::NeverBuilt;
    if changed {
        comp.status = BuildStatus::SourceChanged;
    }
    Ok(changed)
}

/// Check if source has changed since last build.
pub fn source_changed(component_id: &str) -> KernelResult<bool> {
    let state = STATE.lock();
    let comp = state.components.iter().find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;
    Ok(comp.status == BuildStatus::SourceChanged
        || (comp.status != BuildStatus::NeverBuilt
            && comp.current_source_hash != comp.last_source_hash))
}

/// Scan all components for source changes.
pub fn scan_changed() -> Vec<String> {
    let state = STATE.lock();
    state.components.iter()
        .filter(|c| c.status == BuildStatus::SourceChanged
            || (c.status != BuildStatus::NeverBuilt
                && c.current_source_hash != c.last_source_hash))
        .map(|c| c.id.clone())
        .collect()
}

/// Simulate building a component.
pub fn build(component_id: &str) -> KernelResult<()> {
    let timestamp = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();
    let comp = state.components.iter_mut().find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;

    // Capture params snapshot.
    let params_snapshot: Vec<(String, String)> = comp.params.iter()
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
    let comp = state.components.iter_mut().find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;

    let params_snapshot: Vec<(String, String)> = comp.params.iter()
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
    STATE.lock().build_logs.iter()
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

    let hash_kernel = simple_hash("/src/kernel");
    let hash_drivers = simple_hash("/src/drivers");
    let hash_compositor = simple_hash("/src/compositor");
    let hash_shell = simple_hash("/src/shell");
    let hash_init = simple_hash("/src/init");
    let hash_boot = simple_hash("/src/boot");

    state.components = vec![
        Component {
            id: String::from("kernel"),
            name: String::from("Kernel"),
            comp_type: ComponentType::Kernel,
            source_dir: String::from("/src/kernel"),
            output_path: String::from("/boot/kernel"),
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
            source_dir: String::from("/src/drivers"),
            output_path: String::from("/lib/drivers/"),
            params: vec![
                BuildParam {
                    key: String::from("virtio"),
                    description: String::from("Enable virtio drivers"),
                    value: String::from("true"),
                    default_value: String::from("true"),
                    allowed: vec![String::from("true"), String::from("false")],
                    requires_full_rebuild: false,
                },
            ],
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
            source_dir: String::from("/src/compositor"),
            output_path: String::from("/usr/lib/compositor"),
            params: vec![
                BuildParam {
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
                },
            ],
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
            source_dir: String::from("/src/shell"),
            output_path: String::from("/usr/bin/shell"),
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
            source_dir: String::from("/src/init"),
            output_path: String::from("/sbin/init"),
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
            source_dir: String::from("/src/boot"),
            output_path: String::from("/boot/efi/boot.efi"),
            params: vec![
                BuildParam {
                    key: String::from("secure_boot"),
                    description: String::from("Sign for Secure Boot"),
                    value: String::from("false"),
                    default_value: String::from("false"),
                    allowed: vec![String::from("true"), String::from("false")],
                    requires_full_rebuild: true,
                },
            ],
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
    let built = state.components.iter()
        .filter(|c| c.status == BuildStatus::UpToDate)
        .count();
    let changed = state.components.iter()
        .filter(|c| c.status == BuildStatus::SourceChanged)
        .count();
    (state.components.len(), built, changed, OP_COUNT.load(Ordering::Relaxed))
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

    clear_all();

    // Test 1: register component.
    serial_println!("kernelbuild::self_test 1: register");
    register_component("test-kern", "Test Kernel", ComponentType::Kernel,
        "/src/test", "/boot/test")?;
    let comp = get_component("test-kern")?;
    assert_eq!(comp.name, "Test Kernel");
    assert_eq!(comp.status, BuildStatus::NeverBuilt);
    assert!(comp.system_critical);

    // Test 2: duplicate registration.
    serial_println!("kernelbuild::self_test 2: duplicate");
    assert!(register_component("test-kern", "Dup", ComponentType::Kernel,
        "/src/dup", "/boot/dup").is_err());

    // Test 3: add parameter.
    serial_println!("kernelbuild::self_test 3: parameters");
    add_param("test-kern", "page_size", "Page size", "16384",
        &["4096", "16384", "65536"], true)?;
    let comp = get_component("test-kern")?;
    assert_eq!(comp.params.len(), 1);
    assert_eq!(comp.params[0].value, "16384");

    // Test 4: set parameter.
    serial_println!("kernelbuild::self_test 4: set param");
    set_param("test-kern", "page_size", "4096")?;
    let comp = get_component("test-kern")?;
    assert_eq!(comp.params[0].value, "4096");
    // Invalid value.
    assert!(set_param("test-kern", "page_size", "1234").is_err());

    // Test 5: reset parameter.
    serial_println!("kernelbuild::self_test 5: reset param");
    reset_param("test-kern", "page_size")?;
    let comp = get_component("test-kern")?;
    assert_eq!(comp.params[0].value, "16384");

    // Test 6: build.
    serial_println!("kernelbuild::self_test 6: build");
    build("test-kern")?;
    let comp = get_component("test-kern")?;
    assert_eq!(comp.status, BuildStatus::UpToDate);
    assert_eq!(comp.build_count, 1);
    let logs = build_logs("test-kern");
    assert_eq!(logs.len(), 1);
    assert!(logs[0].success);

    // Test 7: source change detection.
    serial_println!("kernelbuild::self_test 7: source change");
    detect_source_change("test-kern", 999)?;
    let comp = get_component("test-kern")?;
    assert_eq!(comp.status, BuildStatus::SourceChanged);
    assert!(source_changed("test-kern")?);
    let changed = scan_changed();
    assert!(changed.contains(&String::from("test-kern")));

    // Test 8: rebuild after change.
    serial_println!("kernelbuild::self_test 8: rebuild");
    build("test-kern")?;
    let comp = get_component("test-kern")?;
    assert_eq!(comp.status, BuildStatus::UpToDate);
    assert_eq!(comp.build_count, 2);

    // Test 9: build failure.
    serial_println!("kernelbuild::self_test 9: build failure");
    record_build_failure("test-kern", "missing dependency")?;
    let comp = get_component("test-kern")?;
    assert_eq!(comp.status, BuildStatus::Failed);
    let logs = build_logs("test-kern");
    assert_eq!(logs.len(), 3);
    assert!(!logs[2].success);

    // Test 10: remove (system_critical fails).
    serial_println!("kernelbuild::self_test 10: remove");
    assert!(remove_component("test-kern").is_err());
    // Register and remove non-critical.
    register_component("test-util", "Util", ComponentType::CoreUtility,
        "/src/util", "/bin/util")?;
    remove_component("test-util")?;
    assert!(get_component("test-util").is_err());

    // Test 11: init_defaults.
    serial_println!("kernelbuild::self_test 11: defaults");
    init_defaults();
    let comps = list_components();
    assert!(comps.len() >= 5);

    clear_all();
    serial_println!("kernelbuild::self_test: all 11 tests passed");
    Ok(())
}
