// Oracle for `interpret_echo_escapes` in `kernel/src/kshell.rs`.
//
//   rustc -O -o /tmp/echo-escapes-oracle scripts/echo-escapes-oracle.rs \
//       && /tmp/echo-escapes-oracle
//
// The function was rewritten from byte iteration to char iteration to fix a
// re-encoding bug (`byte as char` mapped 0x80..=0xFF to U+0080..=U+00FF, which
// `String::push` then wrote back out as *two* UTF-8 bytes, so `echo -e "café"`
// printed "cafÃ©").
//
// A bug fix that changes the iteration model of a parser carries an obvious
// risk: that it also, silently, changes behaviour for the inputs that were
// already correct. Those inputs are pure ASCII, they are the overwhelming
// majority of real `echo -e` use, and *every* pre-existing test of this
// function was ASCII -- so a regression there would have been invisible in
// exactly the same way the original bug was.
//
// So this checks the two implementations against each other exhaustively over
// ASCII rather than over a handful of hand-picked cases. The old version is
// reproduced here verbatim (it no longer exists in the tree) precisely so the
// comparison is against what actually shipped, not against a description of
// it. It lives on the host because the kernel is `no_std`; the in-tree
// `kshell::self_test` asserts the resulting behaviour, and this proves the
// behaviour it asserts is the pre-existing one.

/// The implementation as it stood before the fix: byte iteration, with the
/// `bytes[i] as char` copy that caused the corruption.
fn old(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'\\' && i.saturating_add(1) < len {
            let next = bytes[i.saturating_add(1)];
            match next {
                b'n' => {
                    result.push('\n');
                    i = i.saturating_add(2);
                }
                b't' => {
                    result.push('\t');
                    i = i.saturating_add(2);
                }
                b'r' => {
                    result.push('\r');
                    i = i.saturating_add(2);
                }
                b'\\' => {
                    result.push('\\');
                    i = i.saturating_add(2);
                }
                b'0' => {
                    result.push('\0');
                    i = i.saturating_add(2);
                }
                b'a' => {
                    result.push('\x07');
                    i = i.saturating_add(2);
                }
                b'b' => {
                    result.push('\x08');
                    i = i.saturating_add(2);
                }
                _ => {
                    result.push('\\');
                    i = i.saturating_add(1);
                }
            }
        } else {
            result.push(bytes[i] as char);
            i = i.saturating_add(1);
        }
    }
    result
}

/// The implementation as it now stands in `kshell.rs`: char iteration.
fn new(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '\\' {
            result.push(c);
            continue;
        }
        let escaped = match chars.peek() {
            Some('n') => '\n',
            Some('t') => '\t',
            Some('r') => '\r',
            Some('\\') => '\\',
            Some('0') => '\0',
            Some('a') => '\x07',
            Some('b') => '\x08',
            _ => {
                result.push('\\');
                continue;
            }
        };
        result.push(escaped);
        chars.next();
    }
    result
}

fn main() {
    // ---- 1. Exhaustive equivalence over ASCII -----------------------------
    //
    // Every string of length 0..=3 over an alphabet chosen to contain each
    // structurally distinct ASCII character class: the escape lead-in, every
    // recognised escape letter, an unrecognised letter, and an ordinary one.
    // Length 3 is enough to reach every state the parser has (an escape whose
    // continuation is itself an escape, a trailing backslash after a complete
    // escape, and so on).
    let alphabet: Vec<char> = vec!['\\', 'n', 't', 'r', '0', 'a', 'b', 'z', 'x'];
    let mut checked = 0u64;
    let mut mismatches = 0u64;

    for len in 0..=3usize {
        let mut idx = vec![0usize; len];
        loop {
            let s: String = idx.iter().map(|&i| alphabet[i]).collect();
            let (o, n) = (old(&s), new(&s));
            if o != n {
                if mismatches < 20 {
                    println!("MISMATCH on {s:?}: old={o:?} new={n:?}");
                }
                mismatches += 1;
            }
            checked += 1;

            if len == 0 {
                break;
            }
            let mut p = len;
            loop {
                if p == 0 {
                    break;
                }
                p -= 1;
                idx[p] += 1;
                if idx[p] < alphabet.len() {
                    break;
                }
                idx[p] = 0;
                if p == 0 {
                    break;
                }
            }
            if idx.iter().all(|&i| i == 0) {
                break;
            }
        }
    }

    // Also every single ASCII byte on its own, and each preceded by a
    // backslash -- this covers the escape table exhaustively rather than
    // relying on the alphabet above to have named every case.
    for b in 0u8..=127 {
        let c = b as char;
        for s in [format!("{c}"), format!("\\{c}"), format!("x{c}y")] {
            let (o, n) = (old(&s), new(&s));
            if o != n {
                if mismatches < 20 {
                    println!("MISMATCH on {s:?}: old={o:?} new={n:?}");
                }
                mismatches += 1;
            }
            checked += 1;
        }
    }

    println!("ASCII equivalence: {checked} inputs checked, {mismatches} mismatches");

    // ---- 2. The non-ASCII cases, where they are *supposed* to differ ------
    println!();
    println!("Non-ASCII (old is the bug, new is the fix):");
    for s in ["café", "é", "→", "🦀", "é\\né", "🦀\\t🦀"] {
        let (o, n) = (old(s), new(s));
        println!(
            "  {:10} old={:?} ({} bytes)   new={:?} ({} bytes)   input was {} bytes",
            s,
            o,
            o.len(),
            n,
            n.len(),
            s.len()
        );
        assert_eq!(n.len(), s.len().min(n.len()), "sanity");
    }

    // The property the fix guarantees: with no escapes present, the output is
    // byte-identical to the input. The old version violated this for every
    // non-ASCII input.
    println!();
    let mut old_violations = 0;
    for s in ["café", "é", "→", "🦀", "naïve", "日本語"] {
        assert_eq!(new(s), s, "new() must be the identity on escape-free text");
        if old(s) != s {
            old_violations += 1;
        }
    }
    println!(
        "identity-on-escape-free-text: new() holds for all 6 samples; old() violated it for {old_violations}/6"
    );

    if mismatches == 0 {
        println!();
        println!("RESULT: the rewrite is behaviour-preserving on ASCII and fixes non-ASCII.");
    } else {
        println!();
        println!("RESULT: FAILED -- {mismatches} ASCII behaviour changes.");
        std::process::exit(1);
    }
}
