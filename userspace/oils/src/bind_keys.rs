//! How readline spells a key sequence — the translation between the text a
//! `bind` command is written with and the bytes readline matches on, and back
//! again for the listings.
//!
//! Both directions are needed the moment the tables stop being constants. A
//! binding is *matched* on bytes, so that is what a live table has to hold: it
//! is the only spelling in which `"\C-y"`, `"\C-Y"` and `"\031"` are the one
//! binding they really are, and the only one that sorts the way readline lists
//! them. But a binding is *written* and *printed* in an escaped text form, and
//! the two are not the same text — readline prints the form it would have
//! chosen, not the one it was given. So `bind '"\ey": yank'` is answered by
//! `bind -q yank` with `\M-y`.
//!
//! This mirrors readline's `rl_translate_keyseq` (in) and `_rl_get_keyname`
//! (out). The round-trip is pinned against readline's own compiled-in tables:
//! every binding in [`crate::bind_tables`] decodes and prints back to exactly
//! the text it was captured as — see the tests at the foot of this file.
//!
//! [`Maps`] is the table itself, seeded from those constants and mutable
//! thereafter. Readline's keymaps are a *tree* — the emacs map's escape and
//! `\C-x` slots hold whole keymaps of their own, which `bind -m` names
//! `emacs-meta` and `emacs-ctlx` — and the names are two views of one thing:
//! binding `Q` in `emacs-meta` is binding `\M-Q` in `emacs`, and either name
//! shows the other's work (measured). Storing whole byte sequences in the three
//! *root* maps and treating the sub-keymaps as prefixed slices of them gets
//! that for free, and is the only arrangement in which it cannot drift.

/// Escape, which readline uses for two unrelated things: a key in its own
/// right, and the prefix that stands in for a meta modifier.
pub const ESC: u8 = 0x1b;
/// Delete, which readline names `\C-?` rather than by the letter its bit
/// pattern would give.
const RUBOUT: u8 = 0x7f;

/// The byte readline's `\C-` prefix produces, as its `CTRL()` macro does it:
/// the character uppercased and then stripped of its top three bits. That is
/// why `\C-a` and `\C-A` are the same key (1) and `\C-@` is NUL.
fn ctrl(c: u8) -> u8 {
    c.to_ascii_uppercase() & 0x1f
}

/// Decode one escape *body* — everything after the backslash, starting at
/// `i` — into the byte it stands for, and say how many bytes of `spec` it ate.
///
/// The set is readline's: the C escapes it shares with the shell, `\d` for
/// delete (which C spells nothing), `\e` for escape, an octal run of up to
/// three digits and a hex run of up to two. Anything else stands for itself,
/// so `\q` is `q` — readline does not diagnose an unknown escape.
fn one_escape(spec: &[u8], i: usize) -> (u8, usize) {
    // A backslash at the very end of the sequence is a byte like any other.
    let Some(&c) = spec.get(i) else {
        return (b'\\', 0);
    };
    match c {
        b'a' => (7, 1),
        b'b' => (8, 1),
        b'd' => (RUBOUT, 1),
        b'e' => (ESC, 1),
        b'f' => (12, 1),
        b'n' => (b'\n', 1),
        b'r' => (b'\r', 1),
        b't' => (b'\t', 1),
        b'v' => (11, 1),
        b'0'..=b'7' => {
            // Up to three octal digits in all, and the value wraps into a byte
            // rather than being refused — `\400` is `\000`.
            let mut v: u32 = u32::from(c - b'0');
            let mut n = 1usize;
            while n < 3 {
                match spec.get(i + n) {
                    Some(&d @ b'0'..=b'7') => {
                        v = v * 8 + u32::from(d - b'0');
                        n += 1;
                    }
                    _ => break,
                }
            }
            #[allow(clippy::cast_possible_truncation)]
            (v as u8, n)
        }
        b'x' => {
            // `\x` with no digit behind it is just an `x`; readline reads at
            // most two digits, so `\x41B` is `AB`.
            let mut v: u32 = 0;
            let mut n = 1usize;
            while n <= 2 {
                match spec.get(i + n).and_then(|d| (*d as char).to_digit(16)) {
                    Some(d) => {
                        v = v * 16 + d;
                        n += 1;
                    }
                    None => break,
                }
            }
            if n == 1 {
                (b'x', 1)
            } else {
                #[allow(clippy::cast_possible_truncation)]
                (v as u8, n)
            }
        }
        _ => (c, 1),
    }
}

/// Decode the text of a key sequence into the bytes readline matches on, as
/// `rl_translate_keyseq` does.
///
/// `spec` is the sequence alone — a caller that was handed `"…": command` has
/// already taken the quotes and the target off.
///
/// `\M-` is not a modifier bit here: readline turns it into a leading escape
/// byte, which is what makes `\ey` and `\M-y` the same two bytes and is why a
/// listing spells an escape-prefixed sequence with `\M-`. `\C-` *is* a bit
/// operation, and the two nest — `\C-\M-g` is escape followed by `\C-g`,
/// exactly like `\M-\C-g`.
#[must_use]
pub fn decode(spec: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(spec.len());
    let mut i = 0usize;
    while let Some(&c) = spec.get(i) {
        i += 1;
        if c != b'\\' {
            out.push(c);
            continue;
        }
        // `\C-` and `\M-` modify what follows rather than standing for a byte,
        // so they are read before the escape table.
        if matches!(spec.get(i), Some(b'C' | b'M')) && spec.get(i + 1) == Some(&b'-') {
            let modifier = spec.get(i).copied().unwrap_or(b'C');
            i += 2;
            if modifier == b'M' {
                // Meta is a prefix byte, and the rest of the sequence is read
                // as if it had been written on its own.
                out.push(ESC);
                continue;
            }
            // `\C-\M-x` — the two written the other way round, meaning the
            // same thing.
            if spec.get(i) == Some(&b'\\')
                && spec.get(i + 1) == Some(&b'M')
                && spec.get(i + 2) == Some(&b'-')
            {
                out.push(ESC);
                i += 3;
            }
            let Some(&k) = spec.get(i) else {
                // `\C-` with nothing to control: readline has nothing to
                // produce and neither has this.
                break;
            };
            i += 1;
            if k == b'?' {
                // The one control name that is not its letter's bit pattern.
                out.push(RUBOUT);
            } else if k == b'\\' {
                // The controlled character is itself written as an escape, as
                // in `\C-\\` for the byte 0x1c.
                let (b, used) = one_escape(spec, i);
                i += used;
                out.push(ctrl(b));
            } else {
                out.push(ctrl(k));
            }
            continue;
        }
        let (b, used) = one_escape(spec, i);
        i += used;
        out.push(b);
    }
    out
}

/// How a listing spells an escape byte that has something after it.
///
/// readline has two dumpers and they do not agree. The one behind `bind -p`,
/// `-P` and `-q` walks the keymap tree and names the escape sub-map by the
/// modifier it stands for; the one behind `-s`, `-S` and `-X` — which bash
/// also borrows for its `-x` bindings — writes the byte out as itself. So one
/// and the same binding is `\M-Q` to `bind -q` and `\eQ` to `bind -X`
/// (measured).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Meta {
    /// An escape with a key behind it is the meta prefix: `\M-`.
    Prefix,
    /// An escape is always `\e`, prefix or not.
    Literal,
}

/// Render a key sequence the way readline's listings spell it.
///
/// `is_prefix` says whether a *longer* sequence is also bound in the same
/// keymap. It has to be asked because readline keeps a bound prefix in the
/// slot its continuation map reserves for "and nothing further", and prints
/// that slot as a trailing `\000`: binding `\C-x` alone in the emacs map —
/// where `\C-x\C-e` and friends live — is listed as `\C-x\000`, and the escape
/// bound by itself in `vi-insert` is `\M-\000`. Under [`Meta::Prefix`] the same
/// rule decides the one place escape is not written `\M-`: it is the prefix
/// spelling everywhere except at the end of a sequence that is nobody's prefix,
/// where it is `\e`.
///
/// This also spells a *macro's text* and a `-x` binding's command, which are
/// not key sequences at all but go through the same dumper and so come out
/// escaped the same way.
#[must_use]
pub fn encode(seq: &[u8], is_prefix: bool, meta: Meta) -> Vec<u8> {
    let mut out = Vec::with_capacity(seq.len().saturating_mul(4));
    let last = seq.len().saturating_sub(1);
    for (i, &b) in seq.iter().enumerate() {
        match b {
            ESC if meta == Meta::Prefix && (i < last || is_prefix) => {
                out.extend_from_slice(b"\\M-");
            }
            ESC => out.extend_from_slice(b"\\e"),
            RUBOUT => out.extend_from_slice(b"\\C-?"),
            0x00..=0x1f => {
                out.extend_from_slice(b"\\C-");
                // The inverse of `ctrl`: put the top bits back and lowercase
                // the letters, which leaves `@[\]^_` alone. Only the backslash
                // needs escaping again on the way out.
                let c = (b | 0x40).to_ascii_lowercase();
                if c == b'\\' {
                    out.push(b'\\');
                }
                out.push(c);
            }
            // A byte with the top bit set has no name, so it is printed in
            // octal — always three digits, so it cannot run into a digit of
            // the sequence behind it.
            0x80..=0xff => out.extend_from_slice(format!("\\{b:03o}").as_bytes()),
            // The listing quotes the whole sequence, so these two have to be
            // escaped inside it.
            b'\\' | b'"' => {
                out.push(b'\\');
                out.push(b);
            }
            _ => out.push(b),
        }
    }
    if is_prefix {
        out.extend_from_slice(b"\\000");
    }
    out
}

/// Read one entry of a captured listing back into the sequence it names.
///
/// The trailing `\000` [`encode`] writes for a bound prefix is a property of
/// the keymap rather than of the binding, so it comes off here and is derived
/// again on the way out. The flag is returned only so a caller checking the
/// capture against itself can compare like with like.
#[must_use]
pub fn table_entry(text: &str) -> (Vec<u8>, bool) {
    match text.strip_suffix("\\000") {
        Some(head) => (decode(head.as_bytes()), true),
        None => (decode(text.as_bytes()), false),
    }
}

/// Order two key sequences as readline walks a keymap.
///
/// Byte order, except that a sequence which is a *prefix* of another comes
/// **after** it: readline keeps the prefix's own binding in the slot past the
/// end of the byte range (its `ANYOTHERKEY`), so `bind -q` answers
/// `"\C-t\000", "\C-y"` when `\C-t`, `\C-tZ` and `\C-y` are all bound — the
/// prefix sorts after its own continuations but still before the next key
/// (measured).
#[must_use]
pub fn cmp_seq(a: &[u8], b: &[u8]) -> core::cmp::Ordering {
    use core::cmp::Ordering;
    for (x, y) in a.iter().zip(b) {
        match x.cmp(y) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    // Whichever ran out first is the prefix, and the prefix is the greater.
    b.len().cmp(&a.len())
}

/// What a key sequence can be bound to. The three are one namespace: binding
/// any of them at a sequence replaces whatever was there, and `bind -r` takes
/// any of them away (measured) — but each is listed by its own option, and by
/// no other.
#[derive(Clone, PartialEq, Eq)]
pub enum Target {
    /// A readline function, listed by `-p`, `-P` and `-q`. Held as the
    /// representative of its alias group (see [`function`]) rather than as the
    /// name that was typed, because what a key is bound to is the *function*,
    /// and one function can answer to more than one name.
    Function(&'static str),
    /// A macro: the bytes readline pushes back as if typed. Listed by `-s` and
    /// `-S`, and decoded on the way in, so `"\C-y"` is one byte and not four.
    Macro(Vec<u8>),
    /// A shell command bound by `bind -x`, listed by `-X`. Kept exactly as
    /// written — it is a command, not a key sequence, and readline never
    /// decodes it.
    Command(Vec<u8>),
}

/// readline's live key tables.
///
/// Three roots, because readline has three keymaps that are not reachable from
/// each other: the emacs map, the vi movement map, and the vi insert map.
/// Everything `bind -m` else names is a slice of one of those — see the module
/// docs. Each root is kept sorted by [`cmp_seq`], which is both the order every
/// listing wants and a binary search for the mutations.
#[derive(Clone)]
pub struct Maps {
    emacs: Vec<(Vec<u8>, Target)>,
    vi: Vec<(Vec<u8>, Target)>,
    vi_insert: Vec<(Vec<u8>, Target)>,
}

/// One of the three roots [`Maps`] keeps.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Root {
    Emacs,
    Vi,
    ViInsert,
}

/// One live binding, as the keymap that was asked for spells it.
pub struct Entry<'a> {
    /// The sequence with the view's own prefix already taken off, so an
    /// `emacs-meta` entry reads `Q` where the `emacs` one reads `\M-Q`.
    pub seq: &'a [u8],
    /// Whether a longer binding extends it — [`encode`]'s `\000`.
    pub is_prefix: bool,
    pub target: &'a Target,
}

/// The escape that begins every `emacs-meta` sequence, as a prefix to match on.
const ESC_PREFIX: &[u8] = &[ESC];
/// `\C-x`, likewise for `emacs-ctlx`.
const CTLX_PREFIX: &[u8] = &[0x18];

/// Which root a keymap name is a view of, and the prefix its own sequences
/// carry inside that root.
///
/// The name is expected to be canonical — the first of the aliases
/// [`crate::bind_tables::KEYMAPS`] lists — because that is what `bind -m` has
/// already resolved it to. Anything else is the emacs map, which is readline's
/// default and so the right answer for the caller that passed no `-m` at all.
fn view(keymap: &str) -> (Root, &'static [u8]) {
    match keymap {
        "emacs-meta" => (Root::Emacs, ESC_PREFIX),
        "emacs-ctlx" => (Root::Emacs, CTLX_PREFIX),
        "vi" => (Root::Vi, &[]),
        "vi-insert" => (Root::ViInsert, &[]),
        _ => (Root::Emacs, &[]),
    }
}

impl Maps {
    /// The tables as readline compiles them in.
    ///
    /// Only the roots are read: the `emacs-meta` and `emacs-ctlx` captures are
    /// the same bindings written without their prefix (checked in the tests),
    /// so seeding from them as well would double every one of them.
    #[must_use]
    pub fn seeded() -> Self {
        let of = |canonical: &str| -> Vec<(Vec<u8>, Target)> {
            let mut v: Vec<(Vec<u8>, Target)> = crate::bind_tables::KEYMAPS
                .iter()
                .find(|m| m.names.first() == Some(&canonical))
                .map(|m| {
                    m.bindings
                        .iter()
                        .map(|(text, func)| {
                            let name = function(func.as_bytes()).unwrap_or(func);
                            (table_entry(text).0, Target::Function(name))
                        })
                        .collect()
                })
                .unwrap_or_default();
            // The capture is grouped by function, not by key; the live table is
            // the other way round.
            v.sort_by(|(a, _), (b, _)| cmp_seq(a, b));
            // An aliased function is captured once per name, and those are one
            // binding — the same key, now the same representative too.
            v.dedup_by(|a, b| a.0 == b.0);
            v
        };
        Self { emacs: of("emacs"), vi: of("vi"), vi_insert: of("vi-insert") }
    }

    fn root(&self, r: Root) -> &Vec<(Vec<u8>, Target)> {
        match r {
            Root::Emacs => &self.emacs,
            Root::Vi => &self.vi,
            Root::ViInsert => &self.vi_insert,
        }
    }

    fn root_mut(&mut self, r: Root) -> &mut Vec<(Vec<u8>, Target)> {
        match r {
            Root::Emacs => &mut self.emacs,
            Root::Vi => &mut self.vi,
            Root::ViInsert => &mut self.vi_insert,
        }
    }

    /// Everything bound in `keymap`, in the order its listings come out.
    ///
    /// `is_prefix` is read off the neighbour rather than searched for: under
    /// [`cmp_seq`] every continuation of a sequence sorts immediately before
    /// it, so the entry in front is the only one that can extend it.
    #[must_use]
    pub fn entries(&self, keymap: &str) -> Vec<Entry<'_>> {
        let (root, prefix) = view(keymap);
        let all = self.root(root);
        let mut out = Vec::new();
        for (i, (seq, target)) in all.iter().enumerate() {
            let Some(rest) = seq.strip_prefix(prefix) else { continue };
            // A view is named by a prefix that is itself a key, so the prefix
            // alone is not one of the view's own bindings.
            if rest.is_empty() && !prefix.is_empty() {
                continue;
            }
            let is_prefix = i
                .checked_sub(1)
                .and_then(|p| all.get(p))
                .is_some_and(|(before, _)| before.starts_with(seq));
            out.push(Entry { seq: rest, is_prefix, target });
        }
        out
    }

    /// Bind `seq` — as `keymap` spells it — replacing whatever was there.
    pub fn bind(&mut self, keymap: &str, seq: &[u8], target: Target) {
        let (root, prefix) = view(keymap);
        let mut full = prefix.to_vec();
        full.extend_from_slice(seq);
        let all = self.root_mut(root);
        match all.binary_search_by(|(s, _)| cmp_seq(s, &full)) {
            Ok(i) => {
                if let Some(slot) = all.get_mut(i) {
                    slot.1 = target;
                }
            }
            Err(i) => all.insert(i, (full, target)),
        }
    }

    /// Take away whatever `seq` is bound to in `keymap`, of any kind.
    pub fn unbind_seq(&mut self, keymap: &str, seq: &[u8]) {
        let (root, prefix) = view(keymap);
        let mut full = prefix.to_vec();
        full.extend_from_slice(seq);
        let all = self.root_mut(root);
        if let Ok(i) = all.binary_search_by(|(s, _)| cmp_seq(s, &full)) {
            all.remove(i);
        }
    }

    /// Take away every sequence in `keymap` that runs `func`, which is a
    /// representative from [`function`] — so unbinding by either of an aliased
    /// function's names takes away all of its keys, as readline's does.
    ///
    /// Sub-keymaps go with it: `bind -u yank` in the emacs map unbinds `\M-Q`
    /// as well as `\C-y` (measured), which falls out of storing whole
    /// sequences. A view is still only its own slice, so the same `-u` under
    /// `-m emacs-meta` leaves `\C-y` alone.
    pub fn unbind_function(&mut self, keymap: &str, func: &str) {
        let (root, prefix) = view(keymap);
        self.root_mut(root).retain(|(seq, target)| {
            !(seq.starts_with(prefix)
                && seq.len() > prefix.len()
                && matches!(target, Target::Function(f) if *f == func))
        });
    }
}

/// One name per readline *function*, so two names for the same function
/// compare equal.
///
/// readline's funmap holds some C functions under two names — `\M-.` is listed
/// by `bind -p` under both `yank-last-arg` and `insert-last-argument` — and a
/// listing walks names, so an aliased function is printed once for each. A
/// table keyed by key sequence has one entry there, not two, so the aliases
/// have to be recognised rather than stored.
///
/// The groups are read off the captured tables themselves: two names are the
/// same function exactly when some key sequence is captured under both. That
/// cannot see an alias pair that is bound nowhere, but neither can any listing
/// — both names print as unbound either way.
fn representatives() -> &'static std::collections::HashMap<&'static str, &'static str> {
    use std::collections::HashMap;
    static GROUPS: std::sync::OnceLock<HashMap<&'static str, &'static str>> =
        std::sync::OnceLock::new();
    GROUPS.get_or_init(|| {
        // `name -> the name that stands for its group`, following chains so a
        // three-way alias lands on one representative however it was linked.
        let mut of: HashMap<&'static str, &'static str> = HashMap::new();
        fn root<'a>(of: &HashMap<&'a str, &'a str>, mut n: &'a str) -> &'a str {
            while let Some(&up) = of.get(n) {
                if up == n {
                    break;
                }
                n = up;
            }
            n
        }
        for map in &crate::bind_tables::KEYMAPS {
            let mut seen: HashMap<Vec<u8>, &'static str> = HashMap::new();
            for (text, func) in map.bindings {
                let seq = table_entry(text).0;
                let Some(&other) = seen.get(&seq) else {
                    seen.insert(seq, func);
                    continue;
                };
                let (a, b) = (root(&of, func), root(&of, other));
                if a != b {
                    // The earlier name in readline's own list stands for the
                    // group, so the choice does not depend on capture order.
                    let (keep, drop) = if a < b { (a, b) } else { (b, a) };
                    of.insert(drop, keep);
                }
            }
        }
        crate::bind_tables::FUNCTION_NAMES
            .iter()
            .map(|&n| (n, root(&of, n)))
            .collect()
    })
}

/// The readline function of that name, if there is one, as the one name every
/// alias for it resolves to.
///
/// A name readline does not know is not an error anywhere a binding is made —
/// `bind '"\C-t": nosuchfn'` succeeds and binds nothing (measured) — so this
/// returning `None` is the whole of that check.
#[must_use]
pub fn function(name: &[u8]) -> Option<&'static str> {
    let name = core::str::from_utf8(name).ok()?;
    representatives().get(name).copied()
}

#[cfg(test)]
mod tests {
    use super::{Meta, Target, cmp_seq, decode, encode, table_entry as table_seq};
    use crate::bind_tables::KEYMAPS;

    #[test]
    fn every_readline_binding_survives_a_round_trip() {
        for map in &KEYMAPS {
            for (text, func) in map.bindings {
                let (seq, is_prefix) = table_seq(text);
                assert!(!seq.is_empty(), "{}: {text} decoded to nothing", func);
                let back = encode(&seq, is_prefix, Meta::Prefix);
                assert_eq!(
                    String::from_utf8_lossy(&back),
                    *text,
                    "{}/{func}: {text} -> {seq:?} -> {}",
                    map.names.first().copied().unwrap_or("?"),
                    String::from_utf8_lossy(&back)
                );
            }
        }
    }

    /// The `\000` marker is not an arbitrary label: it is exactly the
    /// sequences that another binding in the same keymap extends. Deriving it
    /// rather than reading it is what lets a *live* table print a prefix
    /// binding correctly after the tables have been changed.
    #[test]
    fn the_prefix_marker_is_derivable_from_the_keymap() {
        for map in &KEYMAPS {
            let seqs: Vec<Vec<u8>> = map.bindings.iter().map(|(t, _)| table_seq(t).0).collect();
            for ((text, func), seq) in map.bindings.iter().zip(&seqs) {
                let derived = seqs
                    .iter()
                    .any(|o| o.len() > seq.len() && o.starts_with(seq));
                assert_eq!(
                    String::from_utf8_lossy(&encode(seq, derived, Meta::Prefix)),
                    *text,
                    "{}/{func}",
                    map.names.first().copied().unwrap_or("?")
                );
            }
        }
    }

    /// readline lists a function's key sequences in the order its keymap walks
    /// them, which is [`cmp_seq`] — so a live table kept sorted that way lists
    /// in readline's order without having to remember the capture order.
    #[test]
    fn the_tables_are_already_in_keymap_order_within_each_function() {
        for map in &KEYMAPS {
            for (name, _) in map.bindings {
                let of_func: Vec<Vec<u8>> = map
                    .bindings
                    .iter()
                    .filter(|(_, f)| f == name)
                    .map(|(t, _)| table_seq(t).0)
                    .collect();
                let mut sorted = of_func.clone();
                sorted.sort_by(|a, b| cmp_seq(a, b));
                assert_eq!(of_func, sorted, "{name}");
            }
        }
    }

    /// A prefix sorts after everything that extends it, and nowhere else —
    /// which is what puts `\C-t\000` between `\C-tZ` and `\C-y`.
    #[test]
    fn a_bound_prefix_sorts_after_its_own_continuations() {
        let mut seqs: Vec<&[u8]> = vec![b"\x19", b"\x14", b"\x14Z", b"\x14A", b"\x1bb"];
        seqs.sort_by(|a, b| cmp_seq(a, b));
        assert_eq!(seqs, vec![&b"\x14A"[..], b"\x14Z", b"\x14", b"\x19", b"\x1bb"]);
    }

    #[test]
    fn the_ways_of_writing_one_key_all_decode_to_it() {
        // The same byte spelled five ways: control letter (either case), the
        // octal it is, the hex it is, and the escape readline prints.
        for spec in [r"\C-y", r"\C-Y", r"\031", r"\x19"] {
            assert_eq!(decode(spec.as_bytes()), vec![0x19], "{spec}");
        }
        // Meta is an escape prefix, so all three of these are two bytes.
        for spec in [r"\M-y", r"\ey", r"\033y"] {
            assert_eq!(decode(spec.as_bytes()), vec![0x1b, b'y'], "{spec}");
        }
        // The two modifiers nest either way round.
        for spec in [r"\M-\C-g", r"\C-\M-g"] {
            assert_eq!(decode(spec.as_bytes()), vec![0x1b, 0x07], "{spec}");
        }
        assert_eq!(decode(br"\C-?"), vec![0x7f]);
        assert_eq!(decode(br"\d"), vec![0x7f]);
        assert_eq!(decode(br"\C-@"), vec![0x00]);
        assert_eq!(decode(br"\C-\\"), vec![0x1c]);
        // An unknown escape is the letter itself, and a trailing backslash is
        // a backslash.
        assert_eq!(decode(br"\q"), vec![b'q']);
        assert_eq!(decode(br"a\"), vec![b'a', b'\\']);
        // `\x` with nothing behind it is an `x`.
        assert_eq!(decode(br"\xz"), vec![b'x', b'z']);
    }

    #[test]
    fn a_sequence_is_printed_the_way_readline_would_have_written_it() {
        let p = |seq: &[u8], pre: bool| encode(seq, pre, Meta::Prefix);
        assert_eq!(p(&[0x19], false), b"\\C-y");
        assert_eq!(p(&[0x1b, b'y'], false), b"\\M-y");
        assert_eq!(p(&[0x1b], false), b"\\e");
        assert_eq!(p(&[0x1b], true), b"\\M-\\000");
        assert_eq!(p(&[0x18], true), b"\\C-x\\000");
        assert_eq!(p(&[0x00], false), b"\\C-@");
        assert_eq!(p(&[0x1c], false), b"\\C-\\\\");
        assert_eq!(p(&[0x7f], false), b"\\C-?");
        assert_eq!(p(&[0xe6], false), b"\\346");
        assert_eq!(p(b"\"", false), b"\\\"");
        assert_eq!(p(b"\\", false), b"\\\\");
        assert_eq!(p(b"zq", false), b"zq");
    }

    /// The macro and `-x` dumper is the same escaping with one difference, and
    /// it is the one readline's own listings disagree on.
    #[test]
    fn the_macro_dumper_writes_escape_as_itself() {
        let l = |seq: &[u8]| encode(seq, false, Meta::Literal);
        assert_eq!(l(&[0x1b, b'Q']), b"\\eQ");
        assert_eq!(l(&[0x19, 0x1b, b'b']), b"\\C-y\\eb");
        assert_eq!(encode(&[0x14], true, Meta::Literal), b"\\C-t\\000");
        // A `-x` command is never decoded, so its backslashes are doubled by
        // the same pass that would have escaped a key sequence's.
        assert_eq!(l(br"printf %s\\n hi"), br"printf %s\\\\n hi");
    }

    /// The sub-keymaps are not tables of their own: `emacs-meta` is exactly the
    /// escape-prefixed part of `emacs` with the escape taken off, and
    /// `emacs-ctlx` the same for `\C-x` (checked against real bash, which
    /// reports 104 of each and no difference). Seeding only the roots and
    /// slicing for the rest is what keeps a change made through one name
    /// visible through the other.
    #[test]
    fn a_sub_keymap_is_a_slice_of_its_root_and_not_a_table_of_its_own() {
        let maps = super::Maps::seeded();
        for name in ["emacs", "emacs-meta", "emacs-ctlx", "vi", "vi-insert"] {
            let captured = KEYMAPS
                .iter()
                .find(|m| m.names.first() == Some(&name))
                .expect("every keymap is captured");
            let live: Vec<String> = maps
                .entries(name)
                .iter()
                .map(|e| String::from_utf8_lossy(&encode(e.seq, e.is_prefix, Meta::Prefix)).into_owned())
                .collect();
            let mut want: Vec<String> = captured
                .bindings
                .iter()
                .map(|(t, _)| (*t).to_string())
                .collect();
            want.sort_by(|a, b| cmp_seq(&table_seq(a).0, &table_seq(b).0));
            // An aliased function is captured once per name — `\M-.` is listed
            // as both `yank-last-arg` and `insert-last-argument` — but it is
            // one key and so one row of the live table.
            want.dedup();
            assert_eq!(live, want, "{name}");
        }
    }

    /// The two names really are one table underneath: what is bound through
    /// the child shows through the parent, prefix and all, and the reverse.
    #[test]
    fn binding_through_one_name_is_visible_through_the_other() {
        let mut maps = super::Maps::seeded();
        let yank = Target::Function(super::function(b"yank").expect("yank is a function"));
        maps.bind("emacs-meta", b"Q", yank.clone());
        let seqs: Vec<Vec<u8>> = maps
            .entries("emacs")
            .iter()
            .filter(|e| *e.target == yank)
            .map(|e| encode(e.seq, e.is_prefix, Meta::Prefix))
            .collect();
        assert_eq!(seqs, vec![b"\\C-y".to_vec(), b"\\M-Q".to_vec()]);

        // …and `-u` through the parent reaches into the child, while `-u`
        // through the child leaves the parent's own bindings alone.
        let mut child = maps.clone();
        child.unbind_function("emacs-meta", "yank");
        assert_eq!(child.entries("emacs").iter().filter(|e| *e.target == yank).count(), 1);
        maps.unbind_function("emacs", "yank");
        assert_eq!(maps.entries("emacs").iter().filter(|e| *e.target == yank).count(), 0);
    }

    /// A prefix that is bound in its own right keeps its binding, and gets the
    /// marker only for as long as something extends it.
    #[test]
    fn the_prefix_marker_comes_and_goes_with_the_continuation() {
        let mut maps = super::Maps::seeded();
        let printed = |m: &super::Maps| -> Vec<String> {
            m.entries("emacs")
                .iter()
                .filter(|e| matches!(e.target, Target::Macro(_)))
                .map(|e| String::from_utf8_lossy(&encode(e.seq, e.is_prefix, Meta::Literal)).into_owned())
                .collect()
        };
        maps.bind("emacs", &[0x14], Target::Macro(b"pfx".to_vec()));
        assert_eq!(printed(&maps), vec!["\\C-t"]);
        maps.bind("emacs", &[0x14, b'Z'], Target::Macro(b"m3".to_vec()));
        assert_eq!(printed(&maps), vec!["\\C-tZ", "\\C-t\\000"]);
        maps.unbind_seq("emacs", &[0x14, b'Z']);
        assert_eq!(printed(&maps), vec!["\\C-t"]);
        // Binding over a sequence replaces whatever kind was there.
        maps.bind("emacs", &[0x14], Target::Command(b"echo".to_vec()));
        assert!(printed(&maps).is_empty());
    }
}
