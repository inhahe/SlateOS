//! The real SSH client, run against the real SSH server, in one process.
//!
//! # Why this crate exists at all
//!
//! `userspace/ssh` and `userspace/sshd` are the two ends of one protocol. Every
//! function in the wire layer is therefore a *contract between two programs* —
//! and until this crate existed, nothing anywhere compared the two sides of any
//! of those contracts. Each crate tested its own copy against its own
//! expectations and passed.
//!
//! That is not a theoretical gap. Eleven duplications of the wire layer have
//! been found in this stack, every one of them by someone reading two files
//! side by side, none of them by a test. Six were live divergences, including:
//!
//! - the server hashed a *placeholder* client version string into the RFC 4253
//!   §8 exchange hash, so a correct client could not verify any host key
//!   signature it received — and the server's own tests, which recomputed the
//!   hash the same wrong way, agreed with it perfectly;
//! - the server's KEXINIT cookie was `sha256(b"sshd-kex-cookie")`, one constant
//!   shared by every copy of the binary, which hands an observer half of the
//!   input to the exchange hash;
//! - the client's Diffie-Hellman exponent was two hard-coded constants hashed
//!   together.
//!
//! Both suites were green through all of it, because a suite that only ever
//! talks to itself cannot notice that it is wrong in a way it is *consistently*
//! wrong. See known-issues.md
//! `TD-B-THE-SSH-WIRE-LAYER-IS-WRITTEN-TWICE-AND-NOTHING-MAKES-THE-TWO-COPIES-AGREE`.
//!
//! # Why a third crate, rather than a test in either of them
//!
//! A crate's own tests can only reach one side of a two-party protocol. To have
//! both ends in one process, something has to depend on both — and a crate
//! cannot depend on itself. So the test lives here, and both peers arrive as
//! ordinary dependencies.
//!
//! That in turn is why `ssh` and `sshd` each became a library with a three-line
//! binary on top: a binary crate produces no rlib, so while the client and the
//! daemon were each a `main.rs`, this crate could not link either of them.
//!
//! # What makes it runnable at all
//!
//! Two seams in `sshwire`, both of which exist because of this test:
//!
//! - [`sshwire::Transport`] — both binaries were written against a raw kernel
//!   socket handle, so neither could be exercised without a kernel.
//!   [`sshwire::memory_pair`] supplies a connected pair of in-memory endpoints
//!   instead.
//! - [`sshwire::SecretSource`] — both were written against
//!   `randrange::fill_secret` directly, so neither could be exercised without
//!   kernel *randomness*, which the Windows host this suite runs on refuses to
//!   provide on purpose. A handshake with no Diffie-Hellman exponent is not a
//!   handshake, so without this seam the test could not exist on this machine.
//!
//! The secret seam is reached through a Cargo feature (`deterministic-secrets`)
//! that only this crate's `dev-dependencies` turn on, so a shipped binary does
//! not contain the substitution point at all. It is emphatically not a way for
//! a *caller* to pick weak randomness: neither program exposes it to a command
//! line, a config file, or the network.
//!
//! # Why two threads
//!
//! The handshake is synchronous and interlocked — each side blocks reading what
//! the other has not sent yet — so one thread driving both in turn would
//! deadlock at the first message. Each peer therefore runs on its own thread.
//!
//! The peers are *constructed inside* their own threads rather than built here
//! and moved: `Box<dyn Transport>` carries no `Send` bound, so neither
//! `SshSession` nor `ConnectionState` may cross a thread boundary. What crosses
//! is the [`sshwire::MemoryTransport`], which is `Send` because it is a pair of
//! `Arc`s over a `Mutex`/`Condvar` pipe.

// The panicking lints are allowed here and nowhere else, on the same terms the
// two crates under test allow them in their own suites: a test that panics is a
// test reporting a failure in the loudest way available, which is what it is
// for. `format_collect` joins them because the only string this file builds is
// a hex dump printed inside an assertion message.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::format_collect
)]
mod tests {
    use std::cell::Cell;
    use std::path::Path;
    use std::thread;

    use sshwire::SecretSource;

    /// The session identifier both ends derive from the fixed inputs below.
    ///
    /// Recorded from a run in which the two ends agreed — it is an observation,
    /// not a value derived from the RFC by hand. Its job is to notice a change
    /// that both ends make *together*, which agreement cannot see. See
    /// [`the_handshake_transcript_is_the_one_recorded_here`].
    const RECORDED_SESSION_ID: &str =
        "684db82f1f61c0cb107eb3c0e015101c8cf2e57c631612f173247fad99f8795c";

    /// A reproducible stand-in for the kernel CSPRNG.
    ///
    /// Counts up, which is all this test needs: the two peers must each get
    /// bytes that are the same on every run, so that "both ends derived the
    /// same session id" can be strengthened to "both ends derived *exactly*
    /// this session id" — an assertion that fails when either end drifts, not
    /// only when the two drift apart from each other. It is deliberately not
    /// random: a test whose inputs differ per run cannot distinguish "the
    /// handshake is wrong" from "this run was unlucky".
    ///
    /// The counter is **thread-local**, and that is the whole of what makes the
    /// above true. A shared counter would be drawn from by the client thread
    /// and the server thread concurrently, so which bytes each one received
    /// would depend on how the two happened to interleave — a source that is
    /// deterministic in the sense that it uses no entropy, and nondeterministic
    /// in the only sense that matters. One counter per thread gives each peer
    /// its own fixed sequence, in the order its own protocol flow draws.
    ///
    /// # Errors
    ///
    /// Never — but the signature is [`sshwire::SecretSource`]'s, not this
    /// function's to choose. A substitute source has to be able to express the
    /// failure the kernel one can, since "the CSPRNG refused" is a case both
    /// peers must handle.
    #[expect(
        clippy::unnecessary_wraps,
        reason = "the Result is the SecretSource signature, not this function's choice"
    )]
    // `missing_const_for_thread_local` asks for the `const { ... }` initializer
    // that is already written below: clippy 1.95 does not recognise the form the
    // macro emits and fires anyway. Confirmed by experiment here — the warning
    // survives stripping the doc comment out of the macro body and survives
    // spelling the macro `std::thread_local!` — and independently in
    // `gui/toolkit/src/signal.rs`, which carries the same suppression for the
    // same lint on the same rustc. The `#[allow]` goes on the function because
    // an attribute on a macro *invocation* is ignored, and this is the smallest
    // item that encloses it.
    #[allow(
        clippy::missing_const_for_thread_local,
        reason = "clippy 1.95 false positive: the initializer already is a const block"
    )]
    fn counting_secrets(buf: &mut [u8]) -> Result<(), randrange::EntropyError> {
        // Declared inside the only function that draws from it, so that "one
        // counter per thread" cannot quietly become "one counter shared by the
        // client thread and the server thread" through a second caller.
        thread_local! {
            static NEXT: Cell<u8> = const { Cell::new(1) };
        }

        NEXT.with(|next| {
            for byte in buf.iter_mut() {
                *byte = next.get();
                next.set(next.get().wrapping_add(1));
            }
        });
        Ok(())
    }

    /// The seed the test server's host key is built from.
    ///
    /// A fixed seed rather than a generated one so the host key — and therefore
    /// the signature over the exchange hash, and therefore the `known_hosts`
    /// entry the client writes — is the same on every run.
    const HOST_KEY_SEED: [u8; 32] = [
        0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c,
        0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae,
        0x7f, 0x60,
    ];

    /// Run one complete handshake and return both ends' session identifiers.
    ///
    /// Both peers run on spawned threads, and neither on the caller's. That is
    /// not symmetry for its own sake: [`counting_secrets`] keeps its counter in
    /// thread-local storage, so a peer driven on the test harness's own thread
    /// would inherit whatever that thread had already drawn. Giving each peer a
    /// fresh thread makes each one's byte sequence start from the same place on
    /// every run, which is what lets a *recorded* session id mean anything.
    ///
    /// The handshake is interlocked -- each side blocks reading what the other
    /// has not sent yet -- so driving both from one thread would deadlock on the
    /// first message regardless.
    ///
    /// The peers are also *constructed inside* their threads rather than built
    /// here and moved in. `Box<dyn Transport>` carries no `Send` bound, so
    /// neither `SshSession` nor `ConnectionState` may cross a thread boundary;
    /// what crosses is the [`sshwire::MemoryTransport`], which is `Send`.
    fn one_handshake(known_hosts: &Path) -> ([u8; 32], [u8; 32]) {
        let (client_side, server_side) = sshwire::memory_pair();

        let server = thread::spawn(move || {
            let secrets: SecretSource = counting_secrets;
            let mut conn = sshd::ConnectionState::new(
                Box::new(server_side),
                sshd::SshdConfig::default_config(),
                sshd::HostKey::from_seed(HOST_KEY_SEED),
                false,
            )
            .with_secret_source(secrets);

            sshd::do_version_exchange(&mut conn)
                .and_then(|()| sshd::do_key_exchange(&mut conn))
                .map(|()| conn.session_id())
                .map_err(|e| e.to_string())
        });

        // The client's configuration comes from the real argument parser rather
        // than a hand-built `Config`, so the defaults under test are the
        // defaults that ship. `StrictHostKeyChecking=no` because the test server
        // is by definition a host this client has never seen, and
        // `UserKnownHostsFile` because the alternative is writing into the
        // developer's own `~/.ssh/known_hosts`.
        let args = vec![
            "ssh".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
            "-o".to_string(),
            format!("UserKnownHostsFile={}", known_hosts.display()),
            "interop@localhost".to_string(),
        ];
        let client = thread::spawn(move || {
            let config = ssh::parse_args_from(args)
                .map_err(|e| format!("client arguments rejected: {e}"))?;
            let secrets: SecretSource = counting_secrets;
            let mut session =
                ssh::SshSession::new(Box::new(client_side), config).with_secret_source(secrets);

            session
                .version_exchange()
                .and_then(|()| session.key_exchange())
                .map_err(|e| e.to_string())?;
            Ok(session.session_id().copied())
        });

        // Join both before asserting on either. A failure on one side leaves the
        // other blocked reading a message that will never arrive, until its
        // transport reports the peer gone -- so both threads do end, and both
        // outcomes are worth printing when something is wrong.
        let client_outcome: Result<Option<[u8; 32]>, String> =
            client.join().expect("the client thread must not panic");
        let server_outcome: Result<Option<[u8; 32]>, String> =
            server.join().expect("the server thread must not panic");

        let client_id = client_outcome
            .expect("the client must complete the handshake")
            .expect("the client must have a session id after key exchange");
        let server_id = server_outcome
            .expect("the server must complete the handshake")
            .expect("the server must have a session id after key exchange");
        (client_id, server_id)
    }

    /// The client and the server agree on a session identifier.
    ///
    /// RFC 4253 §7.2 makes the session id the exchange hash of the first key
    /// exchange, and both ends compute it independently from the same
    /// transcript: the two version strings, the two KEXINIT payloads, the host
    /// key, both Diffie-Hellman public values and the shared secret. Any
    /// disagreement about *any* of those -- a version string one side remembers
    /// differently, a field order, a length prefix, an integer encoding --
    /// produces two different session ids.
    ///
    /// That makes this one comparison the sharpest single assertion available
    /// about these two programs, and it is the one that was missing while six
    /// divergences shipped, both suites green throughout.
    #[test]
    fn the_client_and_the_server_derive_the_same_session_id() {
        let scratch = scratchdir::ScratchDir::new("ssh-interop-session-id");
        let (client_id, server_id) = one_handshake(&scratch.path("known_hosts"));

        assert_eq!(
            client_id, server_id,
            "the two ends of this protocol computed different exchange hashes, \
             which means they disagree about the handshake transcript"
        );
    }

    /// The same inputs produce the same handshake twice.
    ///
    /// This is what makes the previous test's agreement worth something. Two
    /// ends can agree on a value that neither of them computed the way the RFC
    /// says -- that is exactly the state this stack was in when the server
    /// hashed a placeholder client version and its own tests, recomputing the
    /// hash the same wrong way, agreed with it. Agreement alone cannot see
    /// that; it only sees the two ends drifting *apart*.
    ///
    /// Pinning the transcript is the other half. With the host key seed and both
    /// peers' secret sources fixed, every byte either end hashes is fixed, so
    /// the session id is a single value -- and any change to what either end
    /// sends, in what order, or what it feeds the hash, moves it. A deliberate
    /// protocol change is then expected to update this constant, which is the
    /// point: it forces the change to be looked at from both sides at once.
    #[test]
    fn the_handshake_transcript_is_the_one_recorded_here() {
        let scratch = scratchdir::ScratchDir::new("ssh-interop-transcript");
        let (client_id, _server_id) = one_handshake(&scratch.path("known_hosts"));

        assert_eq!(
            hex(&client_id),
            RECORDED_SESSION_ID,
            "the handshake transcript changed. If that was deliberate, update \
             RECORDED_SESSION_ID -- after checking the change is right on *both* \
             sides, which is the whole reason this constant exists."
        );
    }

    /// Lowercase hex, for an assertion failure a reader can compare by eye.
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
