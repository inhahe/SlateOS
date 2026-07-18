//! Scripting language registration — OS-wide embeddable script engine registry.
//!
//! Provides an ActiveScript-like system where scripting languages (Lua, WASM,
//! Python, etc.) can register with the OS so any application can embed
//! scripting via a common API.
//!
//! ## Design Reference
//!
//! design.txt lines 1331-1336:
//! "something like ActiveScript? — can register a scripting language with
//! the OS so that any application that employs scripting via a common API."
//! "The modern equivalent: embed Lua or a WASM runtime as a standard
//! extension mechanism. Lua is tiny, fast, designed for embedding. WASM
//! is sandboxed and language-agnostic. Probably offer both."
//!
//! ## Architecture
//!
//! ```text
//! Language provider (e.g., Lua runtime)
//!   → scriptlang::register(EngineInfo { ... })
//!
//! Application wants scripting
//!   → scriptlang::list_engines() → pick one
//!   → scriptlang::create_context(engine_id) → ContextHandle
//!   → scriptlang::eval(handle, code) → result
//!   → scriptlang::destroy_context(handle)
//!
//! OS extension system
//!   → scriptlang::file_extensions("lua") → engine_id
//!   → scriptlang::run_file(engine_id, path) → result
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

/// The type/category of a scripting engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineType {
    /// Traditional interpreted scripting (Lua, Python, Ruby).
    Interpreted,
    /// JIT-compiled scripting (LuaJIT, V8).
    Jit,
    /// WebAssembly runtime (sandboxed, language-agnostic).
    Wasm,
    /// Shell/command scripting.
    Shell,
    /// Domain-specific language.
    Dsl,
    /// Compiled AOT (e.g., fastpy).
    Compiled,
}

/// Sandbox level that the engine supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxLevel {
    /// No sandboxing — full access.
    None,
    /// Basic sandboxing — limited filesystem, no network.
    Basic,
    /// Strict sandboxing — no filesystem, no network, memory limited.
    Strict,
    /// WASM-style sandboxing — capability-based, formally verifiable.
    Capability,
}

/// A registered scripting engine.
#[derive(Debug, Clone)]
pub struct EngineInfo {
    /// Unique engine ID.
    pub id: u64,
    /// Engine name (e.g., "Lua 5.4", "Wasmtime").
    pub name: String,
    /// Version string.
    pub version: String,
    /// Engine type.
    pub engine_type: EngineType,
    /// Sandbox level.
    pub sandbox: SandboxLevel,
    /// File extensions handled (e.g., ["lua", "luac"]).
    pub extensions: Vec<String>,
    /// MIME types handled.
    pub mime_types: Vec<String>,
    /// Path to the engine binary/library.
    pub engine_path: String,
    /// Whether this engine is built into the OS.
    pub builtin: bool,
    /// Whether this engine is currently available.
    pub enabled: bool,
    /// Maximum memory per context (bytes, 0 = unlimited).
    pub max_memory: u64,
    /// Maximum execution time per eval (ms, 0 = unlimited).
    pub max_time_ms: u64,
    /// Description.
    pub description: String,
}

/// An active scripting context (execution environment).
#[derive(Debug, Clone)]
pub struct ScriptContext {
    /// Context handle.
    pub handle: u64,
    /// Engine ID this context belongs to.
    pub engine_id: u64,
    /// Requesting application ID.
    pub app_id: String,
    /// Creation timestamp (ns).
    pub created_ns: u64,
    /// Number of evaluations performed.
    pub eval_count: u64,
    /// Whether the context is currently executing.
    pub executing: bool,
    /// Variables/bindings registered in this context.
    pub bindings: Vec<ScriptBinding>,
}

/// A host-provided binding (variable/function) exposed to scripts.
#[derive(Debug, Clone)]
pub struct ScriptBinding {
    /// Binding name (e.g., "print", "app_data").
    pub name: String,
    /// Type description (e.g., "function", "string", "table").
    pub type_desc: String,
    /// Whether the script can modify this binding.
    pub writable: bool,
}

/// Result of script evaluation.
#[derive(Debug, Clone)]
pub struct EvalResult {
    /// Whether execution succeeded.
    pub success: bool,
    /// Output/return value (string representation).
    pub output: String,
    /// Error message (if any).
    pub error: String,
    /// Execution time in microseconds.
    pub duration_us: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    engines: Vec<EngineInfo>,
    contexts: Vec<ScriptContext>,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    engines: Vec::new(),
    contexts: Vec::new(),
    changes: 0,
});

static NEXT_ENGINE_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_CONTEXT_ID: AtomicU64 = AtomicU64::new(1);
static EVAL_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Engine registration
// ---------------------------------------------------------------------------

/// Register a new scripting engine.
pub fn register_engine(
    name: &str,
    version: &str,
    engine_type: EngineType,
    sandbox: SandboxLevel,
    engine_path: &str,
    description: &str,
    builtin: bool,
) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.engines.len() >= 128 {
        return Err(KernelError::ResourceExhausted);
    }
    if state.engines.iter().any(|e| e.name == name && e.version == version) {
        return Err(KernelError::AlreadyExists);
    }
    let id = NEXT_ENGINE_ID.fetch_add(1, Ordering::Relaxed);
    state.engines.push(EngineInfo {
        id,
        name: String::from(name),
        version: String::from(version),
        engine_type,
        sandbox,
        extensions: Vec::new(),
        mime_types: Vec::new(),
        engine_path: String::from(engine_path),
        builtin,
        enabled: true,
        max_memory: 0,
        max_time_ms: 0,
        description: String::from(description),
    });
    state.changes += 1;
    Ok(id)
}

/// Unregister a scripting engine.
pub fn unregister_engine(engine_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    // Prevent removal if active contexts exist.
    if state.contexts.iter().any(|c| c.engine_id == engine_id) {
        return Err(KernelError::NotEmpty);
    }
    let before = state.engines.len();
    state.engines.retain(|e| e.id != engine_id);
    if state.engines.len() == before {
        return Err(KernelError::NotFound);
    }
    state.changes += 1;
    Ok(())
}

/// Get engine info by ID.
pub fn get_engine(engine_id: u64) -> KernelResult<EngineInfo> {
    let state = STATE.lock();
    state
        .engines
        .iter()
        .find(|e| e.id == engine_id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all registered engines.
pub fn list_engines() -> Vec<EngineInfo> {
    STATE.lock().engines.clone()
}

/// Enable or disable an engine.
pub fn set_enabled(engine_id: u64, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let engine = state
        .engines
        .iter_mut()
        .find(|e| e.id == engine_id)
        .ok_or(KernelError::NotFound)?;
    engine.enabled = enabled;
    state.changes += 1;
    Ok(())
}

/// Add a file extension mapping to an engine.
pub fn add_extension(engine_id: u64, ext: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let engine = state
        .engines
        .iter_mut()
        .find(|e| e.id == engine_id)
        .ok_or(KernelError::NotFound)?;
    let ext_s = String::from(ext);
    if !engine.extensions.contains(&ext_s) {
        if engine.extensions.len() >= 32 {
            return Err(KernelError::ResourceExhausted);
        }
        engine.extensions.push(ext_s);
        state.changes += 1;
    }
    Ok(())
}

/// Add a MIME type mapping to an engine.
pub fn add_mime_type(engine_id: u64, mime: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let engine = state
        .engines
        .iter_mut()
        .find(|e| e.id == engine_id)
        .ok_or(KernelError::NotFound)?;
    let mime_s = String::from(mime);
    if !engine.mime_types.contains(&mime_s) {
        if engine.mime_types.len() >= 32 {
            return Err(KernelError::ResourceExhausted);
        }
        engine.mime_types.push(mime_s);
        state.changes += 1;
    }
    Ok(())
}

/// Set resource limits for an engine.
pub fn set_limits(engine_id: u64, max_memory: u64, max_time_ms: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let engine = state
        .engines
        .iter_mut()
        .find(|e| e.id == engine_id)
        .ok_or(KernelError::NotFound)?;
    engine.max_memory = max_memory;
    engine.max_time_ms = max_time_ms;
    state.changes += 1;
    Ok(())
}

/// Find engine by file extension.
pub fn engine_for_extension(ext: &str) -> Option<EngineInfo> {
    let state = STATE.lock();
    state
        .engines
        .iter()
        .find(|e| e.enabled && e.extensions.iter().any(|x| x == ext))
        .cloned()
}

/// Find engine by MIME type.
pub fn engine_for_mime(mime: &str) -> Option<EngineInfo> {
    let state = STATE.lock();
    state
        .engines
        .iter()
        .find(|e| e.enabled && e.mime_types.iter().any(|m| m == mime))
        .cloned()
}

// ---------------------------------------------------------------------------
// Execution contexts
// ---------------------------------------------------------------------------

/// Create a new scripting context for an engine.
pub fn create_context(engine_id: u64, app_id: &str) -> KernelResult<u64> {
    let mut state = STATE.lock();
    // Verify engine exists and is enabled.
    let engine = state
        .engines
        .iter()
        .find(|e| e.id == engine_id)
        .ok_or(KernelError::NotFound)?;
    if !engine.enabled {
        return Err(KernelError::PermissionDenied);
    }
    if state.contexts.len() >= 1024 {
        return Err(KernelError::ResourceExhausted);
    }
    let handle = NEXT_CONTEXT_ID.fetch_add(1, Ordering::Relaxed);
    let now = crate::hpet::elapsed_ns();
    state.contexts.push(ScriptContext {
        handle,
        engine_id,
        app_id: String::from(app_id),
        created_ns: now,
        eval_count: 0,
        executing: false,
        bindings: Vec::new(),
    });
    state.changes += 1;
    Ok(handle)
}

/// Destroy a scripting context.
pub fn destroy_context(handle: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let before = state.contexts.len();
    state.contexts.retain(|c| c.handle != handle);
    if state.contexts.len() == before {
        return Err(KernelError::NotFound);
    }
    state.changes += 1;
    Ok(())
}

/// Get context info.
pub fn get_context(handle: u64) -> KernelResult<ScriptContext> {
    let state = STATE.lock();
    state
        .contexts
        .iter()
        .find(|c| c.handle == handle)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all active contexts.
pub fn list_contexts() -> Vec<ScriptContext> {
    STATE.lock().contexts.clone()
}

/// Add a binding (host-provided variable/function) to a context.
pub fn add_binding(
    handle: u64,
    name: &str,
    type_desc: &str,
    writable: bool,
) -> KernelResult<()> {
    let mut state = STATE.lock();
    let ctx = state
        .contexts
        .iter_mut()
        .find(|c| c.handle == handle)
        .ok_or(KernelError::NotFound)?;
    if ctx.bindings.iter().any(|b| b.name == name) {
        return Err(KernelError::AlreadyExists);
    }
    if ctx.bindings.len() >= 256 {
        return Err(KernelError::ResourceExhausted);
    }
    ctx.bindings.push(ScriptBinding {
        name: String::from(name),
        type_desc: String::from(type_desc),
        writable,
    });
    state.changes += 1;
    Ok(())
}

/// Remove a binding from a context.
pub fn remove_binding(handle: u64, name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let ctx = state
        .contexts
        .iter_mut()
        .find(|c| c.handle == handle)
        .ok_or(KernelError::NotFound)?;
    let before = ctx.bindings.len();
    ctx.bindings.retain(|b| b.name != name);
    if ctx.bindings.len() == before {
        return Err(KernelError::NotFound);
    }
    state.changes += 1;
    Ok(())
}

/// Simulate evaluating code in a context.
/// In a real OS, this would dispatch to the engine runtime.
pub fn eval(handle: u64, _code: &str) -> KernelResult<EvalResult> {
    let mut state = STATE.lock();
    let ctx = state
        .contexts
        .iter_mut()
        .find(|c| c.handle == handle)
        .ok_or(KernelError::NotFound)?;
    if ctx.executing {
        return Err(KernelError::WouldBlock);
    }
    ctx.eval_count += 1;
    EVAL_COUNT.fetch_add(1, Ordering::Relaxed);
    // Simulated result — real implementation would call engine.
    Ok(EvalResult {
        success: true,
        output: String::from("(simulated)"),
        error: String::new(),
        duration_us: 1,
    })
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

/// Initialize with built-in scripting engines.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.engines.is_empty() {
        return;
    }

    let next_id = || NEXT_ENGINE_ID.fetch_add(1, Ordering::Relaxed);

    // Lua 5.4 — tiny, fast, designed for embedding.
    let lua_id = next_id();
    state.engines.push(EngineInfo {
        id: lua_id,
        name: String::from("Lua"),
        version: String::from("5.4"),
        engine_type: EngineType::Interpreted,
        sandbox: SandboxLevel::Basic,
        extensions: alloc::vec![String::from("lua"), String::from("luac")],
        mime_types: alloc::vec![String::from("text/x-lua")],
        engine_path: String::from("/usr/lib/scripting/lua54"),
        builtin: true,
        enabled: true,
        max_memory: 64 * 1024 * 1024, // 64 MiB per context
        max_time_ms: 30_000,           // 30 seconds
        description: String::from("Lightweight embeddable scripting language"),
    });

    // WASM runtime — sandboxed, language-agnostic.
    let wasm_id = next_id();
    state.engines.push(EngineInfo {
        id: wasm_id,
        name: String::from("Wasmtime"),
        version: String::from("1.0"),
        engine_type: EngineType::Wasm,
        sandbox: SandboxLevel::Capability,
        extensions: alloc::vec![String::from("wasm"), String::from("wat")],
        mime_types: alloc::vec![String::from("application/wasm")],
        engine_path: String::from("/usr/lib/scripting/wasmtime"),
        builtin: true,
        enabled: true,
        max_memory: 256 * 1024 * 1024, // 256 MiB per context
        max_time_ms: 60_000,           // 60 seconds
        description: String::from("WebAssembly runtime with capability-based sandboxing"),
    });

    // Shell scripting — OS command interpreter.
    let _sh_id = next_id();
    state.engines.push(EngineInfo {
        id: _sh_id,
        name: String::from("Shell"),
        version: String::from("1.0"),
        engine_type: EngineType::Shell,
        sandbox: SandboxLevel::None,
        extensions: alloc::vec![String::from("sh"), String::from("bash")],
        mime_types: alloc::vec![String::from("text/x-shellscript")],
        engine_path: String::from("/usr/bin/sh"),
        builtin: true,
        enabled: true,
        max_memory: 0,    // no limit
        max_time_ms: 0,   // no limit
        description: String::from("OS shell command interpreter"),
    });

    // Python via fastpy — AOT compiled Python.
    let _py_id = next_id();
    state.engines.push(EngineInfo {
        id: _py_id,
        name: String::from("Fastpy"),
        version: String::from("1.0"),
        engine_type: EngineType::Compiled,
        sandbox: SandboxLevel::Basic,
        extensions: alloc::vec![String::from("py"), String::from("pyw")],
        mime_types: alloc::vec![String::from("text/x-python")],
        engine_path: String::from("/usr/lib/scripting/fastpy"),
        builtin: true,
        enabled: true,
        max_memory: 512 * 1024 * 1024, // 512 MiB
        max_time_ms: 0,               // no limit
        description: String::from("AOT-compiled Python via fastpy"),
    });

    // JavaScript — for web content scripting.
    let _js_id = next_id();
    state.engines.push(EngineInfo {
        id: _js_id,
        name: String::from("QuickJS"),
        version: String::from("0.1"),
        engine_type: EngineType::Interpreted,
        sandbox: SandboxLevel::Strict,
        extensions: alloc::vec![String::from("js"), String::from("mjs")],
        mime_types: alloc::vec![
            String::from("application/javascript"),
            String::from("text/javascript"),
        ],
        engine_path: String::from("/usr/lib/scripting/quickjs"),
        builtin: true,
        enabled: true,
        max_memory: 128 * 1024 * 1024, // 128 MiB
        max_time_ms: 30_000,
        description: String::from("Lightweight JavaScript engine for embedding"),
    });

    state.changes += 1;
}

/// Return (engine_count, context_count, total_evals, changes).
pub fn stats() -> (usize, usize, u64, u64) {
    let state = STATE.lock();
    let engines = state.engines.len();
    let contexts = state.contexts.len();
    let evals = EVAL_COUNT.load(Ordering::Relaxed);
    (engines, contexts, evals, state.changes)
}

pub fn reset_stats() {
    EVAL_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.engines.clear();
    state.contexts.clear();
    state.changes = 0;
    NEXT_ENGINE_ID.store(1, Ordering::Relaxed);
    NEXT_CONTEXT_ID.store(1, Ordering::Relaxed);
    EVAL_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: register engines.
    serial_println!("scriptlang::self_test 1: register engines");
    let lua = register_engine(
        "TestLua", "5.4", EngineType::Interpreted, SandboxLevel::Basic,
        "/usr/lib/lua", "test lua engine", true,
    )?;
    let wasm = register_engine(
        "TestWasm", "1.0", EngineType::Wasm, SandboxLevel::Capability,
        "/usr/lib/wasm", "test wasm engine", true,
    )?;
    assert_eq!(list_engines().len(), 2);
    // Duplicate name+version fails.
    assert!(register_engine(
        "TestLua", "5.4", EngineType::Interpreted, SandboxLevel::Basic,
        "/x", "dup", false,
    ).is_err());

    // Test 2: extensions and MIME types.
    serial_println!("scriptlang::self_test 2: extensions and MIME");
    add_extension(lua, "lua")?;
    add_extension(lua, "luac")?;
    add_mime_type(lua, "text/x-lua")?;
    let found = engine_for_extension("lua");
    assert!(found.is_some());
    assert_eq!(found.expect("lua engine").id, lua);
    assert!(engine_for_extension("unknown").is_none());

    // Test 3: resource limits.
    serial_println!("scriptlang::self_test 3: resource limits");
    set_limits(lua, 64 * 1024 * 1024, 30_000)?;
    let info = get_engine(lua)?;
    assert_eq!(info.max_memory, 64 * 1024 * 1024);
    assert_eq!(info.max_time_ms, 30_000);

    // Test 4: create/destroy contexts.
    serial_println!("scriptlang::self_test 4: contexts");
    let ctx1 = create_context(lua, "test.app")?;
    let ctx2 = create_context(wasm, "test.app")?;
    assert_eq!(list_contexts().len(), 2);
    // Cannot unregister engine with active contexts.
    assert!(unregister_engine(lua).is_err());
    destroy_context(ctx1)?;
    destroy_context(ctx2)?;
    assert!(list_contexts().is_empty());

    // Test 5: bindings.
    serial_println!("scriptlang::self_test 5: bindings");
    let ctx = create_context(lua, "test.app")?;
    add_binding(ctx, "print", "function", false)?;
    add_binding(ctx, "data", "table", true)?;
    let info = get_context(ctx)?;
    assert_eq!(info.bindings.len(), 2);
    // Duplicate fails.
    assert!(add_binding(ctx, "print", "function", false).is_err());
    remove_binding(ctx, "data")?;
    let info = get_context(ctx)?;
    assert_eq!(info.bindings.len(), 1);

    // Test 6: eval.
    serial_println!("scriptlang::self_test 6: eval");
    let result = eval(ctx, "print('hello')")?;
    assert!(result.success);
    let info = get_context(ctx)?;
    assert_eq!(info.eval_count, 1);
    destroy_context(ctx)?;

    // Test 7: enable/disable and cleanup.
    serial_println!("scriptlang::self_test 7: enable/disable");
    set_enabled(lua, false)?;
    let info = get_engine(lua)?;
    assert!(!info.enabled);
    // Cannot create context on disabled engine.
    assert!(create_context(lua, "test.app").is_err());
    set_enabled(lua, true)?;
    // Now cleanup works.
    unregister_engine(lua)?;
    unregister_engine(wasm)?;
    assert!(list_engines().is_empty());

    clear_all();
    serial_println!("scriptlang::self_test: all 7 tests passed");
    Ok(())
}
