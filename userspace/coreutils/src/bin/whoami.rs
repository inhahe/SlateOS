//! whoami — print effective user name.
//!
//! Usage: whoami
//!   Prints the name of the current user.
//!   Falls back to the numeric UID if no name database is available.
//!
//! Portability: on POSIX-y targets (Linux, and our `x86_64-slateos`
//! custom target which reports `target_os = "linux"`) we look up the
//! effective UID via the `geteuid()` extern from the C runtime.  On
//! Windows hosts the symbol does not exist in mingw-w64; we use the
//! `USERNAME` env var the Win32 environment sets, falling back to the
//! numeric UID `0` if even that is absent.  The Windows host path is
//! purely so the coreutils crate builds and runs under `cargo test` on
//! the developer machine — production targets always have a real
//! `geteuid`.

use std::env;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn geteuid() -> u32;
}

/// Look up the current user name using the standard environment
/// variables (USER then LOGNAME on POSIX, USERNAME on Windows).
///
/// Returns `None` if neither is set.  Pulled into a helper so tests
/// can exercise it without spawning a subprocess.
fn user_from_env() -> Option<String> {
    if let Ok(name) = env::var("USER")
        && !name.is_empty()
    {
        return Some(name);
    }
    if let Ok(name) = env::var("LOGNAME")
        && !name.is_empty()
    {
        return Some(name);
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(name) = env::var("USERNAME")
            && !name.is_empty()
        {
            return Some(name);
        }
    }
    None
}

/// Numeric UID fallback when no name is available.
///
/// On POSIX targets this issues `geteuid()`.  On Windows hosts we
/// return 0 — there is no meaningful POSIX UID, and 0 is what
/// `whoami --uid`-style flags produce when the user database is
/// unavailable on other systems too.
fn current_uid() -> u32 {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: geteuid is a simple POSIX getter, no pointer arguments.
        unsafe { geteuid() }
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

fn main() {
    if let Some(name) = user_from_env() {
        println!("{name}");
        return;
    }
    println!("{}", current_uid());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // env::set_var / env::remove_var mutate process-global state.
    // Cargo runs unit tests in parallel by default, so serialise.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// RAII helper: lock the env mutex and remember the current values
    /// of the variables we'll be poking, restoring them on drop so a
    /// failing assertion doesn't leak state into subsequent tests.
    struct EnvScope {
        _guard: std::sync::MutexGuard<'static, ()>,
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl EnvScope {
        fn new(keys: &[&'static str]) -> Self {
            let guard = ENV_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let saved = keys
                .iter()
                .map(|&k| (k, env::var(k).ok()))
                .collect::<Vec<_>>();
            for (k, _) in &saved {
                // SAFETY: env::remove_var is safe on Windows + Linux for
                // these process-local mutations; the lock above prevents
                // races against parallel tests.
                unsafe {
                    env::remove_var(k);
                }
            }
            Self {
                _guard: guard,
                saved,
            }
        }

        fn set(&self, key: &str, value: &str) {
            // SAFETY: see new(); lock is held for the lifetime of self.
            unsafe {
                env::set_var(key, value);
            }
        }
    }

    impl Drop for EnvScope {
        fn drop(&mut self) {
            for (k, prev) in &self.saved {
                // SAFETY: see new().
                unsafe {
                    match prev {
                        Some(v) => env::set_var(k, v),
                        None => env::remove_var(k),
                    }
                }
            }
        }
    }

    #[test]
    fn user_from_env_prefers_user_over_logname() {
        let scope = EnvScope::new(&["USER", "LOGNAME", "USERNAME"]);
        scope.set("USER", "alice");
        scope.set("LOGNAME", "bob");
        assert_eq!(user_from_env().as_deref(), Some("alice"));
    }

    #[test]
    fn user_from_env_falls_back_to_logname() {
        let scope = EnvScope::new(&["USER", "LOGNAME", "USERNAME"]);
        scope.set("LOGNAME", "carol");
        assert_eq!(user_from_env().as_deref(), Some("carol"));
    }

    #[test]
    fn user_from_env_returns_none_when_unset() {
        let _scope = EnvScope::new(&["USER", "LOGNAME", "USERNAME"]);
        assert_eq!(user_from_env(), None);
    }

    #[test]
    fn user_from_env_treats_empty_as_unset() {
        let scope = EnvScope::new(&["USER", "LOGNAME", "USERNAME"]);
        // Empty USER must not shadow LOGNAME — POSIX shells sometimes
        // export USER="" when the user database is unavailable.
        scope.set("USER", "");
        scope.set("LOGNAME", "dave");
        assert_eq!(user_from_env().as_deref(), Some("dave"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn user_from_env_uses_username_on_windows() {
        let scope = EnvScope::new(&["USER", "LOGNAME", "USERNAME"]);
        scope.set("USERNAME", "eve");
        assert_eq!(user_from_env().as_deref(), Some("eve"));
    }

    #[test]
    fn current_uid_returns_zero_on_non_posix_hosts() {
        // On Windows hosts (the common dev environment) current_uid()
        // is the fallback path returning 0.  On Linux/slateos the cfg
        // gate compiles the geteuid() path, which we cannot easily
        // assert a value for — so this assertion is only run on the
        // platforms where the fallback compiles.
        #[cfg(not(target_os = "linux"))]
        {
            assert_eq!(current_uid(), 0);
        }
    }
}
