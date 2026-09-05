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
        one_handshake_with(sshd::HostKey::from_seed(HOST_KEY_SEED), known_hosts)
    }

    /// Handshake, then let the client rekey `rekeys` times, returning each end's
    /// session identifier as it stood after every exchange.
    ///
    /// # Why more than one rekey is worth running
    ///
    /// A rekey that produced *different* keys at the two ends does not fail at
    /// the point of the mistake -- both ends finish the exchange happily, having
    /// each derived something. It fails on the next packet, which one end
    /// encrypts with a key the other does not have, and surfaces as a MAC
    /// failure attributed to whatever that packet happened to be.
    ///
    /// So each exchange here is the check on the one before it: the client's
    /// second `KEXINIT` travels under the keys the first rekey installed, and if
    /// those disagreed the server cannot read it. That makes a plain
    /// `expect("...")` on the outcome a real assertion about key agreement, with
    /// no need for this crate to invent traffic to send.
    fn handshake_then_rekeys(known_hosts: &Path, rekeys: usize) -> (Vec<[u8; 32]>, Vec<[u8; 32]>) {
        let (client_side, server_side) = sshwire::memory_pair();
        let host_key = sshd::HostKey::from_seed(HOST_KEY_SEED);

        let server = thread::spawn(move || {
            let secrets: SecretSource = counting_secrets;
            let mut conn = sshd::ConnectionState::new(
                Box::new(server_side),
                sshd::SshdConfig::default_config(),
                host_key,
                false,
            )
            .with_secret_source(secrets);

            sshd::do_version_exchange(&mut conn).map_err(|e| e.to_string())?;
            sshd::do_key_exchange(&mut conn).map_err(|e| e.to_string())?;

            let mut ids = vec![conn.session_id().ok_or("server has no session id")?];
            for i in 0..rekeys {
                // Exactly what the daemon's own dispatch loop does with a
                // KEXINIT that arrives mid-session: read the packet, hand it on.
                let kexinit = conn
                    .recv_packet()
                    .map_err(|e| format!("server reading rekey {i} KEXINIT: {e}"))?;
                sshd::do_rekey(&mut conn, &kexinit)
                    .map_err(|e| format!("server rekey {i}: {e}"))?;
                ids.push(conn.session_id().ok_or("server lost its session id")?);
            }
            Ok::<Vec<[u8; 32]>, String>(ids)
        });

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

            session.version_exchange().map_err(|e| e.to_string())?;
            session.key_exchange().map_err(|e| e.to_string())?;

            let mut ids = vec![*session.session_id().ok_or("client has no session id")?];
            for i in 0..rekeys {
                // The client's key exchange is written to be run whenever, not
                // only on a fresh connection, so a rekey is the same call again.
                session
                    .key_exchange()
                    .map_err(|e| format!("client rekey {i}: {e}"))?;
                ids.push(*session.session_id().ok_or("client lost its session id")?);
            }
            Ok::<Vec<[u8; 32]>, String>(ids)
        });

        let client_outcome = client.join().expect("the client thread must not panic");
        let server_outcome = server.join().expect("the server thread must not panic");
        (
            client_outcome.expect("the client must complete every exchange"),
            server_outcome.expect("the server must complete every exchange"),
        )
    }

    /// As [`one_handshake`], but the daemon serves the host key it is handed.
    ///
    /// Split out for [`the_daemon_starts_on_the_key_its_own_key_tool_writes`],
    /// which needs a key that came back out of a *file* rather than one built
    /// from a seed in memory. Every other caller wants the seed, so that stays
    /// the default rather than becoming a thing each test spells out.
    ///
    /// A `HostKey` may be moved onto the server's thread even though the peers
    /// themselves may not: it is two byte arrays, so it is `Send`. It is the
    /// `Box<dyn Transport>` inside `ConnectionState` that is not.
    fn one_handshake_with(host_key: sshd::HostKey, known_hosts: &Path) -> ([u8; 32], [u8; 32]) {
        let (client_side, server_side) = sshwire::memory_pair();

        let server = thread::spawn(move || {
            let secrets: SecretSource = counting_secrets;
            let mut conn = sshd::ConnectionState::new(
                Box::new(server_side),
                sshd::SshdConfig::default_config(),
                host_key,
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

    /// The client can rekey an established session, repeatedly, and the daemon
    /// serves it.
    ///
    /// SSH renegotiates its keys periodically (RFC 4253 §9). The failure this
    /// pins is not a wrong answer but a **hang**: a client that has sent
    /// `KEXINIT` may send nothing else until the exchange completes (§7.1), so a
    /// server that reads the message and does nothing leaves the session open,
    /// silent and stuck mid-command, with no error at either end. That was
    /// `TD-B-SSHD-DOES-NOT-REKEY-SO-A-LONG-SESSION-HANGS`.
    ///
    /// What is checked here is the exchange itself: the server side calls
    /// `sshd::do_rekey` with a `KEXINIT` it has just read, which is exactly what
    /// the daemon's dispatch arm does with the one it reads. That the arm is
    /// wired to it at all is a separate fact, and belongs to `sshd`'s own suite
    /// where the dispatcher is reachable --
    /// `a_kexinit_mid_session_starts_a_key_exchange_instead_of_being_ignored`.
    ///
    /// Three exchanges run: the handshake and two rekeys. The second rekey is
    /// what makes the first one's result meaningful -- see
    /// [`handshake_then_rekeys`].
    #[test]
    fn the_client_can_rekey_an_established_session_and_the_daemon_serves_it() {
        let scratch = scratchdir::ScratchDir::new("ssh-interop-rekey");
        let (client_ids, server_ids) = handshake_then_rekeys(&scratch.path("known_hosts"), 2);

        assert_eq!(client_ids.len(), 3, "one id per exchange");
        assert_eq!(server_ids.len(), 3, "one id per exchange");
        assert_eq!(
            client_ids, server_ids,
            "the two ends disagree about the session identifier after rekeying"
        );
    }

    /// A rekey does not change the session identifier.
    ///
    /// RFC 4253 §7.2: the session id is the exchange hash of the *first* key
    /// exchange and stays that for the life of the connection; a rekey derives
    /// new keys from its own hash but leaves this one alone.
    ///
    /// It is worth its own assertion because the damage is not confined to key
    /// derivation. The session id is the value a `publickey` signature is made
    /// over (RFC 4252 §7, and the tests further down), so a session id that
    /// moved under a rekey would retroactively invalidate the authentication the
    /// session was already running under -- and, because both ends would move it
    /// to the *same* new value, the tests above would go on passing.
    #[test]
    fn a_rekey_leaves_the_session_identifier_alone() {
        let scratch = scratchdir::ScratchDir::new("ssh-interop-rekey-session-id");
        let (client_ids, _) = handshake_then_rekeys(&scratch.path("known_hosts"), 2);

        for (i, id) in client_ids.iter().enumerate() {
            assert_eq!(
                id,
                client_ids.first().expect("there is at least one exchange"),
                "the session id changed at exchange {i}"
            );
        }
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

    /// The daemon starts on a key file its own key tool wrote.
    ///
    /// This is the test that was missing while the documented way to set the
    /// daemon up did not work:
    ///
    /// ```text
    /// ssh-keygen -t ed25519 -f /etc/ssh/ssh_host_ed25519_key
    /// sshd
    /// ```
    ///
    /// `ssh-keygen` wrapped `-----BEGIN ED25519 PRIVATE KEY-----` around a bare
    /// `seed || public || comment` blob — a container of its own invention that
    /// nothing else in this tree, or anywhere else, can read — so the second
    /// command failed on the key the first had just written. Both crates' test
    /// suites were green the whole time, because each one tested its encoder
    /// against its own decoder. That is the one arrangement that cannot notice a
    /// disagreement, and it is why this assertion has to live in a third crate
    /// that links both programs.
    ///
    /// It deliberately goes further than "the daemon parses the file". Parsing
    /// only proves the container survived the round trip; a key file could
    /// decode cleanly and still carry the wrong 32 bytes, or the right bytes
    /// under a name the client rejects. So the loaded key is put to work: it
    /// serves a whole handshake, the client verifies the signature it makes over
    /// the exchange hash, and the resulting session id is compared against the
    /// same recorded transcript the seed-built key produces. Nothing short of
    /// the file carrying exactly the key `ssh-keygen` generated, under the name
    /// both ends agree on, gets that far.
    #[test]
    fn the_daemon_starts_on_the_key_its_own_key_tool_writes() {
        // `encode_private_key` rather than `generate_key`: the latter draws a
        // seed and a checkint from `randrange`, which refuses on this host (see
        // open-questions.md), and a generated key would make the transcript
        // differ per run anyway. The `checkint` being a parameter is what lets a
        // test pin it; it is the format's own integrity field, two copies of the
        // same random word, and its *value* is not what is under test here.
        let keypair = ssh_keygen::Ed25519KeyPair::from_seed(HOST_KEY_SEED);
        let key_file = ssh_keygen::encode_private_key(
            &keypair.seed,
            &keypair.public,
            "interop@slateos",
            0x1234_5678,
        );

        let host_key = sshd::HostKey::from_openssh_text(&key_file).unwrap_or_else(|e| {
            panic!(
                "sshd refused the private key ssh-keygen wrote: {e}\n\
                 The file begins:\n{}",
                key_file.lines().next().unwrap_or("<empty>")
            )
        });

        let scratch = scratchdir::ScratchDir::new("ssh-interop-keygen-host-key");
        let (client_id, server_id) = one_handshake_with(host_key, &scratch.path("known_hosts"));

        assert_eq!(
            client_id, server_id,
            "the client and the daemon disagreed about a handshake the daemon \
             ran on a key ssh-keygen wrote"
        );
        assert_eq!(
            hex(&client_id),
            RECORDED_SESSION_ID,
            "the daemon read the key file without complaint but served a \
             different host key than the one ssh-keygen generated, so the file \
             does not carry the key it claims to"
        );
    }

    // ---- publickey authentication (RFC 4252 §7), both ends in one process ----
    //
    // The handshake tests above compare a value the two ends each *computed*.
    // These compare something narrower and sharper: bytes one end produced,
    // consumed by the other end's real decision function.
    //
    // §7 is the part of this protocol where a disagreement is least visible. The
    // client signs `sshwire::pubkey_signed_blob` and the server rebuilds it, and
    // nothing on the wire carries the blob itself — that is the design, since a
    // signature over bytes the sender chose would prove nothing. So a field the
    // two ends order differently, length-prefix differently, or one of them
    // omits, produces no decode error anywhere. It produces a signature that
    // fails to verify, which RFC 4252 §5.1 reports as a bare
    // `SSH_MSG_USERAUTH_FAILURE` — byte-identical to the reply for a key that
    // genuinely is not authorised. "Publickey does not work and nothing says
    // why" is the entire symptom, from both ends, for every possible cause.
    //
    // Three crates meet in these four tests, which is one more than the name of
    // this one suggests: `ssh-keygen` writes both files, `ssh` reads the private
    // one and signs, `sshd` reads the public one as `authorized_keys` and
    // verifies. Each of those is a contract between two programs that until now
    // only ever had one program's opinion of it.

    /// The seed the client's own key is built from.
    ///
    /// Deliberately not [`HOST_KEY_SEED`]: if the two were the same, "the client
    /// authenticated" could pass while the client was holding the *server's*
    /// key, which is not the thing being tested.
    const CLIENT_KEY_SEED: [u8; 32] = [0x5a; 32];

    /// A key belonging to nobody in these tests, for the rejection cases.
    const STRANGER_KEY_SEED: [u8; 32] = [0x17; 32];

    /// The account being authenticated, as bytes rather than as a `&str`.
    ///
    /// The signature covers the bytes the client put on the wire for this field.
    /// Spelling it as bytes at every point it appears is what keeps a `String`
    /// round trip — which rewrites anything that is not valid UTF-8 — out of the
    /// path between signing and verifying.
    const INTEROP_USER: &[u8] = b"interop";

    /// RFC 4252 §5: the service the authentication is *for*, bound into the
    /// signature so that one obtained for a different service cannot be reused.
    const SSH_CONNECTION: &[u8] = b"ssh-connection";

    /// The two files a user with a key actually has: the private key their
    /// client signs with, and the one line an administrator pastes into
    /// `authorized_keys`.
    ///
    /// Both come from `ssh-keygen` rather than being assembled here, because
    /// "the daemon accepts a key the client signed with" is only worth knowing
    /// if the key is one this tree's own tool produced. Building the
    /// `authorized_keys` line by hand in this file would make the test agree with
    /// a third copy of the format that no program reads.
    ///
    /// `encode_private_key` rather than `generate_key` for
    /// [`the_daemon_starts_on_the_key_its_own_key_tool_writes`]'s reason: the
    /// latter draws from `randrange`, which refuses on this host.
    fn key_files(seed: [u8; 32]) -> (String, String) {
        let keypair = ssh_keygen::Ed25519KeyPair::from_seed(seed);
        let private =
            ssh_keygen::encode_private_key(&keypair.seed, &keypair.public, "interop", 0x1234_5678);
        let authorized = ssh_keygen::public_key_line(&keypair.public, "interop");
        (private, authorized)
    }

    /// The client's own loader, on the text of a private key file.
    fn identity(private_key_file: &str) -> ssh::Identity {
        ssh::Identity::from_openssh_text(private_key_file)
            .expect("the client must load a key ssh-keygen wrote")
    }

    /// The daemon accepts a signature the client made over the session the two
    /// of them just negotiated.
    ///
    /// The session identifiers are the ones the *handshake* produced, and each
    /// end uses its own: the client signs with the value it derived, the daemon
    /// verifies with the value it derived. That is what happens on a real
    /// connection, and it means this test also fails if the two ever stop
    /// agreeing — the signature is over a value neither end was told.
    #[test]
    fn the_daemon_accepts_a_signature_the_client_made_over_the_session_they_negotiated() {
        let scratch = scratchdir::ScratchDir::new("ssh-interop-pubkey-accept");
        let (client_id, server_id) = one_handshake(&scratch.path("known_hosts"));

        let (private, authorized) = key_files(CLIENT_KEY_SEED);
        let request = identity(&private).auth_request(INTEROP_USER, SSH_CONNECTION, &client_id);

        let verdict = sshd::decide_publickey_request(&request, &server_id, &authorized)
            .expect("the daemon must be able to read the request its own peer built");

        assert_eq!(
            verdict,
            sshd::PubkeyOutcome::Accepted,
            "the daemon refused a signature the client made with the key that is \
             listed in the authorized_keys the daemon was given. The two ends \
             disagree about the bytes RFC 4252 §7 signs, or about the public key \
             blob, or about how ssh-keygen writes one of the two files."
        );
    }

    /// A key that is not in `authorized_keys` does not get in.
    ///
    /// The complement of the test above, and the reason that one means anything:
    /// a server that accepted everything would pass it. Here the signature is
    /// genuine and the key is real — the only thing wrong is that this account
    /// never listed it.
    ///
    /// No handshake: what is under test is the decision, and a session
    /// identifier is 32 bytes both ends were handed. The test above is the one
    /// that establishes those bytes are the ones a real connection produces.
    #[test]
    fn a_key_that_is_not_in_authorized_keys_is_refused() {
        let session_id = [0x11u8; 32];
        let (private, _my_own_line) = key_files(CLIENT_KEY_SEED);
        let (_stranger_private, stranger_line) = key_files(STRANGER_KEY_SEED);

        let request = identity(&private).auth_request(INTEROP_USER, SSH_CONNECTION, &session_id);
        let verdict = sshd::decide_publickey_request(&request, &session_id, &stranger_line)
            .expect("a well-formed request is readable whatever the daemon decides");

        assert_eq!(
            verdict,
            sshd::PubkeyOutcome::Rejected,
            "the daemon admitted a key that is not in the authorized_keys it was \
             given"
        );
    }

    /// A signature captured from one connection does not authenticate another.
    ///
    /// This is the whole job of the session identifier in the signed blob (RFC
    /// 4252 §7): it is the exchange hash of the first key exchange, which no
    /// single peer chooses, so a signature is usable only on the connection it
    /// was made for. A hostile server that collected one cannot present it
    /// elsewhere.
    ///
    /// The check is worth making across the boundary rather than inside either
    /// crate because the binding only works if both ends put the identifier in
    /// the same place. A pair of implementations that both omitted it would
    /// interoperate perfectly and be replayable by anyone.
    #[test]
    fn a_signature_made_for_one_session_does_not_authenticate_another() {
        let (private, authorized) = key_files(CLIENT_KEY_SEED);
        let request = identity(&private).auth_request(INTEROP_USER, SSH_CONNECTION, &[0x11; 32]);

        let verdict = sshd::decide_publickey_request(&request, &[0x22; 32], &authorized)
            .expect("a replayed request is still a well-formed one");

        assert_eq!(
            verdict,
            sshd::PubkeyOutcome::Rejected,
            "a signature made for one session authenticated a different one, so \
             the session identifier is not actually bound into what the two ends \
             sign and verify"
        );
    }

    /// Changing one byte of the signature is enough to be refused.
    ///
    /// Which sounds obvious, and is the exact bug this server shipped: it
    /// compared the offered public key against `authorized_keys`, found a match,
    /// and returned success without ever checking the signature — so anyone who
    /// could read a `.pub` file could log in as its owner. `sshd`'s own suite
    /// covers that now; this is the same question asked of a signature the real
    /// client really produced, rather than one the server's tests built.
    ///
    /// The last byte of the packet is the last byte of the signature, since the
    /// signature is the final field. Flipping a bit in it leaves every length
    /// prefix intact, so the request stays perfectly well-formed and the only
    /// thing wrong with it is the one thing that must be checked.
    #[test]
    fn a_signature_with_one_bit_changed_is_refused() {
        let session_id = [0x11u8; 32];
        let (private, authorized) = key_files(CLIENT_KEY_SEED);
        let mut request =
            identity(&private).auth_request(INTEROP_USER, SSH_CONNECTION, &session_id);

        let last = request.len() - 1;
        request[last] ^= 0x01;

        let verdict = sshd::decide_publickey_request(&request, &session_id, &authorized)
            .expect("flipping a signature bit does not malform the packet");

        assert_eq!(
            verdict,
            sshd::PubkeyOutcome::Rejected,
            "the daemon accepted a request whose signature had been altered, \
             which means it is not verifying the signature at all"
        );
    }

    /// Lowercase hex, for an assertion failure a reader can compare by eye.
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
