//! UTS namespaces: the host name and NIS domain name a process sees and sets
//! -- Linux's `uts_namespace` (`man 7 uts_namespaces`).
//!
//! Every process is in one (`pcb::Process::uts_ns`), inherited by `fork` and
//! spawn. The **root** namespace's names are the system's
//! (`fs::nameservice`): reading and setting them there is what `uname` and
//! `sethostname` always did, and what the system's own readers (`sysfs`, the
//! kernel shell, the network settings) go on reading. Any other namespace
//! keeps its own copy, made from its creator's when `unshare(CLONE_NEWUTS)`
//! or `clone(CLONE_NEWUTS)` made it, or given by the container layer
//! (`--hostname`). Setting a name in one changes nothing outside it.
//!
//! A namespace lives while a process is in it or a handle holds it
//! (`/proc/<pid>/ns/uts` opened, for `setns`): [`retain`] and [`release`]
//! count both. The root is never counted and never goes.
//!
//! Names are UTF-8 and at most [`crate::uname::NODENAME_MAX`] bytes, the
//! rules `sethostname` and `setdomainname` already applied to the system's.

use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use crate::sync::Mutex;
use alloc::collections::BTreeMap;
use alloc::string::String;
use core::sync::atomic::{AtomicU64, Ordering};

/// A UTS namespace's id. [`ROOT_UTS`] is the system's.
pub type UtsNsId = u64;

/// The root UTS namespace: the system's names.
pub const ROOT_UTS: UtsNsId = 0;

/// The most namespaces besides the root that may exist at once -- a bound on
/// what an unprivileged loop of `unshare` could pin, as Linux's
/// `/proc/sys/user/max_uts_namespaces`. `ResourceExhausted` (`ENOSPC`)
/// beyond it.
const MAX_NAMESPACES: usize = 4096;

/// One namespace's names and the processes and handles that hold it.
struct UtsNs {
    hostname: String,
    domainname: String,
    refs: u32,
}

/// Every namespace but the root, by id.
static TABLE: Mutex<BTreeMap<UtsNsId, UtsNs>> = Mutex::new(BTreeMap::new());

/// The next id to give.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// `id`'s names: the system's for the root.
fn names_of(table: &BTreeMap<UtsNsId, UtsNs>, id: UtsNsId) -> Option<(String, String)> {
    if id == ROOT_UTS {
        crate::fs::nameservice::init_defaults();
        return Some((
            crate::fs::nameservice::get_hostname(),
            crate::fs::nameservice::get_domain(),
        ));
    }
    table
        .get(&id)
        .map(|ns| (ns.hostname.clone(), ns.domainname.clone()))
}

/// A new namespace with a copy of `from`'s names, held once by the caller
/// (who passes that hold to the process it puts there).
///
/// # Errors
///
/// `NotFound` for a `from` that does not exist; `ResourceExhausted` past
/// [`MAX_NAMESPACES`].
pub fn create_from(from: UtsNsId) -> KernelResult<UtsNsId> {
    let mut table = TABLE.lock();
    if table.len() >= MAX_NAMESPACES {
        return Err(KernelError::ResourceExhausted);
    }
    let (hostname, domainname) = names_of(&table, from).ok_or(KernelError::NotFound)?;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    table.insert(
        id,
        UtsNs {
            hostname,
            domainname,
            refs: 1,
        },
    );
    Ok(id)
}

/// One more hold on `id` -- a process put there, a handle opened. `false` if
/// it does not exist (the root always does, and is not counted).
pub fn retain(id: UtsNsId) -> bool {
    if id == ROOT_UTS {
        return true;
    }
    match TABLE.lock().get_mut(&id) {
        Some(ns) => {
            ns.refs = ns.refs.saturating_add(1);
            true
        }
        None => false,
    }
}

/// One hold on `id` given up; the namespace goes with its last.
pub fn release(id: UtsNsId) {
    if id == ROOT_UTS {
        return;
    }
    let mut table = TABLE.lock();
    let gone = match table.get_mut(&id) {
        Some(ns) => {
            ns.refs = ns.refs.saturating_sub(1);
            ns.refs == 0
        }
        None => false,
    };
    if gone {
        table.remove(&id);
    }
}

/// `id`'s host name: the system's for the root, `None` for a namespace that
/// does not exist.
#[must_use]
pub fn hostname(id: UtsNsId) -> Option<String> {
    names_of(&TABLE.lock(), id).map(|(h, _)| h)
}

/// `id`'s NIS domain name, as [`hostname`].
#[must_use]
pub fn domainname(id: UtsNsId) -> Option<String> {
    names_of(&TABLE.lock(), id).map(|(_, d)| d)
}

/// The rule every setter applies: UTF-8 (the caller checked) and at most
/// [`crate::uname::NODENAME_MAX`] bytes.
fn check_name(name: &str) -> KernelResult<()> {
    if name.len() > crate::uname::NODENAME_MAX {
        Err(KernelError::InvalidArgument)
    } else {
        Ok(())
    }
}

/// Set `id`'s host name: the system's for the root.
///
/// # Errors
///
/// `InvalidArgument` past the length; `NotFound` for a namespace that does
/// not exist; the system's own refusal for the root.
pub fn set_hostname(id: UtsNsId, name: &str) -> KernelResult<()> {
    check_name(name)?;
    if id == ROOT_UTS {
        crate::fs::nameservice::init_defaults();
        return crate::fs::nameservice::set_hostname(name);
    }
    let mut table = TABLE.lock();
    let ns = table.get_mut(&id).ok_or(KernelError::NotFound)?;
    ns.hostname = String::from(name);
    Ok(())
}

/// Set `id`'s NIS domain name, as [`set_hostname`].
///
/// # Errors
///
/// As [`set_hostname`].
pub fn set_domainname(id: UtsNsId, name: &str) -> KernelResult<()> {
    check_name(name)?;
    if id == ROOT_UTS {
        crate::fs::nameservice::init_defaults();
        return crate::fs::nameservice::set_domain(name);
    }
    let mut table = TABLE.lock();
    let ns = table.get_mut(&id).ok_or(KernelError::NotFound)?;
    ns.domainname = String::from(name);
    Ok(())
}

/// The UTS namespace of the calling process: the root for a kernel task, or
/// a process going away.
#[must_use]
pub fn of_current() -> UtsNsId {
    crate::proc::thread::owner_process(crate::sched::current_task_id())
        .filter(|&pid| pid != 0)
        .and_then(crate::proc::pcb::uts_ns)
        .unwrap_or(ROOT_UTS)
}

/// Set the calling process's namespace's host name ([`set_hostname`] on
/// [`of_current`]): the shape the native setter takes.
///
/// # Errors
///
/// As [`set_hostname`].
pub fn set_hostname_here(name: &str) -> KernelResult<()> {
    set_hostname(of_current(), name)
}

/// [`set_hostname_here`] for the NIS domain name.
///
/// # Errors
///
/// As [`set_domainname`].
pub fn set_domainname_here(name: &str) -> KernelResult<()> {
    set_domainname(of_current(), name)
}

/// Whether `id` exists.
#[must_use]
pub fn exists(id: UtsNsId) -> bool {
    id == ROOT_UTS || TABLE.lock().contains_key(&id)
}

/// How many namespaces besides the root exist.
#[must_use]
pub fn count() -> usize {
    TABLE.lock().len()
}

/// A namespace made from another has its names and changes only its own; it
/// goes with its last hold; the root's names are the system's.
///
/// # Errors
///
/// `InternalError` on the first check that fails.
pub fn self_test() -> KernelResult<()> {
    let fail = |what: &str| {
        serial_println!("[utsns]   FAIL: {}", what);
        Err(KernelError::InternalError)
    };
    serial_println!("[utsns] Running self-test...");
    let before = count();
    let a = create_from(ROOT_UTS)?;
    if hostname(a) != hostname(ROOT_UTS) || domainname(a) != domainname(ROOT_UTS) {
        release(a);
        return fail("a new namespace did not start with its creator's names");
    }
    let system = hostname(ROOT_UTS);
    if set_hostname(a, "inside").is_err() || set_domainname(a, "inner.test").is_err() {
        release(a);
        return fail("a namespace's names could not be set");
    }
    let b = match create_from(a) {
        Ok(b) => b,
        Err(e) => {
            release(a);
            return Err(e);
        }
    };
    let copied =
        hostname(b).as_deref() == Some("inside") && domainname(b).as_deref() == Some("inner.test");
    let _ = set_hostname(b, "deeper");
    let kept = hostname(a).as_deref() == Some("inside") && hostname(ROOT_UTS) == system;
    let too_long = set_hostname(a, &"x".repeat(crate::uname::NODENAME_MAX + 1)).is_err();
    // Two holds on `a` now: its creator's and one more; it outlives the first.
    let held = retain(a);
    release(a);
    let survives = exists(a);
    release(a);
    release(b);
    let gone = !exists(a) && !exists(b) && count() == before;
    if !copied {
        return fail("a namespace made from another did not copy its names");
    }
    if !kept {
        return fail("setting a name in one namespace changed another's, or the system's");
    }
    if !too_long {
        return fail("a name past the length was taken");
    }
    if !(held && survives && gone) {
        return fail("a namespace did not live exactly as long as its holds");
    }
    serial_println!("[utsns]   names copied, set apart, and freed with the last hold: OK");
    Ok(())
}
