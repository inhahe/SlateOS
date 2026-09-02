#![deny(clippy::all)]

//! age — Slate OS modern file encryption tool
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `age` (default) — encrypt/decrypt files
//! - `age-keygen` — generate age key pairs
//!
//! # This tool does not encrypt anything yet
//!
//! Only argument parsing, `--help` and `--version` are implemented. Every
//! path that would read a file, derive a key, or write a ciphertext refuses
//! and exits non-zero; see [`refuse`].
//!
//! That is a deliberate design choice rather than an oversight, and the
//! history is worth recording because the previous behaviour was actively
//! dangerous. Until 2026-09-02 this file:
//!
//! * shipped a **fixed X25519 identity** — the same public *and secret* key
//!   returned to every caller on every machine, as string literals in a
//!   public repository. Anything encrypted to that recipient was readable by
//!   anyone holding a checkout.
//! * printed `(binary encrypted data written to <path>)` and
//!   `age-keygen: key written to <path>`, then **exited 0**, without
//!   containing a single filesystem call. So
//!   `age -p secrets.txt -o secrets.age && rm secrets.txt` reported success,
//!   created no `secrets.age`, and deleted the only copy of the plaintext.
//! * carried a unit test that asserted the hardcoded public key started with
//!   `age1` — a property a constant satisfies forever, so the test certified
//!   the bug instead of catching it.
//!
//! The rule this file now follows, from lane C's audit in
//! `requests/c-b-2288-userspace-tools-report-success-for-work-they-never-did.md`:
//! **a tool that did not do the thing must not exit 0.** A stub that refuses
//! is harmless — every caller already knows what to do with a non-zero exit.
//! A stub that reports a plausible result and exits 0 is indistinguishable
//! from the real tool to a shell script, and that is what caused data loss.

use quoting::quoteaf_os;
use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

/// Exit status for an operation this tool does not perform.
///
/// Upstream `age` exits 1 on any error, so 1 is what a caller that already
/// understands `age` is prepared for. Deliberately *not* 127: that is the
/// shell's "command not found", which would be a false statement — the
/// binary is present and did run.
const EXIT_NOT_IMPLEMENTED: i32 = 1;

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    Encrypt,
    Decrypt,
}

/// Everything the command line asked for.
///
/// The underscore-prefixed fields are parsed and accepted so that the
/// accepted CLI surface stays stable and `--help` stays truthful, but
/// nothing consumes them: the operations they would configure are not
/// implemented. They are *not* dead code to be deleted — an implementation
/// needs every one of them, and silently rejecting `-a` today would be a
/// second, quieter lie about what this tool is.
#[derive(Clone, Debug)]
struct AgeOptions {
    action: Action,
    recipients: Vec<String>,
    _recipient_files: Vec<String>,
    identity_files: Vec<String>,
    passphrase: bool,
    _armor: bool,
    output: Option<String>,
    _files: Vec<String>,
}

impl Default for AgeOptions {
    fn default() -> Self {
        Self {
            action: Action::Encrypt,
            recipients: Vec::new(),
            _recipient_files: Vec::new(),
            identity_files: Vec::new(),
            passphrase: false,
            _armor: false,
            output: None,
            _files: Vec::new(),
        }
    }
}

// ── Refusal ───────────────────────────────────────────────────────────

/// End an operation this tool does not perform, loudly and non-zero.
///
/// Every path that would have done real work ends here. The message states
/// the two facts a caller can be harmed by getting wrong — that nothing was
/// read and nothing was written — and names the output file explicitly when
/// one was requested, because the file that does *not* exist is the one a
/// following `rm` or `mv` is about to be wrong about.
///
/// Goes to stderr, never stdout: stdout is where a real `age` puts
/// ciphertext, and a caller redirecting it to a file must get an empty file
/// rather than prose that would corrupt the ciphertext if this were ever
/// implemented.
fn refuse(op: &str, output: Option<&str>) -> i32 {
    eprintln!("age: {op}: not implemented on SlateOS");
    match output {
        Some(path) => eprintln!(
            "age: no data was read or written, and {} was NOT created",
            quoteaf_os(path)
        ),
        None => eprintln!("age: no data was read or written"),
    }
    EXIT_NOT_IMPLEMENTED
}

// ── age personality ──────────────────────────────────────────────────

fn run_age(args: Vec<String>) -> i32 {
    let mut opts = AgeOptions::default();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("Usage: age [--encrypt] [-r RECIPIENT] [-o OUTPUT] [INPUT]");
                println!("       age --decrypt [-i IDENTITY] [-o OUTPUT] [INPUT]");
                println!();
                println!("Modern file encryption tool.");
                println!();
                println!("Options:");
                println!("  -e, --encrypt          Encrypt (default)");
                println!("  -d, --decrypt          Decrypt");
                println!("  -r, --recipient REC    Recipient public key");
                println!("  -R, --recipients-file  File with recipient keys");
                println!("  -i, --identity FILE    Identity (private key) file");
                println!("  -p, --passphrase       Encrypt with passphrase");
                println!("  -a, --armor            ASCII armor output");
                println!("  -o, --output FILE      Output file");
                println!("  --version              Show version");
                return 0;
            }
            "--version" => {
                println!("age v0.1.0 (Slate OS)");
                return 0;
            }
            "-e" | "--encrypt" => opts.action = Action::Encrypt,
            "-d" | "--decrypt" => opts.action = Action::Decrypt,
            "-p" | "--passphrase" => opts.passphrase = true,
            "-a" | "--armor" => opts._armor = true,
            "-r" | "--recipient" => {
                i += 1;
                if i < args.len() {
                    opts.recipients.push(args[i].clone());
                }
            }
            "-R" | "--recipients-file" => {
                i += 1;
                if i < args.len() {
                    opts._recipient_files.push(args[i].clone());
                }
            }
            "-i" | "--identity" => {
                i += 1;
                if i < args.len() {
                    opts.identity_files.push(args[i].clone());
                }
            }
            "-o" | "--output" => {
                i += 1;
                if i < args.len() {
                    opts.output = Some(args[i].clone());
                }
            }
            s if !s.starts_with('-') => opts._files.push(s.to_string()),
            _ => {
                eprintln!("age: unknown option {}", quoteaf_os(&args[i]));
                return 1;
            }
        }
        i += 1;
    }

    match opts.action {
        Action::Encrypt => age_encrypt(&opts),
        Action::Decrypt => age_decrypt(&opts),
    }
}

// Argument *validation* below is real and is kept: rejecting `age file` for
// naming no recipient is a fact about argv, which this program does know.
// Only the work that follows it is missing, so the two failures stay
// distinguishable — a caller that fixes its arguments gets a different
// message rather than the same wall.

fn age_encrypt(opts: &AgeOptions) -> i32 {
    if opts.recipients.is_empty() && !opts.passphrase {
        eprintln!("age: error: no recipients specified (use -r or -p)");
        return 1;
    }
    refuse("encrypt", opts.output.as_deref())
}

fn age_decrypt(opts: &AgeOptions) -> i32 {
    if opts.identity_files.is_empty() && !opts.passphrase {
        eprintln!("age: error: no identity specified (use -i or -p)");
        return 1;
    }
    refuse("decrypt", opts.output.as_deref())
}

// ── age-keygen personality ───────────────────────────────────────────

fn run_age_keygen(args: Vec<String>) -> i32 {
    let mut output: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("Usage: age-keygen [-o FILE]");
                println!();
                println!("Generate an age X25519 identity (key pair).");
                println!();
                println!("Options:");
                println!("  -o FILE    Write key to FILE instead of stdout");
                println!("  --version  Show version");
                return 0;
            }
            "--version" => {
                println!("age-keygen v0.1.0 (Slate OS)");
                return 0;
            }
            "-o" | "--output" => {
                i += 1;
                if i < args.len() {
                    output = Some(args[i].clone());
                }
            }
            _ => {}
        }
        i += 1;
    }

    // No key material is emitted, on stdout or anywhere else. The previous
    // implementation printed one fixed identity — secret half included — to
    // every caller on every machine. Emitting a *random-looking* placeholder
    // instead would be worse still: it would be indistinguishable from a real
    // key until the day someone tried to decrypt with it.
    refuse("keygen", output.as_deref())
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("age");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog_name.as_str() {
        "age-keygen" => run_age_keygen(rest),
        _ => run_age(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_string()).collect()
    }

    /// Replaces a test that asserted the hardcoded public key began with
    /// `age1`. That assertion could never fail while the constant existed,
    /// so it certified the defect rather than catching it; the honest test
    /// for a stub is that the stub refuses.
    #[test]
    fn keygen_refuses_instead_of_returning_a_constant() {
        assert_ne!(run_age_keygen(argv(&[])), 0);
        assert_ne!(run_age_keygen(argv(&["-o", "/tmp/age.key"])), 0);
    }

    /// The property lane C's audit actually asks for: a tool that did not do
    /// the thing must not exit 0. Every argument shape that is *valid* — so
    /// would have reached real work — must still fail.
    #[test]
    fn every_operation_refuses_with_a_non_zero_status() {
        assert_ne!(run_age(argv(&["-p", "secrets.txt"])), 0);
        assert_ne!(run_age(argv(&["-p", "-o", "out.age", "secrets.txt"])), 0);
        assert_ne!(run_age(argv(&["-r", "age1example", "secrets.txt"])), 0);
        assert_ne!(run_age(argv(&["-a", "-r", "age1example", "in"])), 0);
        assert_ne!(run_age(argv(&["-d", "-i", "key.txt", "secrets.age"])), 0);
        assert_ne!(run_age(argv(&["-d", "-p", "secrets.age"])), 0);
    }

    /// `--help` and `--version` are reports about the program itself, which
    /// the program does know, so they stay honest and stay zero. If these
    /// ever start failing the tool has become useless rather than safe.
    #[test]
    fn help_and_version_still_succeed() {
        assert_eq!(run_age(argv(&["--help"])), 0);
        assert_eq!(run_age(argv(&["-h"])), 0);
        assert_eq!(run_age(argv(&["--version"])), 0);
        assert_eq!(run_age_keygen(argv(&["--help"])), 0);
        assert_eq!(run_age_keygen(argv(&["--version"])), 0);
    }

    /// Argument validation is implemented, so it must stay distinguishable
    /// from the unimplemented work rather than collapsing into one refusal.
    #[test]
    fn missing_recipient_and_identity_are_still_diagnosed() {
        assert_ne!(run_age(argv(&["secrets.txt"])), 0);
        assert_ne!(run_age(argv(&["-d", "secrets.age"])), 0);
        assert_ne!(run_age(argv(&["--bogus"])), 0);
    }

    /// Guards the security half of the defect: this file once shipped a
    /// fixed X25519 identity, secret half included, as a string literal.
    /// The needle is assembled with `concat!` so that this test's own source
    /// does not trip it.
    #[test]
    fn no_secret_key_material_is_compiled_in() {
        let src = include_str!("main.rs");
        assert!(
            !src.contains(concat!("AGE-SECRET-", "KEY-")),
            "a secret-key literal is back in age/src/main.rs; a committed \
             private key is readable by anyone with a checkout"
        );
        assert!(
            !src.contains(concat!("age1ql3z", "7hjy")),
            "the previously-shipped fixed recipient key is back in \
             age/src/main.rs"
        );
    }

    #[test]
    fn test_default_options() {
        let opts = AgeOptions::default();
        assert_eq!(opts.action, Action::Encrypt);
        assert!(!opts.passphrase);
        assert!(!opts._armor);
    }
}
