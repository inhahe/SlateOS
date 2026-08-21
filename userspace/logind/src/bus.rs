//! The bus-facing surface of logind: what a *program* — as opposed to a
//! person at a terminal — can ask the session manager to do.
//!
//! # Why this module exists
//!
//! `design-decisions.md` §341 settled how a desktop application checks a
//! password: it does not. It hands the typed password to a privileged verifier
//! and gets back a verdict, because the stored hash lives in a root-only file
//! and a full-screen GUI process is the worst place on the system to keep a
//! copy of one. The verifier (`userspace/authlib`) and the gate
//! (`Daemon::authenticate_session` / `Daemon::unlock_session`) landed with that
//! decision. What did not land was the part in the middle: logind had no
//! resident event loop and no endpoint, so there was nothing for
//! `apps/lockscreen` to call, and the answer to lane C had to be filed as
//! "half-landed" (`requests/b-c-desktop-password-checks-go-through-a-privileged-verifier.md`).
//!
//! This module is that middle. It maps bus method calls onto the `Daemon` API
//! that already exists, and — the part that is not plumbing — decides *who is
//! allowed to make each call*.
//!
//! # Every method is authorised, and an unidentified caller gets nothing
//!
//! A session manager that answers any request that reaches it is not a session
//! manager; it is a way to unlock other people's screens. So every method here
//! goes through [`authorize`], which needs to know who is calling, and the
//! kernel is the only party that can say (`libservicebus::Credentials`).
//!
//! **The kernel cannot say yet.** `SYS_SERVICE_ACCEPT` hands back a bare
//! channel handle and records nothing about the process on the other end, so
//! `Connection::peer_credentials()` answers `None` for every connection, and
//! every method here consequently answers [`ERR_UNKNOWN_CALLER`]. That is the
//! correct behaviour and it is deliberate: the alternative — assume the caller
//! is the session's owner because usually it is — would make
//! `ForceUnlockSession` a password-free unlock for anything that can open a
//! channel, which is the exact hole §341 was written to close. The syscall is
//! requested in `requests/b-a-a-service-cannot-find-out-who-is-calling-it.md`;
//! when it lands, `peer_credentials` starts returning `Some`, and nothing in
//! this file changes.
//!
//! # The policy, in one paragraph
//!
//! Root may act on any session. Anyone else may act only on their own, and for
//! them a session belonging to someone else is reported as *not existing*
//! rather than as *forbidden* — a "permission denied" would confirm the
//! session is there, which is a fact about another user that the asker has no
//! business learning. `ForceUnlockSession` is root-only under all
//! circumstances: it is the administrator's override
//! (`loginctl unlock-session`, which systemd gates with polkit), and a screen
//! lock that could call it would not be a lock.

use crate::Daemon;
use libservicebus::{Credentials, Message, fields};

/// The well-known name logind registers on the service registry.
pub const SERVICE_NAME: &str = "system.logind";

// ---------------------------------------------------------------------------
// Error names
// ---------------------------------------------------------------------------

/// The member name is not a method of this interface.
pub const ERR_UNKNOWN_METHOD: &str = "system.logind.Error.UnknownMethod";
/// The payload did not decode, or had the wrong number of fields.
pub const ERR_INVALID_ARGUMENTS: &str = "system.logind.Error.InvalidArguments";
/// No such session — or one the caller is not entitled to know about.
pub const ERR_NO_SUCH_SESSION: &str = "system.logind.Error.NoSuchSession";
/// The caller is identified, but not permitted to do this.
pub const ERR_ACCESS_DENIED: &str = "system.logind.Error.AccessDenied";
/// The kernel could not tell us who the caller is, so nothing is permitted.
pub const ERR_UNKNOWN_CALLER: &str = "system.logind.Error.UnknownCaller";
/// `UnlockSession` without a preceding accepted `AuthenticateSession`.
pub const ERR_NOT_AUTHENTICATED: &str = "system.logind.Error.NotAuthenticated";

// ---------------------------------------------------------------------------
// Outcome wire codes
// ---------------------------------------------------------------------------

/// Wire code for [`authlib::Outcome::Accepted`].
pub const OUTCOME_ACCEPTED: u8 = 0;
/// Wire code for [`authlib::Outcome::Rejected`].
pub const OUTCOME_REJECTED: u8 = 1;
/// Wire code for [`authlib::Outcome::Locked`].
pub const OUTCOME_LOCKED: u8 = 2;
/// Wire code for [`authlib::Outcome::NoPassword`].
pub const OUTCOME_NO_PASSWORD: u8 = 3;
/// Wire code for [`authlib::Outcome::Unusable`].
pub const OUTCOME_UNUSABLE: u8 = 4;
/// Wire code for [`authlib::Outcome::RateLimited`].
pub const OUTCOME_RATE_LIMITED: u8 = 5;

/// Map a verdict onto its wire code and retry delay.
///
/// The codes are explicit constants rather than the enum's discriminants
/// because they are a *wire* contract with a process that is compiled
/// separately: renaming or reordering a variant must not silently turn
/// `Rejected` into `Accepted` in a client built last month.
const fn outcome_code(outcome: authlib::Outcome) -> (u8, u64) {
    match outcome {
        authlib::Outcome::Accepted => (OUTCOME_ACCEPTED, 0),
        authlib::Outcome::Rejected => (OUTCOME_REJECTED, 0),
        authlib::Outcome::Locked => (OUTCOME_LOCKED, 0),
        authlib::Outcome::NoPassword => (OUTCOME_NO_PASSWORD, 0),
        authlib::Outcome::Unusable => (OUTCOME_UNUSABLE, 0),
        authlib::Outcome::RateLimited { retry_after_secs } => {
            (OUTCOME_RATE_LIMITED, retry_after_secs)
        }
    }
}

// ---------------------------------------------------------------------------
// Replies
// ---------------------------------------------------------------------------

/// What dispatching one method call produced.
///
/// Kept separate from `libservicebus::Message` so the policy can be tested
/// without a transport: these are the decisions, and turning them into wire
/// messages is [`handle_message`]'s job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// Success, with a payload encoded by `libservicebus::fields`.
    Return(Vec<u8>),
    /// Failure, named by one of the `ERR_*` constants above.
    Error(&'static str),
}

impl Reply {
    /// A success carrying no data.
    fn empty() -> Self {
        Reply::Return(fields::encode(&[]))
    }

    /// Whether this is an error reply (convenience for tests and callers).
    #[must_use]
    pub const fn is_error(&self) -> bool {
        matches!(self, Reply::Error(_))
    }
}

// ---------------------------------------------------------------------------
// Authorisation
// ---------------------------------------------------------------------------

/// How much authority a method requires over the session it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Required {
    /// The session's own user, or root.
    Owner,
    /// Root, and only root, whoever owns the session.
    Administrator,
}

/// Decide whether `caller` may operate on `session_id` at `level`.
///
/// Returns the session id back on success purely so callers can chain; the
/// value is the caller's own argument.
fn authorize(
    daemon: &Daemon,
    session_id: &str,
    caller: Option<Credentials>,
    level: Required,
) -> Result<(), &'static str> {
    // An unidentified caller is not a caller with default privileges; it is a
    // caller we know nothing about. There is no safe default for that.
    let Some(caller) = caller else {
        return Err(ERR_UNKNOWN_CALLER);
    };

    if level == Required::Administrator {
        // Checked before the session lookup on purpose: a non-root caller must
        // not be able to probe which session ids exist by watching whether
        // ForceUnlockSession answers NoSuchSession or AccessDenied.
        return if caller.is_root() {
            Ok(())
        } else {
            Err(ERR_ACCESS_DENIED)
        };
    }

    match daemon.sessions.get(session_id) {
        Some(session) if caller.is_root() || session.uid == caller.uid => Ok(()),
        // Someone else's session is reported as absent rather than forbidden.
        // "Denied" would confirm that the session exists, which is a fact
        // about another user that this caller has no claim on.
        _ => Err(ERR_NO_SUCH_SESSION),
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Handle one method call.
///
/// `caller` is the kernel's report of the peer, or `None` when it cannot
/// report one — see the module docs for why `None` refuses everything.
///
/// # Panics
///
/// Does not panic: every argument is decoded through
/// `libservicebus::fields`, which returns `None` rather than indexing past the
/// end, and every session lookup is fallible.
pub fn dispatch(
    daemon: &mut Daemon,
    member: &str,
    payload: &[u8],
    caller: Option<Credentials>,
) -> Reply {
    match member {
        "ListSessions" => list_sessions(daemon, caller),
        "GetSession" => one_arg(payload, |id| get_session(daemon, id, caller)),
        "LockSession" => one_arg(payload, |id| lock_session(daemon, id, caller)),
        "UnlockSession" => one_arg(payload, |id| unlock_session(daemon, id, caller)),
        "ForceUnlockSession" => one_arg(payload, |id| force_unlock_session(daemon, id, caller)),
        "TerminateSession" => one_arg(payload, |id| terminate_session(daemon, id, caller)),
        "AuthenticateSession" => authenticate_session(daemon, payload, caller),
        "SetIdleHint" => set_idle_hint(daemon, payload, caller),
        _ => Reply::Error(ERR_UNKNOWN_METHOD),
    }
}

/// Decode a single text argument, or fail with [`ERR_INVALID_ARGUMENTS`].
///
/// The argument is required to be UTF-8 here — unlike a username or a path, a
/// session id is a value logind itself minted (`Daemon::allocate_session_id`
/// produces decimal digits), so a non-UTF-8 one cannot name a real session and
/// rejecting it early is more honest than looking it up and missing.
fn one_arg<F: FnOnce(&str) -> Reply>(payload: &[u8], f: F) -> Reply {
    let Some(args) = fields::decode_exact(payload, 1) else {
        return Reply::Error(ERR_INVALID_ARGUMENTS);
    };
    let Some(arg) = args.first().and_then(|b| core::str::from_utf8(b).ok()) else {
        return Reply::Error(ERR_INVALID_ARGUMENTS);
    };
    f(arg)
}

/// `ListSessions() -> [line, …]`
///
/// Root sees every session; anyone else sees their own. The filter is not a
/// convenience — an unprivileged process being able to enumerate who else is
/// logged in, on what tty, is exactly the reconnaissance step that precedes
/// picking a target.
fn list_sessions(daemon: &Daemon, caller: Option<Credentials>) -> Reply {
    let Some(caller) = caller else {
        return Reply::Error(ERR_UNKNOWN_CALLER);
    };

    // Sorted so the answer does not depend on HashMap iteration order; a
    // listing that reshuffles between identical calls is a listing nobody can
    // diff.
    let mut lines: Vec<String> = daemon
        .sessions
        .values()
        .filter(|s| caller.is_root() || s.uid == caller.uid)
        .map(crate::Session::format_list_line)
        .collect();
    lines.sort();

    let refs: Vec<&[u8]> = lines.iter().map(|l| l.as_bytes()).collect();
    Reply::Return(fields::encode(&refs))
}

/// `GetSession(id) -> properties`
fn get_session(daemon: &Daemon, id: &str, caller: Option<Credentials>) -> Reply {
    if let Err(e) = authorize(daemon, id, caller, Required::Owner) {
        return Reply::Error(e);
    }
    match daemon.sessions.get(id) {
        Some(session) => Reply::Return(fields::encode(&[session.format_properties().as_bytes()])),
        None => Reply::Error(ERR_NO_SUCH_SESSION),
    }
}

/// `LockSession(id)`
fn lock_session(daemon: &mut Daemon, id: &str, caller: Option<Credentials>) -> Reply {
    if let Err(e) = authorize(daemon, id, caller, Required::Owner) {
        return Reply::Error(e);
    }
    match daemon.lock_session(id) {
        Ok(()) => Reply::empty(),
        Err(_) => Reply::Error(ERR_NO_SUCH_SESSION),
    }
}

/// `UnlockSession(id)` — spends the ticket left by `AuthenticateSession`.
fn unlock_session(daemon: &mut Daemon, id: &str, caller: Option<Credentials>) -> Reply {
    if let Err(e) = authorize(daemon, id, caller, Required::Owner) {
        return Reply::Error(e);
    }
    // `authorize` has already established the session exists and is the
    // caller's, so the only remaining failure is the missing ticket. Mapping
    // it to its own error matters: a client that gets NoSuchSession will
    // retry the lookup, whereas one that gets NotAuthenticated will ask for
    // the password, which is what it should do.
    match daemon.unlock_session(id) {
        Ok(()) => Reply::empty(),
        Err(_) => Reply::Error(ERR_NOT_AUTHENTICATED),
    }
}

/// `ForceUnlockSession(id)` — the administrator's password-free override.
fn force_unlock_session(daemon: &mut Daemon, id: &str, caller: Option<Credentials>) -> Reply {
    if let Err(e) = authorize(daemon, id, caller, Required::Administrator) {
        return Reply::Error(e);
    }
    match daemon.force_unlock_session(id) {
        Ok(()) => Reply::empty(),
        Err(_) => Reply::Error(ERR_NO_SUCH_SESSION),
    }
}

/// `TerminateSession(id)`
fn terminate_session(daemon: &mut Daemon, id: &str, caller: Option<Credentials>) -> Reply {
    if let Err(e) = authorize(daemon, id, caller, Required::Owner) {
        return Reply::Error(e);
    }
    match daemon.terminate_session(id) {
        Ok(()) => Reply::empty(),
        Err(_) => Reply::Error(ERR_NO_SUCH_SESSION),
    }
}

/// `AuthenticateSession(id, password) -> [code, retry_after_secs, message]`
///
/// The reply deliberately carries the full verdict rather than a boolean. A
/// lock screen shows the same "wrong password" for `Rejected`, `Locked` and
/// `Unusable`, but `Unusable` means the stored entry is in a format nothing on
/// this system can recompute — the machine is broken, not the typist — and it
/// needs to reach an administrator instead of being counted as a typo.
fn authenticate_session(daemon: &mut Daemon, payload: &[u8], caller: Option<Credentials>) -> Reply {
    let Some(args) = fields::decode_exact(payload, 2) else {
        return Reply::Error(ERR_INVALID_ARGUMENTS);
    };
    let (Some(id), Some(password)) = (
        args.first().and_then(|b| core::str::from_utf8(b).ok()),
        args.get(1),
    ) else {
        return Reply::Error(ERR_INVALID_ARGUMENTS);
    };

    if let Err(e) = authorize(daemon, id, caller, Required::Owner) {
        return Reply::Error(e);
    }

    match daemon.authenticate_session(id, password) {
        Ok(outcome) => {
            let (code, retry) = outcome_code(outcome);
            Reply::Return(fields::encode(&[
                &[code][..],
                &retry.to_le_bytes()[..],
                outcome.user_message().as_bytes(),
            ]))
        }
        Err(_) => Reply::Error(ERR_NO_SUCH_SESSION),
    }
}

/// `SetIdleHint(id, "0"|"1", timestamp)`
fn set_idle_hint(daemon: &mut Daemon, payload: &[u8], caller: Option<Credentials>) -> Reply {
    let Some(args) = fields::decode_exact(payload, 3) else {
        return Reply::Error(ERR_INVALID_ARGUMENTS);
    };
    let (Some(id), Some(idle), Some(ts)) = (
        args.first().and_then(|b| core::str::from_utf8(b).ok()),
        args.get(1).and_then(|b| match *b {
            b"0" => Some(false),
            b"1" => Some(true),
            _ => None,
        }),
        args.get(2)
            .and_then(|b| core::str::from_utf8(b).ok())
            .and_then(|s| s.parse::<u64>().ok()),
    ) else {
        return Reply::Error(ERR_INVALID_ARGUMENTS);
    };

    if let Err(e) = authorize(daemon, id, caller, Required::Owner) {
        return Reply::Error(e);
    }
    match daemon.set_session_idle(id, idle, ts) {
        Ok(()) => Reply::empty(),
        Err(_) => Reply::Error(ERR_NO_SUCH_SESSION),
    }
}

// ---------------------------------------------------------------------------
// Message plumbing
// ---------------------------------------------------------------------------

/// Turn a received call into the message to send back.
///
/// Split from [`dispatch`] so that the policy is testable as values rather
/// than as wire bytes, and so that a future transport change touches one
/// function.
#[must_use]
pub fn handle_message(daemon: &mut Daemon, call: &Message, caller: Option<Credentials>) -> Message {
    match dispatch(daemon, &call.member, &call.payload, caller) {
        Reply::Return(payload) => Message::reply(call).with_payload(&payload),
        Reply::Error(name) => Message::error(call, name),
    }
}

/// Overwrite a call's payload once it has been handled.
///
/// `AuthenticateSession` carries a plaintext password, and the buffer it
/// arrived in is a `Vec` that will be reused or freed without being cleared.
/// Wiping it does not make the password safe — it was in a channel buffer and
/// a receive buffer before it got here — but it shortens the window in which a
/// later heap dump of this process contains someone's password, and it costs a
/// memset.
///
/// `write_volatile` in a loop rather than `fill`, because the optimiser is
/// entitled to delete a store to memory that is never read again, and that is
/// exactly what this is.
pub fn wipe(payload: &mut [u8]) {
    for byte in payload.iter_mut() {
        // SAFETY: `byte` is a valid, aligned, exclusively-borrowed `u8` for
        // the duration of the loop iteration; a volatile write of a `u8` to it
        // is in-bounds and initialises it.
        unsafe { core::ptr::write_volatile(byte, 0) };
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CreateSessionParams, DaemonConfig};

    /// A daemon with one session for uid 1000 ("alice") and one for uid 1001
    /// ("bob"), so that "may I touch someone else's session?" is answerable.
    fn two_user_daemon() -> (Daemon, String, String) {
        let mut d = Daemon::new(DaemonConfig::default());
        let alice = d
            .create_session(CreateSessionParams {
                uid: 1000,
                user: "alice",
                seat_id: "seat0",
                ..Default::default()
            })
            .expect("alice session");
        let bob = d
            .create_session(CreateSessionParams {
                uid: 1001,
                user: "bob",
                seat_id: "seat0",
                ..Default::default()
            })
            .expect("bob session");
        (d, alice, bob)
    }

    const fn creds(uid: u32) -> Credentials {
        Credentials {
            pid: 42,
            uid,
            gid: uid,
        }
    }

    fn call(d: &mut Daemon, member: &str, args: &[&[u8]], who: Option<Credentials>) -> Reply {
        dispatch(d, member, &fields::encode(args), who)
    }

    // -- the unidentified caller ------------------------------------------

    #[test]
    fn a_caller_the_kernel_cannot_identify_gets_nothing() {
        let (mut d, alice, _) = two_user_daemon();
        // Every method, including the read-only ones: an anonymous listing is
        // still a listing of who is logged in.
        for (member, args) in [
            ("ListSessions", vec![]),
            ("GetSession", vec![alice.as_bytes()]),
            ("LockSession", vec![alice.as_bytes()]),
            ("UnlockSession", vec![alice.as_bytes()]),
            ("ForceUnlockSession", vec![alice.as_bytes()]),
            ("TerminateSession", vec![alice.as_bytes()]),
        ] {
            assert_eq!(
                call(&mut d, member, &args, None),
                Reply::Error(ERR_UNKNOWN_CALLER),
                "{member} answered an unidentified caller"
            );
        }
    }

    #[test]
    fn authenticate_from_an_unidentified_caller_is_refused_before_the_password_is_checked() {
        let (mut d, alice, _) = two_user_daemon();
        let before = d.auth.failures("alice");
        let reply = call(
            &mut d,
            "AuthenticateSession",
            &[alice.as_bytes(), b"guess"],
            None,
        );
        assert_eq!(reply, Reply::Error(ERR_UNKNOWN_CALLER));
        // The refusal must happen *before* the verifier runs, or an anonymous
        // caller could lock a real user out by burning their attempt budget.
        assert_eq!(d.auth.failures("alice"), before, "budget was consumed");
    }

    // -- cross-user access -------------------------------------------------

    #[test]
    fn another_users_session_does_not_exist_as_far_as_you_are_concerned() {
        let (mut d, _, bob) = two_user_daemon();
        // Not AccessDenied: that answer would confirm bob is logged in.
        assert_eq!(
            call(&mut d, "GetSession", &[bob.as_bytes()], Some(creds(1000))),
            Reply::Error(ERR_NO_SUCH_SESSION)
        );
        assert_eq!(
            call(&mut d, "LockSession", &[bob.as_bytes()], Some(creds(1000))),
            Reply::Error(ERR_NO_SUCH_SESSION)
        );
        // ... and it is the same answer a genuinely absent session gets, so
        // the two are indistinguishable from outside.
        assert_eq!(
            call(&mut d, "GetSession", &[b"999999"], Some(creds(1000))),
            Reply::Error(ERR_NO_SUCH_SESSION)
        );
    }

    #[test]
    fn a_user_may_work_on_their_own_session() {
        let (mut d, alice, _) = two_user_daemon();
        assert!(!call(&mut d, "GetSession", &[alice.as_bytes()], Some(creds(1000))).is_error());
        assert!(
            !call(
                &mut d,
                "LockSession",
                &[alice.as_bytes()],
                Some(creds(1000))
            )
            .is_error()
        );
        assert!(d.sessions[&alice].locked);
    }

    #[test]
    fn root_may_work_on_anyones_session() {
        let (mut d, _, bob) = two_user_daemon();
        assert!(!call(&mut d, "LockSession", &[bob.as_bytes()], Some(creds(0))).is_error());
        assert!(d.sessions[&bob].locked);
    }

    // -- the administrator's override --------------------------------------

    #[test]
    fn force_unlock_is_refused_to_the_sessions_own_owner() {
        let (mut d, alice, _) = two_user_daemon();
        d.lock_session(&alice).expect("lock");
        // Owning the session is not enough — if it were, a lock screen could
        // clear itself and the password check would be decorative again.
        assert_eq!(
            call(
                &mut d,
                "ForceUnlockSession",
                &[alice.as_bytes()],
                Some(creds(1000))
            ),
            Reply::Error(ERR_ACCESS_DENIED)
        );
        assert!(d.sessions[&alice].locked, "screen was cleared anyway");
    }

    #[test]
    fn force_unlock_tells_a_non_root_caller_nothing_about_which_sessions_exist() {
        let (mut d, _, _) = two_user_daemon();
        // Real session and imaginary session must give the same answer, or the
        // error code becomes a session-id oracle.
        let real = call(&mut d, "ForceUnlockSession", &[b"1"], Some(creds(1000)));
        let imaginary = call(
            &mut d,
            "ForceUnlockSession",
            &[b"999999"],
            Some(creds(1000)),
        );
        assert_eq!(real, Reply::Error(ERR_ACCESS_DENIED));
        assert_eq!(imaginary, real);
    }

    #[test]
    fn root_may_force_unlock() {
        let (mut d, alice, _) = two_user_daemon();
        d.lock_session(&alice).expect("lock");
        assert!(
            !call(
                &mut d,
                "ForceUnlockSession",
                &[alice.as_bytes()],
                Some(creds(0))
            )
            .is_error()
        );
        assert!(!d.sessions[&alice].locked);
    }

    // -- unlock needs a ticket ---------------------------------------------

    #[test]
    fn unlocking_without_authenticating_says_so_specifically() {
        let (mut d, alice, _) = two_user_daemon();
        d.lock_session(&alice).expect("lock");
        // Not NoSuchSession: the client's correct reaction is to ask for a
        // password, not to re-look-up the session.
        assert_eq!(
            call(
                &mut d,
                "UnlockSession",
                &[alice.as_bytes()],
                Some(creds(1000))
            ),
            Reply::Error(ERR_NOT_AUTHENTICATED)
        );
        assert!(d.sessions[&alice].locked);
    }

    // -- argument handling --------------------------------------------------

    #[test]
    fn a_malformed_payload_is_rejected_rather_than_read_past() {
        let (mut d, alice, _) = two_user_daemon();
        // Wrong arity in both directions, and a payload that is not a field
        // list at all.
        assert_eq!(
            dispatch(&mut d, "GetSession", &fields::encode(&[]), Some(creds(0))),
            Reply::Error(ERR_INVALID_ARGUMENTS)
        );
        assert_eq!(
            dispatch(
                &mut d,
                "GetSession",
                &fields::encode(&[alice.as_bytes(), b"extra"]),
                Some(creds(0))
            ),
            Reply::Error(ERR_INVALID_ARGUMENTS)
        );
        assert_eq!(
            dispatch(&mut d, "GetSession", b"\xff\xff", Some(creds(0))),
            Reply::Error(ERR_INVALID_ARGUMENTS)
        );
    }

    #[test]
    fn an_unknown_method_is_not_silently_successful() {
        let (mut d, _, _) = two_user_daemon();
        assert_eq!(
            call(&mut d, "UnlockEverything", &[], Some(creds(0))),
            Reply::Error(ERR_UNKNOWN_METHOD)
        );
    }

    #[test]
    fn set_idle_hint_takes_a_boolean_and_a_timestamp() {
        let (mut d, alice, _) = two_user_daemon();
        assert!(
            !call(
                &mut d,
                "SetIdleHint",
                &[alice.as_bytes(), b"1", b"12345"],
                Some(creds(1000))
            )
            .is_error()
        );
        assert!(d.sessions[&alice].idle);
        assert_eq!(d.sessions[&alice].idle_since, 12345);

        // "true"/"yes" are not accepted: one spelling, checked, beats three
        // spellings and a guess.
        assert_eq!(
            call(
                &mut d,
                "SetIdleHint",
                &[alice.as_bytes(), b"true", b"1"],
                Some(creds(1000))
            ),
            Reply::Error(ERR_INVALID_ARGUMENTS)
        );
    }

    // -- listing ------------------------------------------------------------

    #[test]
    fn a_listing_shows_you_only_yourself_unless_you_are_root() {
        let (mut d, _, _) = two_user_daemon();

        let Reply::Return(payload) = call(&mut d, "ListSessions", &[], Some(creds(1000))) else {
            panic!("listing failed");
        };
        let lines = fields::decode(&payload).expect("decode");
        assert_eq!(lines.len(), 1);
        assert!(String::from_utf8_lossy(lines[0]).contains("alice"));

        let Reply::Return(payload) = call(&mut d, "ListSessions", &[], Some(creds(0))) else {
            panic!("root listing failed");
        };
        assert_eq!(fields::decode(&payload).expect("decode").len(), 2);
    }

    // -- the verdict on the wire --------------------------------------------

    #[test]
    fn an_authentication_verdict_survives_the_wire_with_its_detail_intact() {
        // No stores exist under this daemon's default paths in the test
        // environment, so the verdict is Rejected — which is the point: the
        // encoding must carry *which* refusal it was, not just "no".
        let (mut d, alice, _) = two_user_daemon();
        let Reply::Return(payload) = call(
            &mut d,
            "AuthenticateSession",
            &[alice.as_bytes(), b"whatever"],
            Some(creds(1000)),
        ) else {
            panic!("authenticate failed");
        };

        let parts = fields::decode_exact(&payload, 3).expect("three fields");
        assert_eq!(parts[0].len(), 1, "code is one byte");
        assert_ne!(parts[0][0], OUTCOME_ACCEPTED, "accepted with no store!");
        assert_eq!(parts[1].len(), 8, "retry delay is a u64");
        assert!(!parts[2].is_empty(), "a message the UI can show");
    }

    #[test]
    fn a_password_is_bytes_and_may_not_be_utf8() {
        // Passwords are whatever was typed. A codec that insisted on UTF-8
        // here would reject a legitimate password rather than fail to match
        // it, which is a much more confusing bug.
        let (mut d, alice, _) = two_user_daemon();
        let reply = call(
            &mut d,
            "AuthenticateSession",
            &[alice.as_bytes(), &[0xff, 0xfe, 0x80]],
            Some(creds(1000)),
        );
        assert!(
            !reply.is_error(),
            "non-UTF-8 password was refused as malformed"
        );
    }

    // -- message plumbing ----------------------------------------------------

    #[test]
    fn handle_message_preserves_the_reply_serial_so_a_client_can_match_it() {
        let (mut d, alice, _) = two_user_daemon();
        let mut call_msg =
            Message::method_call("GetSession").with_payload(&fields::encode(&[alice.as_bytes()]));
        call_msg.serial = 77;

        let ok = handle_message(&mut d, &call_msg, Some(creds(1000)));
        assert!(ok.is_reply() && !ok.is_error());
        assert_eq!(ok.reply_serial, 77);

        let denied = handle_message(&mut d, &call_msg, None);
        assert!(denied.is_error());
        assert_eq!(denied.reply_serial, 77);
        assert_eq!(denied.member, ERR_UNKNOWN_CALLER);
    }

    #[test]
    fn wiping_a_payload_leaves_no_password_behind() {
        let mut payload = fields::encode(&[b"1".as_slice(), b"correct horse".as_slice()]);
        assert!(payload.windows(7).any(|w| w == b"correct"));
        wipe(&mut payload);
        assert!(payload.iter().all(|&b| b == 0));
    }
}
