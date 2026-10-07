//! `chpasswd`'s unit tests: the command line, the settings, the clock, the
//! line reader and whole runs over a scratch database.
//!
//! The differential half -- the same runs against Ubuntu's `chpasswd` -- is
//! `scripts/chpasswd-diff.sh`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;
use scratchdir::ScratchDir;

/// The words after the program name, as `args_os` would deliver them.
fn words(parts: &[&str]) -> Vec<OsString> {
    parts.iter().map(OsString::from).collect()
}

fn parsed(parts: &[&str]) -> Result<Config, Stop> {
    parse(&words(parts), b"chpasswd", b"chpasswd")
}

// ---- the command line -------------------------------------------------------

/// What Ubuntu 24.04's `chpasswd --help` prints, byte for byte.
const UBUNTU_HELP: &str = "Usage: chpasswd [options]

Options:
  -c, --crypt-method METHOD     the crypt method (one of NONE DES MD5 SHA256 SHA512 YESCRYPT)
  -e, --encrypted               supplied passwords are encrypted
  -h, --help                    display this help message and exit
  -m, --md5                     encrypt the clear text password using
                                the MD5 algorithm
  -R, --root CHROOT_DIR         directory to chroot into
  -s, --sha-rounds              number of rounds for the SHA, BCRYPT
                                or YESCRYPT crypt algorithms

";

#[test]
fn the_help_is_ubuntus() {
    assert_eq!(usage_text(b"chpasswd"), UBUNTU_HELP);
    assert_eq!(parsed(&["-h"]), Err(Stop::Help));
    assert_eq!(parsed(&["--help"]), Err(Stop::Help));
    assert_eq!(parsed(&["--he"]), Err(Stop::Help));
}

#[test]
fn a_getopt_error_names_argv0_whole_and_the_rest_name_its_last_part() {
    let got = parse(&words(&["-x"]), b"/usr/sbin/chpasswd", b"chpasswd");
    assert_eq!(
        got,
        Err(Stop::Usage(
            "/usr/sbin/chpasswd: invalid option -- 'x'".to_string()
        ))
    );
    let got = parse(
        &words(&["-c", "sha512"]),
        b"/usr/sbin/chpasswd",
        b"chpasswd",
    );
    assert_eq!(
        got,
        Err(Stop::Usage(
            "chpasswd: unsupported crypt method: sha512".to_string()
        ))
    );
    assert_eq!(basename(b"/usr/sbin/chpasswd"), b"chpasswd");
    assert_eq!(basename(b"chpasswd"), b"chpasswd");
    assert_eq!(basename(b"/usr/sbin/"), b"");
}

#[test]
fn the_flags_upstream_refuses_together_are_refused() {
    assert_eq!(
        parsed(&["-s", "9000"]),
        Err(Stop::Usage(
            "chpasswd: -s flag is only allowed with the -c flag".to_string()
        ))
    );
    for combo in [
        &["-e", "-m"][..],
        &["-e", "-c", "MD5"],
        &["-m", "-c", "SHA512"],
    ] {
        assert_eq!(
            parsed(combo),
            Err(Stop::Usage(
                "chpasswd: the -c, -e, and -m flags are exclusive".to_string()
            )),
            "{combo:?}"
        );
    }
    // bcrypt is not in Ubuntu's build, and the comparison is exact.
    for method in ["BCRYPT", "sha512", "Md5", ""] {
        assert!(
            matches!(parsed(&["-c", method]), Err(Stop::Usage(m)) if m.contains("unsupported crypt method")),
            "{method:?}"
        );
    }
    for method in METHODS {
        assert!(parsed(&["-c", method]).is_ok(), "{method}");
    }
}

/// `-s` is read against the `-c` before it, and only then.
#[test]
fn rounds_count_only_after_their_method() {
    let cfg = parsed(&["-c", "SHA512", "-s", "9000"]).unwrap();
    assert_eq!(cfg.sha_rounds, 9000);
    let cfg = parsed(&["-s", "9000", "-c", "SHA512"]).unwrap();
    assert_eq!(
        cfg.sha_rounds, SHA_ROUNDS_DEFAULT,
        "an -s before -c is not read"
    );
    assert!(cfg.sflg);
    // Nor refused, since it is not read.
    assert!(parsed(&["-s", "banana", "-c", "SHA512"]).is_ok());
    assert_eq!(
        parsed(&["-c", "SHA512", "-s", "banana"]),
        Err(Stop::Usage(
            "chpasswd: invalid numeric argument 'banana'".to_string()
        ))
    );
    let cfg = parsed(&["-c", "YESCRYPT", "-s", "7"]).unwrap();
    assert_eq!(cfg.yescrypt_cost, 7);
    // Read for no method it applies to: taken, and never looked at.
    assert!(parsed(&["-c", "MD5", "-s", "banana"]).is_ok());
}

#[test]
fn getlong_is_strtol_in_base_0_with_nothing_left_over() {
    assert_eq!(getlong(b"10"), Some(10));
    assert_eq!(getlong(b"0x10"), Some(16));
    assert_eq!(getlong(b"0X1f"), Some(31));
    assert_eq!(getlong(b"010"), Some(8));
    assert_eq!(getlong(b"0"), Some(0));
    assert_eq!(getlong(b"  12"), Some(12));
    assert_eq!(getlong(b"+5"), Some(5));
    assert_eq!(getlong(b"-5"), Some(-5));
    assert_eq!(getlong(b"-9223372036854775808"), Some(i64::MIN));
    for refused in [
        &b""[..],
        b"08",
        b"0x",
        b"0xg",
        b"12 ",
        b"1e3",
        b"x",
        b"9223372036854775808",
    ] {
        assert_eq!(
            getlong(refused),
            None,
            "{:?}",
            String::from_utf8_lossy(refused)
        );
    }
}

#[test]
fn root_is_found_by_its_own_scan_of_every_word() {
    let scan = |parts: &[&str]| root_flag(&words(parts), b"chpasswd");
    assert_eq!(scan(&["chpasswd"]), Ok(None));
    assert_eq!(scan(&["chpasswd", "-R", "/x"]), Ok(Some(b"/x".to_vec())));
    assert_eq!(
        scan(&["chpasswd", "--root", "/x"]),
        Ok(Some(b"/x".to_vec()))
    );
    assert_eq!(scan(&["chpasswd", "--root=/x"]), Ok(Some(b"/x".to_vec())));
    // Spellings getopt takes and the scan does not.
    assert_eq!(scan(&["chpasswd", "--ro", "/x"]), Ok(None));
    assert_eq!(scan(&["chpasswd", "-R/x"]), Ok(None));
    // And one after `--`, which getopt would not take.
    assert_eq!(
        scan(&["chpasswd", "--", "-R", "/x"]),
        Ok(Some(b"/x".to_vec()))
    );
    assert_eq!(
        scan(&["chpasswd", "-R", "/x", "--root=/y"]),
        Err(RootFailure {
            message: "chpasswd: multiple --root options".to_string(),
            status: E_BAD_ARG,
        })
    );
    assert_eq!(
        scan(&["chpasswd", "-e", "-R"]),
        Err(RootFailure {
            message: "chpasswd: option '-R' requires an argument".to_string(),
            status: E_BAD_ARG,
        })
    );
}

#[cfg(unix)]
#[test]
fn a_relative_or_missing_root_is_refused_with_upstreams_words() {
    assert_eq!(
        change_root(b"relative", b"chpasswd"),
        Err(RootFailure {
            message: "chpasswd: invalid chroot path 'relative', only absolute paths are supported."
                .to_string(),
            status: E_BAD_ARG,
        })
    );
    assert_eq!(
        change_root(b"/no/such/lane-b-root", b"chpasswd"),
        Err(RootFailure {
            message: "chpasswd: cannot access chroot directory /no/such/lane-b-root: \
                      No such file or directory"
                .to_string(),
            status: E_BAD_ARG,
        })
    );
}

// ---- the settings -----------------------------------------------------------

fn request_for(parts: &[&str]) -> Option<SettingRequest> {
    setting_request(&parsed(parts).unwrap())
}

#[test]
fn each_method_asks_crypt_gensalt_what_shadow_utils_asks_it() {
    assert_eq!(request_for(&["-e"]), None);
    assert_eq!(request_for(&["-c", "NONE"]), None);
    let md5 = request_for(&["-m"]).unwrap();
    assert_eq!(
        (md5.prefix.as_slice(), md5.count, md5.nrbytes),
        (&b"$1$"[..], 0, 9)
    );
    assert_eq!(request_for(&["-c", "MD5"]), Some(md5));
    let sha = request_for(&["-c", "SHA512"]).unwrap();
    assert_eq!(
        (sha.prefix.as_slice(), sha.count, sha.nrbytes),
        (&b"$6$"[..], 5000, 15)
    );
    // No `-c`: the method new passwords get, at upstream's default cost.
    assert_eq!(request_for(&[]), Some(sha));
    let sha256 = request_for(&["-c", "SHA256", "-s", "9000"]).unwrap();
    assert_eq!(sha256.prefix, b"$5$rounds=9000$");
    assert_eq!(sha256.count, 9000);
    let yes = request_for(&["-c", "YESCRYPT"]).unwrap();
    assert_eq!(
        (yes.prefix.as_slice(), yes.count, yes.nrbytes),
        (&b"$y$j9T$"[..], 5, 16)
    );
    let des = request_for(&["-c", "DES"]).unwrap();
    assert_eq!(des.prefix, vec![b'.'; 99]);
    assert_eq!((des.count, des.nrbytes), (0, 2));
}

#[test]
fn rounds_and_cost_are_clamped_and_zero_means_the_default() {
    assert_eq!(sha_rounds(None), 5000);
    assert_eq!(sha_rounds(Some(0)), 5000);
    assert_eq!(sha_rounds(Some(10)), 1000);
    assert_eq!(sha_rounds(Some(-4)), 999_999_999);
    assert_eq!(sha_rounds(Some(2_000_000_000)), 999_999_999);
    // `-s 4294967297` is read as the int 1, then clamped.
    let request = request_for(&["-c", "SHA512", "-s", "4294967297"]).unwrap();
    assert_eq!(request.prefix, b"$6$rounds=1000$");
    let request = request_for(&["-c", "SHA512", "-s", "0"]).unwrap();
    assert_eq!(request.prefix, b"$6$");
    // A negative count is a huge `unsigned long` upstream, so the most
    // rounds -- measured: Ubuntu's `chpasswd -c SHA512 -s -5` was still
    // hashing a minute later. `-s 3000000000` is a negative `int`.
    let request = request_for(&["-c", "SHA512", "-s", "-5"]).unwrap();
    assert_eq!(request.prefix, b"$6$rounds=999999999$");
    let request = request_for(&["-c", "SHA512", "-s", "3000000000"]).unwrap();
    assert_eq!(request.prefix, b"$6$rounds=999999999$");
    assert_eq!(yescrypt_cost(Some(0)), 5);
    assert_eq!(yescrypt_cost(Some(99)), 11);
    assert_eq!(yescrypt_cost(Some(-1)), 11);
    assert_eq!(&yescrypt_cost_text(1), b"j75$");
    assert_eq!(&yescrypt_cost_text(2), b"j85$");
    assert_eq!(&yescrypt_cost_text(3), b"j7T$");
    assert_eq!(&yescrypt_cost_text(5), b"j9T$");
    assert_eq!(&yescrypt_cost_text(6), b"jAT$");
    assert_eq!(&yescrypt_cost_text(11), b"jFT$");
}

/// Given its bytes, a setting is a pure function of them: as long as
/// libxcrypt's own, method by method.
#[test]
fn a_setting_is_as_long_as_libxcrypts() {
    let made = |parts: &[&str]| {
        let request = request_for(parts).unwrap();
        let random = vec![0x5a_u8; request.nrbytes];
        String::from_utf8(gensalt(&request, &random).unwrap()).unwrap()
    };
    let sha = made(&["-c", "SHA512"]);
    assert!(sha.starts_with("$6$") && sha.len() == 3 + 16, "{sha}");
    let rounds = made(&["-c", "SHA512", "-s", "9000"]);
    assert!(
        rounds.starts_with("$6$rounds=9000$") && rounds.len() == 15 + 16,
        "{rounds}"
    );
    let yes = made(&["-c", "YESCRYPT"]);
    assert!(yes.starts_with("$y$j9T$") && yes.len() == 7 + 22, "{yes}");
    let md5 = made(&["-c", "MD5"]);
    assert!(md5.starts_with("$1$") && md5.len() == 3 + 8, "{md5}");
    let des = made(&["-c", "DES"]);
    assert_eq!(des.len(), 2, "{des}");
}

#[test]
fn a_new_hash_verifies_and_a_source_that_fails_ends_the_run() {
    let request = request_for(&["-c", "MD5"]).unwrap();
    let mut counter = 0u8;
    let mut random = |out: &mut [u8]| {
        for b in out.iter_mut() {
            counter = counter.wrapping_add(37);
            *b = counter;
        }
        true
    };
    let hash = hash_new(b"s3cret", &request, &mut random).unwrap();
    assert!(posix::crypt::verify(b"s3cret", &hash));
    assert!(!posix::crypt::verify(b"S3cret", &hash));
    let mut broken = |_: &mut [u8]| false;
    assert_eq!(
        hash_new(b"s3cret", &request, &mut broken),
        Err(
            "Unable to generate a salt from setting \"$1$\", check your settings in \
             ENCRYPT_METHOD and the corresponding configuration for your selected hash method."
                .to_string()
        )
    );
}

// ---- the clock --------------------------------------------------------------

/// `gettime`, and what it said on the way.
fn clock(now: u64, var: Option<&str>) -> (u64, String) {
    let mut err = Vec::new();
    let got = gettime(now, var.map(str::as_bytes), &mut err);
    (got, String::from_utf8(err).unwrap())
}

#[test]
fn source_date_epoch_is_taken_when_it_is_a_time_no_later_than_now() {
    assert_eq!(clock(500, None), (500, String::new()));
    assert_eq!(clock(500, Some("100")), (100, String::new()));
    assert_eq!(clock(500, Some("500")), (500, String::new()));
    assert_eq!(clock(500, Some(" +7")), (7, String::new()));
    let warned = |var: &str, said: &str| {
        assert_eq!(
            clock(500, Some(var)),
            (
                500,
                format!("Environment variable $SOURCE_DATE_EPOCH: {said}\n")
            ),
            "{var:?}"
        );
    };
    warned("abc", "No digits were found: abc");
    warned("", "No digits were found: ");
    warned("12x", "Trailing garbage: x");
    warned(
        "99999999999999999999",
        "strtoull: Numerical result out of range",
    );
    warned(
        "600",
        "value must be smaller than or equal to the current time (500) but was found to be: 600",
    );
    // `strtoull` negates modulo 2^64.
    warned(
        "-1",
        "value must be smaller than or equal to the current time (500) but was found to be: \
         18446744073709551615",
    );
}

// ---- whole runs ---------------------------------------------------------------

/// A database holding `alice` and `bob`, `bob` locked, each with a `$6$`
/// entry for the password `old`.
fn database(scratch: &ScratchDir) -> std::path::PathBuf {
    let path = scratch.path("users.yaml");
    let mut db = UserDb::new();
    for (uid, name, locked) in [(1000, "alice", false), (1001, "bob", true)] {
        let mut record = userdb::Record::new();
        record.set_uid(uid);
        record.set("username", name);
        record.set_password_with_salt("old", "saltsalt").unwrap();
        if locked {
            record.set_locked(true);
        }
        db.push(record);
    }
    db.save(&path).unwrap();
    path
}

/// The stored entry for `name`.
fn entry(path: &Path, name: &str) -> String {
    UserDb::load(path)
        .unwrap()
        .find(name)
        .unwrap()
        .get(userdb::field::PASSWORD_HASH)
        .unwrap()
}

/// One run of the program over `path`: its status and its standard error.
fn run_over(path: &Path, parts: &[&str], input: &str, epoch: Option<&str>) -> (i32, String) {
    let cfg = parsed(parts).expect("a command line the test means to run");
    let mut err = Vec::new();
    let mut counter = 0u8;
    let mut random = |out: &mut [u8]| {
        for b in out.iter_mut() {
            counter = counter.wrapping_add(91);
            *b = counter;
        }
        true
    };
    let mut world = World {
        db_path: path,
        now: 20_000 * 86_400 + 5,
        source_date_epoch: epoch.map(|e| e.as_bytes().to_vec()),
        random: &mut random,
    };
    let status = run(&cfg, b"chpasswd", input.as_bytes(), &mut err, &mut world);
    (status, String::from_utf8(err).unwrap())
}

#[test]
fn every_line_changes_its_account_and_the_flat_files_follow() {
    let scratch = ScratchDir::new("chpasswd-run");
    let path = database(&scratch);
    let (status, err) = run_over(&path, &["-c", "SHA512"], "alice:new-a\nbob:new-b\n", None);
    // bob is locked, and stays so: deliberate difference 5.
    assert_eq!(
        (status, err.as_str()),
        (
            0,
            "chpasswd: note: `bob' is still locked and will refuse this password; \
             run `passwd -u' to unlock it\n"
        )
    );
    let alice = entry(&path, "alice");
    assert!(alice.starts_with("$6$"), "{alice}");
    assert!(posix::crypt::verify(b"new-a", alice.as_bytes()));
    assert!(posix::crypt::verify(
        b"new-b",
        entry(&path, "bob").as_bytes()
    ));
    // A fresh salt each: deliberate difference 4.
    assert_ne!(alice.get(..19), entry(&path, "bob").get(..19));
    // The day it changed, from the clock.
    let db = UserDb::load(&path).unwrap();
    assert_eq!(db.find("alice").unwrap().aging().changed, Some(20_000));
    assert!(db.find("bob").unwrap().is_locked());
    // And the generated shadow file carries it.
    let shadow = std::fs::read_to_string(scratch.path("shadow")).unwrap();
    assert!(
        shadow
            .lines()
            .any(|l| l.starts_with(&format!("alice:{alice}:20000:"))),
        "{shadow}"
    );
}

#[test]
fn one_bad_line_leaves_every_account_as_it_was() {
    let scratch = ScratchDir::new("chpasswd-bad");
    let path = database(&scratch);
    let before = entry(&path, "alice");
    let (status, err) = run_over(
        &path,
        &["-c", "MD5"],
        "alice:new\nnobody:x\nno colon here\n:empty-name\n",
        None,
    );
    assert_eq!(status, 1);
    assert_eq!(
        err,
        "chpasswd: line 2: user 'nobody' does not exist\n\
         chpasswd: line 3: missing new password\n\
         chpasswd: line 4: user '' does not exist\n\
         chpasswd: error detected, changes ignored\n"
    );
    assert_eq!(entry(&path, "alice"), before);
}

#[test]
fn an_encrypted_line_is_stored_as_it_came_unless_the_files_cannot_hold_it() {
    let scratch = ScratchDir::new("chpasswd-e");
    let path = database(&scratch);
    let ready = "$1$saltsalt$abcdefghijklmnopqrstuv";
    let (status, err) = run_over(&path, &["-e"], &format!("alice:{ready}\n"), None);
    assert_eq!((status, err.as_str()), (0, ""));
    assert_eq!(entry(&path, "alice"), ready);
    // A colon would shift every field after it in /etc/shadow, so the save
    // refuses it and nothing is written -- upstream's answer, measured.
    let (status, err) = run_over(&path, &["-e"], "alice:pa:ss\n", None);
    assert_eq!(
        (status, err),
        (
            1,
            format!(
                "chpasswd: failure while writing changes to {}\n",
                path.display()
            )
        )
    );
    assert_eq!(entry(&path, "alice"), ready);
    // A control byte is upstream's to write, and stored here; the generated
    // shadow shows `*` for a value that is not a hash (difference 9).
    let (status, err) = run_over(&path, &["-e"], "alice:tab\there\n", None);
    assert_eq!((status, err.as_str()), (0, ""));
    assert_eq!(entry(&path, "alice"), "tab\there");
    // A value the database cannot hold at all fails on its line.
    let mut err = Vec::new();
    let cfg = parsed(&["-e"]).unwrap();
    let mut random = |_: &mut [u8]| true;
    let mut world = World {
        db_path: &path,
        now: 0,
        source_date_epoch: None,
        random: &mut random,
    };
    let status = run(
        &cfg,
        b"chpasswd",
        &b"alice:\xff\n"[..],
        &mut err,
        &mut world,
    );
    assert_eq!(status, 1);
    assert_eq!(
        String::from_utf8(err).unwrap(),
        format!(
            "chpasswd: line 1: failed to prepare the new {} entry 'alice'\n\
             chpasswd: error detected, changes ignored\n",
            path.display()
        )
    );
    // `-c NONE` stores the password itself, as upstream does.
    let (status, _) = run_over(&path, &["-c", "NONE"], "alice:plain\n", None);
    assert_eq!(status, 0);
    assert_eq!(entry(&path, "alice"), "plain");
}

/// `fgets` into `BUFSIZ`: a line of `BUFSIZ - 1` bytes before its newline is
/// too long, one byte fewer is not, and the line after a long one is read.
#[test]
fn a_line_is_too_long_at_bufsiz_less_one_and_the_next_still_counts() {
    let scratch = ScratchDir::new("chpasswd-long");
    let path = database(&scratch);
    // `-e`, so the length is all that is tested: hashed, a password this
    // long is over libxcrypt's 512 bytes (see the test after this one).
    let fits = format!("alice:{}\n", "a".repeat(BUFSIZ - 2 - "alice:".len()));
    assert_eq!(fits.len(), BUFSIZ - 1);
    let (status, err) = run_over(&path, &["-e"], &fits, None);
    assert_eq!((status, err.as_str()), (0, ""));
    assert_eq!(entry(&path, "alice").len(), BUFSIZ - 2 - "alice:".len());
    let long = format!("alice:{}\n", "a".repeat(BUFSIZ - 1 - "alice:".len()));
    let (status, err) = run_over(&path, &["-e"], &format!("{long}nobody:x\n"), None);
    assert_eq!(status, 1);
    assert_eq!(
        err,
        "chpasswd: line 1: line too long\n\
         chpasswd: line 2: user 'nobody' does not exist\n\
         chpasswd: error detected, changes ignored\n"
    );
    // The last line needs no newline.
    let (status, err) = run_over(&path, &["-c", "MD5"], "alice:end", None);
    assert_eq!((status, err.as_str()), (0, ""));
    assert!(posix::crypt::verify(
        b"end",
        entry(&path, "alice").as_bytes()
    ));
}

/// libxcrypt hashes a passphrase of fewer than 512 bytes, and answers a longer
/// one with a failure token, which upstream's `pw_encrypt` reads as a missing
/// method under `$` and stores as the password under DES.
#[test]
fn a_passphrase_of_512_bytes_fails_as_libxcrypt_makes_upstream_fail() {
    let scratch = ScratchDir::new("chpasswd-512");
    let path = database(&scratch);
    let at_most = format!("alice:{}\n", "p".repeat(511));
    let (status, err) = run_over(&path, &["-c", "MD5"], &at_most, None);
    assert_eq!((status, err.as_str()), (0, ""));
    let over = format!("alice:{}\n", "p".repeat(512));
    let before = entry(&path, "alice");
    let (status, err) = run_over(&path, &["-c", "MD5"], &over, None);
    assert_eq!(
        (status, err.as_str()),
        (1, "crypt method not supported by libcrypt? (MD5)\n")
    );
    assert_eq!(entry(&path, "alice"), before);
    let (status, err) = run_over(&path, &["-c", "DES"], &over, None);
    assert_eq!((status, err.as_str()), (0, ""));
    assert_eq!(entry(&path, "alice"), "*0");
}

/// A NUL ends the line as C reads it, so its newline is unseen: the line is
/// "too long", and the drain swallows the next line too.
#[test]
fn a_nul_makes_a_line_too_long_and_swallows_the_next() {
    let scratch = ScratchDir::new("chpasswd-nul");
    let path = database(&scratch);
    let (status, err) = run_over(
        &path,
        &["-c", "MD5"],
        "alice:a\0b\nnobody:swallowed\nbob:x\n",
        None,
    );
    assert_eq!(status, 1);
    assert_eq!(
        err,
        "chpasswd: line 1: line too long\nchpasswd: error detected, changes ignored\n"
    );
}

#[test]
fn the_day_is_source_date_epochs_and_day_0_disables_aging() {
    let scratch = ScratchDir::new("chpasswd-epoch");
    let path = database(&scratch);
    let (status, err) = run_over(&path, &["-c", "MD5"], "alice:x\n", Some("864000"));
    assert_eq!((status, err.as_str()), (0, ""));
    let db = UserDb::load(&path).unwrap();
    assert_eq!(db.find("alice").unwrap().aging().changed, Some(10));
    let (status, _) = run_over(&path, &["-c", "MD5"], "alice:x\n", Some("5"));
    assert_eq!(status, 0);
    let db = UserDb::load(&path).unwrap();
    assert_eq!(db.find("alice").unwrap().aging().changed, None);
    // A bad value is reported once per line that sets a password, and the
    // clock is used.
    let (status, err) = run_over(&path, &["-c", "MD5"], "alice:x\nalice:y\n", Some("soon"));
    assert_eq!(status, 0);
    assert_eq!(
        err,
        "Environment variable $SOURCE_DATE_EPOCH: No digits were found: soon\n\
         Environment variable $SOURCE_DATE_EPOCH: No digits were found: soon\n"
    );
}

#[test]
fn the_whole_program_answers_help_and_usage_as_upstream_does() {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let status = chpasswd(
        &words(&["chpasswd", "--help"]),
        &b""[..],
        &mut out,
        &mut err,
    );
    assert_eq!(status, 0);
    assert_eq!(String::from_utf8(out).unwrap(), UBUNTU_HELP);
    assert!(err.is_empty());

    let mut out = Vec::new();
    let mut err = Vec::new();
    let status = chpasswd(&words(&["chpasswd", "-x"]), &b""[..], &mut out, &mut err);
    assert_eq!(status, E_USAGE);
    assert!(out.is_empty());
    assert_eq!(
        String::from_utf8(err).unwrap(),
        format!("chpasswd: invalid option -- 'x'\n{UBUNTU_HELP}")
    );

    let mut out = Vec::new();
    let mut err = Vec::new();
    let status = chpasswd(
        &words(&["chpasswd", "-R", "/a", "-R", "/b"]),
        &b""[..],
        &mut out,
        &mut err,
    );
    assert_eq!(status, E_BAD_ARG);
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "chpasswd: multiple --root options\n"
    );
}
