//! Slate OS SSH Key Generation Utility
//!
//! Generates and manages SSH key pairs for use with Slate OS's SSH client.
//! Supports Ed25519 key pairs (the primary and recommended key type).
//!
//! # Usage
//!
//! ```text
//! ssh-keygen                           Generate Ed25519 key with defaults
//! ssh-keygen -t ed25519                Explicitly request Ed25519
//! ssh-keygen -f ~/.ssh/mykey           Write to a custom file
//! ssh-keygen -C "my comment"           Set key comment
//! ssh-keygen -l -f ~/.ssh/id_ed25519   Show SHA-256 fingerprint
//! ssh-keygen -y -f ~/.ssh/id_ed25519   Print public key from private key
//! ssh-keygen -q -f ~/.ssh/id_ed25519   Quiet mode
//! ```
//!
//! # Key Format
//!
//! Public key:  `ssh-ed25519 <base64> <comment>` (OpenSSH format)
//! Private key: `openssh-key-v1`, unencrypted, as OpenSSH writes it
//!
//! Both encodings live in `sshwire`, which is the crate that exists so that
//! two programs cannot disagree about a format. Until they moved there, the
//! private key was written in a band of this tool's own invention --
//! `-----BEGIN ED25519 PRIVATE KEY-----` around a bare
//! `seed || public || comment` blob -- which nothing else could read, so
//! generating a host key here and starting `sshd` did not work.
//!
//! # Cryptography
//!
//! - Ed25519 key derivation (RFC 8032 §5.1.5) from `posix::ed25519`
//! - SHA-256 for fingerprints, from `sha2`
//!
//! Neither is implemented here, and the Ed25519 one is the reason the sentence
//! above used to read "implemented from first principles". This file carried
//! its own field arithmetic, its own curve, and its own SHA-512, and the public
//! key it derived was **wrong** -- it disagreed with RFC 8032's own test vector,
//! so every key pair it ever produced had a public half that did not match its
//! private half. Its tests did not notice, because they compared its output
//! against its own output. See [`Ed25519KeyPair::from_seed`].

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
//
// The arithmetic and indexing allows are what remains of a much broader
// exemption. They were granted for this crate's own field arithmetic mod
// 2^255 - 19 and its Edwards-curve point operations, where every operation is
// modular by construction — code that no longer exists here. What is left is
// offset arithmetic while walking a key blob, bounded by the length checks that
// precede it.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use std::env;
use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};

// ============================================================================
// I/O + randomness
// ============================================================================
//
// All file and stdout/stderr I/O routes through std, which reaches the native
// Slate OS syscalls via the posix libc layer.  A previous hand-rolled syscall
// stub here hardcoded Linux numbers that collide with unrelated native
// syscalls — WRITE=1=SYS_EXIT (so every write terminated the process),
// OPEN=2=SYS_TASK_ID, CLOSE=3 unassigned, STAT=4 unassigned, EXIT=60=
// SYS_SYSCTL_GET, MKDIR=83 unassigned — making the tool completely
// non-functional.  Randomness comes from `randrange::fill_secret`, which
// reaches the kernel CSPRNG through the linked libc's `getrandom` symbol
// because no std API exposes it.
//
// That extern used to be written out here, and the same eight lines were
// written out twice more (in `ssh` and `sshd`, via `posix::random`). Of the
// three, this was the only correct one: the other two called into an rlib copy
// of `posix` whose syscalls are stubbed out in a program build, so they reached
// a hardware-RDRAND fallback the guest CPU does not have. One copy in one crate
// is what stops the next tool from picking the wrong one.

/// Fill `buf` with cryptographically random bytes from the kernel CSPRNG.
fn fill_random(buf: &mut [u8]) -> Result<(), KeygenError> {
    randrange::fill_secret(buf).map_err(|_| KeygenError::RandomFailed)
}

/// Apply a Unix permission `mode` to `path` (best effort; no-op off-unix).
#[cfg(unix)]
fn set_mode(path: &str, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_mode(_path: &str, _mode: u32) {}

/// Write `data` to stdout.
fn write_stdout(data: &[u8]) -> Result<(), KeygenError> {
    std::io::stdout()
        .write_all(data)
        .map_err(|_| KeygenError::WriteError("stdout".to_string()))
}

/// Write `data` to stderr (best effort).
fn write_stderr(data: &[u8]) {
    let _ = std::io::stderr().write_all(data);
}

/// Create a directory with the given `mode`, ignoring "already exists".
fn mkdir(path: &str, mode: u32) {
    // Best effort: only stamp the mode when we actually created the directory.
    // Other errors (e.g. parent missing) surface when we try to create files
    // inside, mirroring the previous behaviour.
    if std::fs::create_dir(path).is_ok() {
        set_mode(path, mode);
    }
}

/// Check whether a path exists.
fn path_exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// Read an entire file into a `Vec<u8>`.
fn read_file(path: &str) -> Result<Vec<u8>, KeygenError> {
    std::fs::read(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => KeygenError::FileNotFound(path.to_string()),
        _ => KeygenError::ReadError,
    })
}

/// Write `data` to `path` (creating/truncating) and apply `mode`.
fn write_file(path: &str, data: &[u8], mode: u32) -> Result<(), KeygenError> {
    std::fs::write(path, data).map_err(|_| KeygenError::WriteError(path.to_string()))?;
    set_mode(path, mode);
    Ok(())
}

/// Terminate the process with the given exit code.
fn exit(code: i32) -> ! {
    std::process::exit(code)
}

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug)]
pub enum KeygenError {
    RandomFailed,
    ReadError,
    WriteError(String),
    FileNotFound(String),
    FileExists(String),
    InvalidBase64,
    InvalidKeyFile(String),
    UnsupportedKeyType(String),
    ParseError(String),
}

impl fmt::Display for KeygenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RandomFailed => write!(f, "failed to get random bytes from kernel"),
            Self::ReadError => write!(f, "read error"),
            Self::WriteError(p) => write!(f, "write error: {p}"),
            Self::FileNotFound(p) => write!(f, "no such file: {p}"),
            Self::FileExists(p) => write!(f, "file already exists: {p}"),
            Self::InvalidBase64 => write!(f, "invalid base64 data"),
            Self::InvalidKeyFile(m) => write!(f, "invalid key file: {m}"),
            Self::UnsupportedKeyType(t) => write!(f, "unsupported key type: {t}"),
            Self::ParseError(m) => write!(f, "parse error: {m}"),
        }
    }
}

impl From<sshwire::Base64Error> for KeygenError {
    fn from(_: sshwire::Base64Error) -> Self {
        Self::InvalidBase64
    }
}

/// `sshwire` already distinguishes "encrypted", "wrong key type", "the two
/// checkints disagree" and "truncated at field X" by name, and those names are
/// the value of its error type: they say which remedy applies. Carrying the
/// message through keeps them. The hand-written parser this replaces printed
/// "invalid key file: missing header" for every one of those cases, including
/// the common one of pointing the tool at an encrypted key.
impl From<sshwire::PrivateKeyError> for KeygenError {
    fn from(e: sshwire::PrivateKeyError) -> Self {
        Self::InvalidKeyFile(e.to_string())
    }
}

// Base64 lives in `sshwire` now, with the RFC 4648 vectors that used to be
// checked here. It is the same argument the SHA-256 below already carries, one
// step out: a private copy of a public agreement is a copy free to drift, and
// this one is read by `ssh` and `sshd` rather than only by this tool.

// ============================================================================
// SHA-256
// ============================================================================

/// Compute SHA-256 of `data`. Returns the 32-byte digest.
///
/// Delegates to the shared `sha2` crate.  This file used to carry its own
/// SHA-256, which in a key tool is a private copy of a public agreement: the
/// fingerprint it prints is the string a human compares against one printed by
/// somebody else's `ssh-keygen`, and the digest inside a signature has to
/// match what the far end computes.
///
/// This is now the only hash this file mentions. There used to be a private
/// SHA-512 below it as well, kept because `sha2` provides only SHA-256 and
/// Ed25519 needs SHA-512 by definition -- but that argument only held while
/// this crate did its own Ed25519, and it no longer does. The SHA-512 went out
/// with the curve arithmetic that was its only caller.
fn sha256(data: &[u8]) -> [u8; 32] {
    sha2::sha256(data)
}

// ============================================================================
// Ed25519 — Key derivation (RFC 8032 §5.1.5)
// ============================================================================

/// An Ed25519 key pair: private seed plus the public point it determines.
pub struct Ed25519KeyPair {
    /// The 32-byte random seed. This is the private key; everything else is
    /// derived from it, which is why the key file stores this and not a scalar.
    pub seed: [u8; 32],
    /// The 32-byte compressed public key point.
    pub public: [u8; 32],
}

impl Ed25519KeyPair {
    /// Derive a key pair from a 32-byte random seed per RFC 8032 §5.1.5.
    ///
    /// This delegates to `posix::ed25519`, which is the same implementation
    /// `sshd` verifies host key signatures with, and it was not always so.
    /// This file carried roughly 560 lines of its own Ed25519 -- a `Fe` field
    /// element over five 51-bit limbs, an `EdPoint` in extended coordinates, a
    /// scalar multiply and a private SHA-512 -- and **it computed the wrong
    /// public key**. Fed RFC 8032 §7.1 vector 1 it returned
    /// `e000725923fbbc...` where the RFC says `d75a980182b10a...`.
    ///
    /// That is not a cosmetic defect. The public half of every key this tool
    /// ever generated did not correspond to its private half, so a signature
    /// made with the seed could not verify against the published key: an
    /// `authorized_keys` line copied from its output would reject its own
    /// owner, and a host key it wrote would make every client report a bad
    /// signature. The crate's own suite was green, because its tests derived a
    /// public key with the broken code and compared it against a public key
    /// derived with the broken code. Only an external vector -- or, as it
    /// happened, `sshd` deriving the public half itself and disagreeing -- can
    /// see a fault that both sides of an equality share.
    ///
    /// `posix::ed25519` is checked against all four RFC 8032 §7.1 vectors, for
    /// key derivation, signing *and* verification. Two implementations of one
    /// standard is one more than the number that can be tested.
    #[must_use]
    pub fn from_seed(seed: [u8; 32]) -> Self {
        let public = posix::ed25519::public_key(&seed);
        Ed25519KeyPair { seed, public }
    }
}

// ============================================================================
// OpenSSH key format
// ============================================================================

/// The OpenSSH key type identifier for Ed25519.
const KEY_TYPE_ED25519: &str = "ssh-ed25519";

// `ssh_u32` and `ssh_string` are gone: this crate's last two uses of them were
// building the public key blob, which `sshwire::ed25519_public_blob` now does,
// and the fourth copy of RFC 4253 section 6's length-prefixed string is one
// fewer place for it to be written wrong.

/// Build an OpenSSH public key wire encoding for Ed25519:
/// `string("ssh-ed25519") || string(public_key_bytes)`.
///
/// This is the blob that goes on a `known_hosts` or `authorized_keys` line, so
/// it is `sshwire`'s to build: the client reads what this writes.
fn encode_public_key(public: &[u8; 32]) -> Vec<u8> {
    sshwire::ed25519_public_blob(public)
}

/// Format the complete public key line: `ssh-ed25519 <base64> <comment>`.
pub fn public_key_line(public: &[u8; 32], comment: &str) -> String {
    let wire = encode_public_key(public);
    let b64 = sshwire::base64_encode_padded(&wire);
    format!("{KEY_TYPE_ED25519} {b64} {comment}")
}

/// Encode the private key in the `openssh-key-v1` format, unencrypted.
///
/// The previous format was this tool's own invention: a
/// `-----BEGIN ED25519 PRIVATE KEY-----` band around a bare
/// `seed || public || comment` blob. Nothing could read it -- not `sshd`, not
/// OpenSSH, not this project's own client -- so `ssh-keygen -f
/// /etc/ssh/ssh_host_ed25519_key` followed by `sshd` produced a daemon that
/// refused to start on the key its own key tool had just written.
///
/// `checkint` is a parameter rather than drawn inside because this function
/// must be deterministic for a test to be able to state what it writes; the
/// caller that writes a real key draws it from the CSPRNG.
pub fn encode_private_key(
    seed: &[u8; 32],
    public: &[u8; 32],
    comment: &str,
    checkint: u32,
) -> String {
    sshwire::encode_openssh_private_key(seed, public, comment, checkint)
}

/// Parse a private key file and return `(seed, public, comment)`.
///
/// # Errors
///
/// Fails if the file is not an unencrypted `openssh-key-v1` Ed25519 key. The
/// refusals are named -- an encrypted key, a key of another type, a truncated
/// body -- rather than collapsed into one "invalid key file", because the
/// remedy differs in each case.
fn decode_private_key(data: &str) -> Result<([u8; 32], [u8; 32], String), KeygenError> {
    let key = sshwire::decode_openssh_private_key(data)?;
    Ok((key.seed, key.public, key.comment))
}

/// Parse a public key line and return `(wire_bytes, comment)`.
///
/// Expected format: `ssh-ed25519 <base64> [comment]`
fn parse_public_key_line(line: &str) -> Result<([u8; 32], String), KeygenError> {
    let mut parts = line.splitn(3, ' ');
    let keytype = parts
        .next()
        .ok_or_else(|| KeygenError::ParseError("empty line".to_string()))?;
    let b64 = parts
        .next()
        .ok_or_else(|| KeygenError::ParseError("missing base64".to_string()))?;
    let comment = parts.next().unwrap_or("").to_string();

    if keytype != KEY_TYPE_ED25519 {
        return Err(KeygenError::UnsupportedKeyType(keytype.to_string()));
    }

    let wire = sshwire::base64_decode(b64.as_bytes())?;

    // Parse wire format: string("ssh-ed25519") || string(public_key_32_bytes).
    //
    // `read_ssh_string` rather than four hand-written `u32::from_be_bytes`
    // reads over a cursor. The version that did it by hand had, in its own
    // eight lines, `4 + type_len + 4`, `key_len_offset + 4` and
    // `key_start + key_len` as three separately-written statements of one
    // layout -- each one a place where the bound checked and the range indexed
    // could part company, under a file-level `#![allow(indexing_slicing)]`.
    // The reader advances the cursor itself, so there is one statement of it.
    let (declared_type, pos) = sshwire::read_ssh_string(&wire, 0)
        .map_err(|e| KeygenError::InvalidKeyFile(e.to_string()))?;
    let (key, _) = sshwire::read_ssh_string(&wire, pos)
        .map_err(|e| KeygenError::InvalidKeyFile(e.to_string()))?;

    // The blob carries its own algorithm name, and until now nothing compared
    // it against the one in the first field. A line reading
    // `ssh-ed25519 <a base64 ssh-rsa blob>` was accepted, and the 32 bytes
    // taken from wherever the rsa blob happened to have them became "the
    // public key" -- with no error, and a fingerprint that looked plausible.
    if declared_type != KEY_TYPE_ED25519.as_bytes() {
        return Err(KeygenError::UnsupportedKeyType(
            String::from_utf8_lossy(declared_type).into_owned(),
        ));
    }

    let public: [u8; 32] = key
        .try_into()
        .map_err(|_| KeygenError::InvalidKeyFile("bad public key length".to_string()))?;
    Ok((public, comment))
}

// ============================================================================
// Fingerprint
// ============================================================================

/// Compute and format the SHA-256 fingerprint of a public key wire encoding.
///
/// Format: `SHA256:<base64_no_padding>` (OpenSSH convention).
pub fn fingerprint(public: &[u8; 32]) -> String {
    let wire = encode_public_key(public);
    let digest = sha256(&wire);
    // OpenSSH omits trailing `=` padding from fingerprints, which is why
    // `sshwire` names its two encoders apart instead of picking a default:
    // `base64_encode` is unpadded and `base64_encode_padded` is not, so the
    // call site says which it means rather than stripping the padding back off
    // afterwards.
    let b64 = sshwire::base64_encode(&digest);
    format!("SHA256:{b64}")
}

// ============================================================================
// Argument parsing
// ============================================================================

#[derive(Debug, Default)]
struct Args {
    /// Key type (only "ed25519" supported).
    key_type: Option<String>,
    /// Output file path.
    output_file: Option<String>,
    /// Key comment.
    comment: Option<String>,
    /// Show fingerprint mode.
    show_fingerprint: bool,
    /// Print public key from private key.
    print_public: bool,
    /// Quiet mode.
    quiet: bool,
}

fn parse_args(args: &[String]) -> Result<Args, KeygenError> {
    let mut out = Args::default();
    let mut i = 1usize; // skip argv[0]
    while i < args.len() {
        match args[i].as_str() {
            "-t" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| {
                    KeygenError::ParseError("-t requires an argument".to_string())
                })?;
                out.key_type = Some(val.clone());
            }
            "-f" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| {
                    KeygenError::ParseError("-f requires an argument".to_string())
                })?;
                out.output_file = Some(val.clone());
            }
            "-C" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| {
                    KeygenError::ParseError("-C requires an argument".to_string())
                })?;
                out.comment = Some(val.clone());
            }
            "-l" => out.show_fingerprint = true,
            "-y" => out.print_public = true,
            "-q" => out.quiet = true,
            other => {
                return Err(KeygenError::ParseError(format!("unknown option: {other}")));
            }
        }
        i += 1;
    }
    Ok(out)
}

// ============================================================================
// Key file paths
// ============================================================================

/// Resolve the default private key path: `~/.ssh/id_ed25519`.
fn default_key_path() -> String {
    match env::var("HOME") {
        Ok(home) => format!("{home}/.ssh/id_ed25519"),
        Err(_) => "id_ed25519".to_string(),
    }
}

/// Derive the public key path from the private key path (append `.pub`).
fn public_key_path(private_path: &str) -> String {
    format!("{private_path}.pub")
}

// ============================================================================
// Top-level operations
// ============================================================================

/// Generate a new Ed25519 key pair and write it to disk.
fn generate_key(args: &Args) -> Result<(), KeygenError> {
    // Validate key type if specified.
    if let Some(t) = &args.key_type
        && t != "ed25519"
    {
        return Err(KeygenError::UnsupportedKeyType(t.clone()));
    }

    let priv_path = args.output_file.clone().unwrap_or_else(default_key_path);
    let pub_path = public_key_path(&priv_path);

    let comment = args.comment.clone().unwrap_or_else(|| {
        // Default comment: user@hostname (simplified — just use the path).
        format!("generated-key-{priv_path}")
    });

    // Generate 32 random bytes as the seed.
    let mut seed = [0u8; 32];
    fill_random(&mut seed)?;

    let kp = Ed25519KeyPair::from_seed(seed);

    // Ensure the parent directory exists.
    if let Some(parent) = PathBuf::from(&priv_path).parent()
        && let Some(p) = parent.to_str()
        && !p.is_empty()
    {
        mkdir(p, 0o700);
    }

    // Refuse to overwrite an existing private key.
    if path_exists(&priv_path) {
        return Err(KeygenError::FileExists(priv_path));
    }

    // Write the private key (mode 0600 — owner read/write only).
    //
    // The `checkint` is the `openssh-key-v1` format's own integrity check: the
    // same random 32 bits appear twice at the head of the private section, and
    // a decryption with the wrong passphrase makes them disagree. This key is
    // unencrypted, so the check can never fire -- but it is written from the
    // CSPRNG anyway rather than left at zero, because the file is the input to
    // any tool that *does* encrypt it later, and a constant there would make a
    // wrong passphrase indistinguishable from a right one.
    let mut checkint = [0u8; 4];
    fill_random(&mut checkint)?;
    let priv_content =
        encode_private_key(&kp.seed, &kp.public, &comment, u32::from_be_bytes(checkint));
    write_file(&priv_path, priv_content.as_bytes(), 0o600)?;

    // Write the public key (mode 0644).
    let pub_line = public_key_line(&kp.public, &comment);
    let mut pub_content = pub_line.clone();
    pub_content.push('\n');
    write_file(&pub_path, pub_content.as_bytes(), 0o644)?;

    if !args.quiet {
        let msg = format!("Your identification has been saved in {priv_path}\n");
        write_stdout(msg.as_bytes())?;
        let msg = format!("Your public key has been saved in {pub_path}\n");
        write_stdout(msg.as_bytes())?;
        let fp = fingerprint(&kp.public);
        let msg = format!("The key fingerprint is:\n{fp} {comment}\n");
        write_stdout(msg.as_bytes())?;
    }

    Ok(())
}

/// Show the fingerprint of a key file.
fn show_fingerprint(args: &Args) -> Result<(), KeygenError> {
    let path = args.output_file.clone().unwrap_or_else(default_key_path);

    // Try reading as a public key first, then as a private key.
    let public = if path.ends_with(".pub") {
        let data = read_file(&path)?;
        let line = String::from_utf8_lossy(&data);
        let (pub_key, _) = parse_public_key_line(line.trim())?;
        pub_key
    } else {
        let data = read_file(&path)?;
        let s = String::from_utf8_lossy(&data);
        // Deciding by *trying* the private-key decode rather than by looking
        // for its header string: only `NotPem` -- there is no PEM band at all
        // -- means "this might be a public key line instead". The old sniff
        // was a `contains` on the header, so a private key that was truncated,
        // encrypted, or of another type fell through to the public-key parser
        // and was reported as a malformed public key line, which names neither
        // the file nor the problem.
        match sshwire::decode_openssh_private_key(&s) {
            Ok(key) => key.public,
            Err(sshwire::PrivateKeyError::NotPem) => parse_public_key_line(s.trim())?.0,
            Err(e) => return Err(e.into()),
        }
    };

    let fp = fingerprint(&public);
    let comment = if path.ends_with(".pub") {
        let data = read_file(&path)?;
        let line = String::from_utf8_lossy(&data);
        let (_, c) = parse_public_key_line(line.trim())?;
        c
    } else {
        path.clone()
    };
    let msg = format!("256 {fp} {comment} (ED25519)\n");
    write_stdout(msg.as_bytes())?;
    Ok(())
}

/// Read a private key file and print the corresponding public key.
fn print_public_key(args: &Args) -> Result<(), KeygenError> {
    let path = args.output_file.clone().unwrap_or_else(default_key_path);
    let data = read_file(&path)?;
    let s = String::from_utf8_lossy(&data);
    let (_, public, comment) = decode_private_key(&s)?;
    let line = public_key_line(&public, &comment);
    let mut out = line;
    out.push('\n');
    write_stdout(out.as_bytes())?;
    Ok(())
}

// ============================================================================
// Entry point
// ============================================================================

/// Run one invocation of `ssh-keygen`, reading its arguments from the process.
///
/// # Errors
///
/// Returns whatever the requested operation failed with; the binary prints it
/// and exits non-zero.
pub fn run() -> Result<(), KeygenError> {
    let argv: Vec<String> = env::args().collect();
    let args = parse_args(&argv)?;

    if args.show_fingerprint {
        show_fingerprint(&args)
    } else if args.print_public {
        print_public_key(&args)
    } else {
        generate_key(&args)
    }
}

/// Report `e` on stderr the way the binary does, then exit non-zero.
///
/// Lives here rather than in `main.rs` because `write_stderr` and `exit` are
/// this crate's own: exporting two low-level helpers so a three-line `main`
/// can reassemble the message is a wider seam than exporting the message.
pub fn report_and_exit(e: &KeygenError) -> ! {
    let msg = format!("ssh-keygen: {e}\n");
    write_stderr(msg.as_bytes());
    exit(1);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    // The only string built by folding here is a hex dump inside an assertion
    // message, where being able to compare two key values by eye is the point
    // and a `format!` per byte on a failing test costs nothing.
    clippy::format_collect
)]
mod tests {
    use super::*;

    // The base64 tests moved to `sshwire` with the functions, including the
    // RFC 4648 vectors that were checked here and in the client and the
    // daemon -- three suites, three encoders, one specification.

    // --- SHA-256 tests ---

    #[test]
    fn test_sha256_empty() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb924...
        let digest = sha256(b"");
        assert_eq!(
            digest,
            [
                0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
                0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
                0x78, 0x52, 0xb8, 0x55,
            ]
        );
    }

    #[test]
    fn test_sha256_abc() {
        // SHA-256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        // (canonical FIPS 180-4 example).
        let digest = sha256(b"abc");
        assert_eq!(
            digest,
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]
        );
    }

    #[test]
    fn test_sha256_448_bit_message() {
        // "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        let digest = sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq");
        assert_eq!(
            digest,
            [
                0x24, 0x8d, 0x6a, 0x61, 0xd2, 0x06, 0x38, 0xb8, 0xe5, 0xc0, 0x26, 0x93, 0x0c, 0x3e,
                0x60, 0x39, 0xa3, 0x3c, 0xe4, 0x59, 0x64, 0xff, 0x21, 0x67, 0xf6, 0xec, 0xed, 0xd4,
                0x19, 0xdb, 0x06, 0xc1,
            ]
        );
    }

    // The SHA-512, field-element and curve-point tests went out with the code
    // they tested. Nothing was lost by deleting them: every one of them checked
    // an internal identity -- x + 0 == x, x - x == 0, -(-x) == x, 0*G is the
    // identity, the base point is not the identity -- and all of them passed
    // while the derivation built on top returned the wrong public key. A
    // self-consistent implementation of the wrong curve satisfies each of those
    // by construction.
    //
    // The three Ed25519 tests that stood here were worse than useless, because
    // they looked like coverage of the thing that was broken: "an all-zero seed
    // gives a non-zero public key", "an all-ones seed gives a non-zero public
    // key", "the same seed twice gives the same public key". A function
    // returning a fixed non-zero constant passes all three. What replaces them
    // is `the_public_key_derived_here_is_the_one_rfc_8032_specifies`, which
    // compares against a number from outside this tree.

    #[test]
    fn test_ed25519_different_seeds_different_keys() {
        let seed1 = [1u8; 32];
        let seed2 = [2u8; 32];
        let kp1 = Ed25519KeyPair::from_seed(seed1);
        let kp2 = Ed25519KeyPair::from_seed(seed2);
        assert_ne!(kp1.public, kp2.public);
    }

    // --- Public key format ---

    #[test]
    fn test_public_key_line_prefix() {
        let public = [0u8; 32];
        let line = public_key_line(&public, "test@host");
        assert!(line.starts_with("ssh-ed25519 "));
        assert!(line.ends_with("test@host"));
    }

    #[test]
    fn test_parse_public_key_roundtrip() {
        let seed = [77u8; 32];
        let kp = Ed25519KeyPair::from_seed(seed);
        let line = public_key_line(&kp.public, "user@example.com");
        let (parsed_pub, comment) = parse_public_key_line(&line).unwrap();
        assert_eq!(parsed_pub, kp.public);
        assert_eq!(comment, "user@example.com");
    }

    #[test]
    fn test_parse_public_key_wrong_type() {
        let result = parse_public_key_line("ssh-rsa AAAA== comment");
        assert!(result.is_err());
    }

    // --- Private key format ---

    #[test]
    fn test_private_key_roundtrip() {
        let seed = [0xabu8; 32];
        let kp = Ed25519KeyPair::from_seed(seed);
        let pem = encode_private_key(&kp.seed, &kp.public, "my-comment", 0x1234_5678);
        let (dec_seed, dec_pub, dec_comment) = decode_private_key(&pem).unwrap();
        assert_eq!(dec_seed, kp.seed);
        assert_eq!(dec_pub, kp.public);
        assert_eq!(dec_comment, "my-comment");
    }

    /// The public key this tool derives is the one RFC 8032 says it is.
    ///
    /// The single test this crate most needed and did not have. Every other
    /// test of key derivation here compares one of this crate's outputs against
    /// another of its outputs -- generate a pair, encode it, decode it, check
    /// the public key survived -- and all of them passed while the derivation
    /// itself was wrong. A self-consistent implementation of the wrong curve
    /// passes every internal check by construction; only a value that came from
    /// outside this tree can say whether the curve is right.
    ///
    /// The seed and the expected public key are RFC 8032 §7.1's first vector,
    /// copied from the RFC and not from any implementation. `posix::ed25519`
    /// checks the same vector, and all three others, along with signing and
    /// verification -- but it checks them for `posix`, and the assertion that
    /// matters to a *user of this tool* is that the bytes it writes into a key
    /// file are these. Keeping the vector on this side of the call is what
    /// notices if this crate ever grows its own derivation again.
    #[test]
    fn the_public_key_derived_here_is_the_one_rfc_8032_specifies() {
        const SEED: [u8; 32] = [
            0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec,
            0x2c, 0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03,
            0x1c, 0xae, 0x7f, 0x60,
        ];
        let kp = Ed25519KeyPair::from_seed(SEED);
        let got: String = kp.public.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            got, "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
            "the public key derived from RFC 8032 vector 1's seed is not the one \
             the RFC gives, so the public half of every key this tool writes does \
             not match its private half"
        );
    }

    #[test]
    fn the_private_key_this_tool_writes_is_the_one_openssh_writes() {
        // The point of this test is the *spelling* of the band, which is what
        // was wrong: this tool wrote `-----BEGIN ED25519 PRIVATE KEY-----`, a
        // header of its own invention, around a bare `seed || public ||
        // comment` blob. `sshd` looks for `openssh-key-v1` and found neither
        // the header nor a payload it could parse, so generating a host key
        // here and starting the daemon failed. Asserting on the literal string
        // rather than on a shared constant is deliberate: a constant would let
        // both ends move together, which is the failure mode, and this string
        // is fixed by OpenSSH and not by us.
        let kp = Ed25519KeyPair::from_seed([0u8; 32]);
        let pem = encode_private_key(&kp.seed, &kp.public, "c", 0);
        assert!(pem.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----\n"));
        assert!(pem.ends_with("-----END OPENSSH PRIVATE KEY-----\n"));
    }

    #[test]
    fn test_private_key_decode_missing_header() {
        let result = decode_private_key("not a key");
        assert!(result.is_err());
    }

    #[test]
    fn a_key_this_tool_writes_is_a_key_the_daemon_reads() {
        // `sshd` cannot be a dependency of this binary, so what is checked
        // here is that the file goes through the *shared* decoder -- the one
        // `sshd::HostKey::load_from_file` calls. The end-to-end version of
        // this, both programs in one process, lives in `ssh-interop`.
        let kp = Ed25519KeyPair::from_seed([7u8; 32]);
        let pem = encode_private_key(&kp.seed, &kp.public, "host-key", 0xdead_beef);
        let read = sshwire::decode_openssh_private_key(&pem).unwrap();
        assert_eq!(read.seed, kp.seed);
        assert_eq!(read.public, kp.public);
        assert_eq!(read.comment, "host-key");
    }

    #[test]
    fn a_public_key_line_whose_blob_names_another_algorithm_is_refused() {
        // The first field and the algorithm name inside the blob are two
        // statements of one fact, and nothing used to compare them: the parser
        // checked the first field, then took 32 bytes from wherever the blob
        // happened to have them. An `ssh-ed25519` line carrying an `ssh-rsa`
        // blob produced a plausible-looking key and a plausible-looking
        // fingerprint, with no error anywhere.
        let mut blob = Vec::new();
        blob.extend_from_slice(&u32::try_from("ssh-rsa".len()).unwrap().to_be_bytes());
        blob.extend_from_slice(b"ssh-rsa");
        blob.extend_from_slice(&32u32.to_be_bytes());
        blob.extend_from_slice(&[9u8; 32]);
        let line = format!("ssh-ed25519 {} c", sshwire::base64_encode_padded(&blob));

        let err = parse_public_key_line(&line).unwrap_err();
        assert!(
            matches!(err, KeygenError::UnsupportedKeyType(ref t) if t == "ssh-rsa"),
            "expected the blob's own name in the refusal, got {err}"
        );
    }

    // --- Fingerprint ---

    #[test]
    fn test_fingerprint_prefix() {
        let public = [0u8; 32];
        let fp = fingerprint(&public);
        assert!(fp.starts_with("SHA256:"));
    }

    #[test]
    fn test_fingerprint_deterministic() {
        let public = [42u8; 32];
        let fp1 = fingerprint(&public);
        let fp2 = fingerprint(&public);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn test_fingerprint_different_keys_different_fp() {
        let pub1 = [1u8; 32];
        let pub2 = [2u8; 32];
        assert_ne!(fingerprint(&pub1), fingerprint(&pub2));
    }

    // --- Argument parsing ---

    #[test]
    fn test_parse_args_defaults() {
        let args: Vec<String> = vec!["ssh-keygen".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(parsed.key_type.is_none());
        assert!(parsed.output_file.is_none());
        assert!(!parsed.show_fingerprint);
        assert!(!parsed.print_public);
        assert!(!parsed.quiet);
    }

    #[test]
    fn test_parse_args_all_flags() {
        let args: Vec<String> = [
            "ssh-keygen",
            "-t",
            "ed25519",
            "-f",
            "/tmp/key",
            "-C",
            "my comment",
            "-l",
            "-q",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.key_type.as_deref(), Some("ed25519"));
        assert_eq!(parsed.output_file.as_deref(), Some("/tmp/key"));
        assert_eq!(parsed.comment.as_deref(), Some("my comment"));
        assert!(parsed.show_fingerprint);
        assert!(parsed.quiet);
    }

    #[test]
    fn test_parse_args_unknown_flag() {
        let args: Vec<String> = vec!["ssh-keygen".to_string(), "-z".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_missing_t_value() {
        let args: Vec<String> = vec!["ssh-keygen".to_string(), "-t".to_string()];
        assert!(parse_args(&args).is_err());
    }
}
