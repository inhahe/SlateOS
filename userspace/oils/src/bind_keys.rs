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
/// Both `\C-` and `\M-` are *bit* operations, gathered as flags and applied to
/// the character they run into — control first, then meta — so `\C-\M-g` and
/// `\M-\C-g` are one and the same byte 0x87. A prefix that runs off the end of
/// `spec` is applied to a NUL, which is why `"y\M-"` binds the two bytes
/// `y\200` (measured); bash spells the rule out in a comment of its own.
///
/// `convert_meta` is readline's variable of that name. It decides the *last*
/// step only: a byte that came out with the top bit set is split into an escape
/// and the byte without it while the variable is on, and stored as itself while
/// it is off. That is the whole difference between `"\M-a"` being `\ea` and
/// being `\341`, and with it off `\M-a` and `\ea` are two different bindings
/// rather than one. readline turns it off for every locale that is not `C` or
/// `POSIX` (nls.c:168–186), which is every locale osh has — see
/// design-decisions.md §104 and [`crate::bind_tables::VARIABLES`].
///
/// The whole of this mirrors `rl_translate_keyseq` (bind.c:523–648).
#[must_use]
pub fn decode(spec: &[u8], convert_meta: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(spec.len());
    let mut i = 0usize;
    let mut has_control = false;
    let mut has_meta = false;
    // The loop runs one past the end when a modifier is still pending, so that
    // it can be applied to the NUL that ends the string.
    loop {
        let mut c = match spec.get(i) {
            Some(&c) => c,
            None if has_control || has_meta => 0,
            None => break,
        };
        let at_end = spec.get(i).is_none();
        i += 1;
        // Only a backslash with something after it escapes; a trailing one is
        // the byte itself.
        if c == b'\\' && spec.get(i).is_some() {
            // `\C-` and `\M-` modify what follows rather than standing for a
            // byte, so they are read before the escape table and the next
            // character is fetched as if they had not been written.
            if matches!(spec.get(i), Some(b'C' | b'M')) && spec.get(i + 1) == Some(&b'-') {
                if spec.get(i) == Some(&b'M') {
                    has_meta = true;
                } else {
                    has_control = true;
                }
                i += 2;
                continue;
            }
            let (b, used) = one_escape(spec, i);
            i += used;
            c = b;
        }
        if has_control {
            // `?` is the one control name that is not its letter's bit
            // pattern.
            c = if c == b'?' { RUBOUT } else { ctrl(c) };
            has_control = false;
        }
        if has_meta {
            c |= 0x80;
            has_meta = false;
        }
        if c >= 0x80 && convert_meta {
            out.push(ESC);
            out.push(c & 0x7f);
        } else {
            out.push(c);
        }
        if at_end {
            break;
        }
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
            // the sequence behind it. `\M-` is *not* what readline writes here,
            // even though that is how such a key is spelled on the way in: the
            // emacs map binds all of `\200`–`\377` to `self-insert` and lists
            // them that way (captured). Only the escape sub-map prints `\M-`,
            // which is where `convert-meta` sends a meta key as it is bound —
            // see [`Maps::landing`].
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
///
/// The marker is taken off *after* decoding rather than before, because the
/// text in front of it need not stand on its own: the escape sub-map's own
/// binding prints as `\M-\000`, and a bare `\M-` is a modifier with nothing to
/// modify — [`decode`] would hand it the NUL that ends the string and return
/// two bytes rather than one. Decoding the whole entry and dropping the one
/// NUL the marker contributed is the same reading readline gives it, since
/// `\000` is only ever the marker: a genuinely bound NUL prints `\C-@`
/// (`_rl_get_keyname`, bind.c:2592–2660).
///
/// A capture is what the *dumper* wrote, and the dumper's dialect is not
/// [`decode`]'s. `rl_invoking_keyseqs_in_map` (bind.c:2732–2741) emits `\M-`
/// for one thing only — the sub-map hanging off `ESC`, and only while
/// `convert-meta` is on — while every key that is not a sub-map goes through
/// `_rl_get_keyname` (bind.c:2592–2660), which writes a byte with the eighth
/// bit set in **octal** and never writes `\M-` at all. So in a capture `\M-`
/// is always exactly `\e`, and `\200` is always exactly one byte.
///
/// Reading a capture with `rl_translate_keyseq`'s meta rule would get both
/// halves of that wrong: it would turn `\200` into `ESC`, `\000` (readline
/// itself does not round-trip its own listing here — feeding `"\200"` back to
/// `bind` with `convert-meta` on really does bind `ESC` `NUL`). The capture is
/// a listing, so the listing's dialect governs: rewrite its `\M-` to the `\e`
/// it names, then decode with the meta bit standing for nothing but itself.
#[must_use]
pub fn table_entry(text: &str) -> (Vec<u8>, bool) {
    let src = text.as_bytes();
    let mut spec: Vec<u8> = Vec::with_capacity(src.len());
    let mut i = 0usize;
    while let Some(&c) = src.get(i) {
        // A backslash escapes exactly one character, so stepping over the pair
        // is what keeps `\\M-` — a literal backslash, then `M-` — from being
        // mistaken for the sub-map's name. Anything longer than the pair (an
        // octal run, the `-x` of `\C-x`) is plain text from here on and needs
        // no further care.
        if c == b'\\' && src.get(i + 1).is_some() {
            if src.get(i + 1) == Some(&b'M') && src.get(i + 2) == Some(&b'-') {
                spec.extend_from_slice(b"\\e");
                i = i.saturating_add(3);
                continue;
            }
            spec.extend_from_slice(&src[i..=i.saturating_add(1)]);
            i = i.saturating_add(2);
            continue;
        }
        spec.push(c);
        i = i.saturating_add(1);
    }
    let mut seq = decode(&spec, false);
    if text.ends_with("\\000") {
        // The decode of a `\000` is one NUL wherever it sat, so exactly one
        // comes off.
        debug_assert_eq!(seq.last(), Some(&0));
        seq.pop();
        return (seq, true);
    }
    (seq, false)
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
    /// readline's variables, in the order `bind -v` lists them, which is the
    /// order they are captured in. `keymap` is one of them: readline computes
    /// that row from whichever map is current rather than storing it, but the
    /// two are the same thing, so it is kept here and read back by
    /// [`Maps::keymap`].
    vars: Vec<(&'static str, Vec<u8>)>,
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
        Self {
            emacs: of("emacs"),
            vi: of("vi"),
            vi_insert: of("vi-insert"),
            vars: crate::bind_tables::VARIABLES
                .iter()
                .map(|(n, v)| (*n, v.as_bytes().to_vec()))
                .collect(),
        }
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

    /// Where a key sequence really lands.
    ///
    /// A lone key with the top bit set is not stored as itself while
    /// `convert-meta` is on: readline sends it into the escape sub-map as it
    /// binds it (`rl_bind_key_in_map`), which is why `Meta-t` and `"\M-t"` are
    /// one binding and both list as `\M-t` (measured). Longer sequences are
    /// already spelled with the escape by then, so only the one-byte case can
    /// be redirected.
    fn landing(&self, seq: &[u8]) -> Vec<u8> {
        match *seq {
            [b] if b >= 0x80 && self.var_on("convert-meta") => vec![ESC, b & 0x7f],
            _ => seq.to_vec(),
        }
    }

    /// Bind `seq` — as `keymap` spells it — replacing whatever was there.
    pub fn bind(&mut self, keymap: &str, seq: &[u8], target: Target) {
        let (root, prefix) = view(keymap);
        let mut full = prefix.to_vec();
        full.extend_from_slice(&self.landing(seq));
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
        full.extend_from_slice(&self.landing(seq));
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

    /// Every readline variable and its current value, in the order `bind -v`
    /// and `bind -V` list them.
    pub fn vars(&self) -> impl Iterator<Item = (&'static str, &[u8])> {
        self.vars.iter().map(|(n, v)| (*n, v.as_slice()))
    }

    /// The value of one variable, or the empty string if readline has no such
    /// variable — which no caller here can ask for, since the only way to name
    /// one is through [`Maps::set_var`], which rejects the unknown.
    fn var(&self, name: &str) -> &[u8] {
        self.vars.iter().find(|(n, _)| *n == name).map_or(&[][..], |(_, v)| v.as_slice())
    }

    /// Whether a boolean variable is on.
    fn var_on(&self, name: &str) -> bool {
        self.var(name) == b"on"
    }

    /// The live `convert-meta`, for a caller outside this module that has to
    /// hand it to [`decode`] or [`parse_operand`].
    #[must_use]
    pub fn convert_meta(&self) -> bool {
        self.var_on("convert-meta")
    }

    /// The keymap `bind` reads and writes when `-m` did not name one — the
    /// `keymap` variable, which `set keymap` and `set editing-mode` both move.
    ///
    /// The stored value is always a name [`keymap_name`] returned, so looking
    /// it up again gives back the same `'static` name and cannot fail; the
    /// fallback is unreachable rather than a guess. Not borrowing from `self`
    /// is what lets a caller hold the answer while it goes on to *change* the
    /// tables, which every mutating phase of `bind` does.
    #[must_use]
    pub fn keymap(&self) -> &'static str {
        keymap_name(self.var("keymap")).unwrap_or("emacs")
    }

    /// Move the keymap `bind` reads and writes, as readline's `rl_set_keymap`
    /// does for `bind -m`.
    ///
    /// The name is one [`Maps::keymap`] or [`keymap_name`] gave back, so it is
    /// already canonical and there is nothing here to reject — which is why
    /// this is not [`Maps::set_var`] with a `Result` nobody could act on.
    pub fn set_keymap(&mut self, name: &'static str) {
        if let Some(slot) = self.vars.iter_mut().find(|(n, _)| *n == "keymap") {
            slot.1 = name.as_bytes().to_vec();
        }
    }

    /// How `bind -p`, `-P` and `-q` spell an escape that prefixes a longer
    /// sequence.
    ///
    /// readline names the escape sub-map after the modifier it stands for only
    /// while it is *converting* meta characters into it; with `convert-meta
    /// off` nothing is redirected there and the byte is written as itself
    /// (measured: with it off, `bind -p` shows `"\e\C-t"` where it otherwise
    /// shows `"\M-\C-t"`).
    #[must_use]
    pub fn meta(&self) -> Meta {
        if self.var_on("convert-meta") { Meta::Prefix } else { Meta::Literal }
    }

    /// Apply a `set NAME VALUE`, as readline's `rl_variable_bind` does.
    ///
    /// `Err` is readline's own complaint about it, worded as readline words it
    /// and without the `readline: ` its caller prints. Neither failure is fatal
    /// — bash returns 0 from a `bind` whose operand readline refused (measured)
    /// — so the error is text to print and not a status.
    ///
    /// A boolean takes its value from readline's `bool_to_int`: on for an empty
    /// value, `1`, or `on` in any case, and off for everything else, including
    /// a word that means nothing (`set expand-tilde whatever` turns it *off*).
    /// Everything else is stored as written, except the two that name a keymap
    /// and are checked for it.
    pub fn set_var(&mut self, name: &[u8], value: &[u8]) -> Result<(), Vec<u8>> {
        let refused = || {
            let mut e = name.to_vec();
            e.extend_from_slice(b": could not set value to `");
            e.extend_from_slice(value);
            e.push(b'\'');
            e
        };
        let Some(i) = self.vars.iter().position(|(n, _)| n.as_bytes() == name) else {
            let mut e = name.to_vec();
            e.extend_from_slice(b": unknown variable name");
            return Err(e);
        };
        // A variable is a boolean exactly when readline's compiled-in value for
        // it is one, which is what the capture holds: no boolean is ever
        // anything but `on` or `off`, and no other variable is either.
        let boolean = matches!(
            crate::bind_tables::VARIABLES.iter().find(|(n, _)| n.as_bytes() == name),
            Some((_, "on" | "off"))
        );
        let new: Vec<u8> = match name {
            b"keymap" => keymap_name(value).ok_or_else(refused)?.as_bytes().to_vec(),
            // `editing-mode` is stored in its own right and *also* moves the
            // keymap, because in readline it is the keymap: choosing vi selects
            // the insert map, which is where a vi line starts.
            b"editing-mode" => {
                let starts_in = match value {
                    b"emacs" => "emacs",
                    b"vi" => "vi-insert",
                    _ => return Err(refused()),
                };
                self.set_var(b"keymap", starts_in.as_bytes())?;
                value.to_vec()
            }
            _ if boolean => {
                let on = value.is_empty() || value == b"1" || value.eq_ignore_ascii_case(b"on");
                if on { b"on".to_vec() } else { b"off".to_vec() }
            }
            _ => value.to_vec(),
        };
        if let Some(slot) = self.vars.get_mut(i) {
            slot.1 = new;
        }
        Ok(())
    }
}

/// The canonical name of the keymap readline knows by `name` — the first of
/// the aliases [`crate::bind_tables::KEYMAPS`] lists for it, so `vi-move` and
/// `vi-command` both come back as `vi`.
#[must_use]
pub fn keymap_name(name: &[u8]) -> Option<&'static str> {
    crate::bind_tables::KEYMAPS
        .iter()
        .find(|m| m.names.iter().any(|n| n.as_bytes() == name))
        .and_then(|m| m.names.first().copied())
}

/// What one `bind` operand asks for, once readline has read it.
pub enum Operand {
    /// Nothing to do: the line was blank, a comment, or a `$`-directive. The
    /// directives mean something only inside a *file*, where there are further
    /// lines for one to include or exclude — [`Maps::read_inputrc`] reads them
    /// before it gets here. A lone `bind '$if Bash'` has no such file and does
    /// nothing at all (measured: status 0, no output).
    Nothing,
    /// `set NAME VALUE`, for [`Maps::set_var`].
    Set(Vec<u8>, Vec<u8>),
    /// A key sequence and what to put at it. `None` is an *unbinding*: readline
    /// looks the target up and binds whatever it finds without checking, so a
    /// name it does not know — or no name at all, as in `"\C-t": ` — takes the
    /// key's binding away instead of failing (measured).
    Bind(Vec<u8>, Option<Target>),
    /// readline refused the line. The text is its complaint, without the
    /// `readline: ` its caller prints; the status stays 0 regardless.
    Error(Vec<u8>),
}

/// True for the bytes readline's parser treats as separating whitespace.
fn ws(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

/// Find the closing quote of a quoted run starting at `open`, honouring the
/// backslash that escapes one. `None` if the run never closes.
fn closing_quote(s: &[u8], open: usize) -> Option<usize> {
    let quote = *s.get(open)?;
    let mut j = open.checked_add(1)?;
    loop {
        match s.get(j) {
            None => return None,
            // The escaped byte is skipped whatever it is — that is how `"a\"b"`
            // stays one run — and a trailing backslash runs off the end, which
            // is the unterminated case.
            Some(b'\\') => j = j.checked_add(2)?,
            Some(&b) if b == quote => return Some(j),
            Some(_) => j = j.checked_add(1)?,
        }
    }
}

/// The single key an unquoted key name stands for — readline's
/// `glean_key_from_name`, plus the modifier bits its caller adds.
///
/// The name proper is whatever follows the last `-`, so `Control-x` gleans `x`;
/// the modifiers are then read from the *whole* name as case-insensitive
/// substrings, which is why `ESC-w` is a control binding and not a meta one:
/// it contains `C-`, and readline's meta prefixes are only `Meta` and `M-`
/// (measured — it binds `\C-w`).
fn glean_key(name: &[u8]) -> u8 {
    let tail = match name.iter().rposition(|&b| b == b'-') {
        Some(i) if i + 1 < name.len() => name.get(i + 1..).unwrap_or(name),
        _ => name,
    };
    let named = |want: &str| tail.eq_ignore_ascii_case(want.as_bytes());
    let mut key = if named("rubout") || named("del") {
        RUBOUT
    } else if named("esc") || named("escape") {
        ESC
    } else if named("lfd") || named("newline") {
        b'\n'
    } else if named("ret") || named("return") {
        b'\r'
    } else if named("spc") || named("space") {
        b' '
    } else if named("tab") {
        b'\t'
    } else {
        tail.first().copied().unwrap_or(0)
    };
    let has = |what: &str| {
        let w = what.as_bytes();
        name.windows(w.len()).any(|s| s.eq_ignore_ascii_case(w))
    };
    if has("Control-") || has("C-") || has("CTRL-") {
        key = ctrl(key);
    }
    if has("Meta") || has("M-") {
        key |= 0x80;
    }
    key
}

/// Read one `bind` operand — readline's `rl_parse_and_bind`.
///
/// The shape is a key sequence, a separator, and a target. The separator is a
/// colon *or* whitespace, whichever comes first, and only the one byte: a
/// colon that follows a space has already missed its turn and is read as the
/// target instead, so `"\C-t" : yank` unbinds `\C-t` rather than binding it
/// (measured). A quoted key sequence is skipped over whole while looking for
/// that separator, so a colon or a space inside it is part of the key.
///
/// `convert_meta` is the live value of readline's variable, which the caller
/// has to read afresh for every operand: a `set convert-meta off` earlier in
/// the same file steers the bindings after it, so the flag is not a property of
/// the call. See [`decode`].
#[must_use]
pub fn parse_operand(spec: &[u8], convert_meta: bool) -> Operand {
    let start = spec.iter().position(|&b| !ws(b)).unwrap_or(spec.len());
    let s = spec.get(start..).unwrap_or(&[]);
    match s.first() {
        None | Some(b'#' | b'$') => return Operand::Nothing,
        _ => {}
    }
    // `set` is recognised before anything else and takes the rest of the line
    // as a name and a value, so neither needs a separator and the value may
    // hold anything at all.
    if s.len() >= 3
        && s.get(..3).is_some_and(|w| w.eq_ignore_ascii_case(b"set"))
        && s.get(3).is_none_or(|&b| ws(b))
    {
        let rest = s.get(3..).unwrap_or(&[]);
        let a = rest.iter().position(|&b| !ws(b)).unwrap_or(rest.len());
        let named = rest.get(a..).unwrap_or(&[]);
        let b = named.iter().position(|&c| ws(c)).unwrap_or(named.len());
        let (name, after) = named.split_at(b);
        let c = after.iter().position(|&b| !ws(b)).unwrap_or(after.len());
        return Operand::Set(name.to_vec(), after.get(c..).unwrap_or(&[]).to_vec());
    }

    let mut i = 0usize;
    while let Some(&c) = s.get(i) {
        if c == b'"' {
            let Some(close) = closing_quote(s, i) else {
                let mut e = b"".to_vec();
                e.extend_from_slice(spec);
                e.extend_from_slice(b": no closing `\"' in key binding");
                return Operand::Error(e);
            };
            i = close;
        } else if c == b':' || ws(c) {
            break;
        }
        i += 1;
    }
    if i >= s.len() {
        let mut e = spec.to_vec();
        e.extend_from_slice(b": no key sequence terminator");
        return Operand::Error(e);
    }
    let keyname = s.get(..i).unwrap_or(&[]);
    // Exactly one byte of separator is consumed, then whitespace — never a
    // second colon.
    let after = s.get(i.saturating_add(1)..).unwrap_or(&[]);
    let t = after.iter().position(|&b| !ws(b)).unwrap_or(after.len());
    let target = after.get(t..).unwrap_or(&[]);

    let bound = match target.first() {
        // A quoted target is a macro — the text readline pushes back as if it
        // had been typed, so it is decoded like a key sequence. An unterminated
        // one is not an error here: readline takes what there is.
        Some(b'"' | b'\'') => {
            let end = closing_quote(target, 0).unwrap_or(target.len());
            Some(Target::Macro(decode(
                target.get(1..end).unwrap_or(&[]),
                convert_meta,
            )))
        }
        _ => {
            let end = target.iter().position(|&b| ws(b)).unwrap_or(target.len());
            function(target.get(..end).unwrap_or(&[])).map(Target::Function)
        }
    };
    let seq = if keyname.first() == Some(&b'"') {
        let end = closing_quote(keyname, 0).unwrap_or(keyname.len());
        decode(keyname.get(1..end).unwrap_or(&[]), convert_meta)
    } else {
        vec![glean_key(keyname)]
    };
    Operand::Bind(seq, bound)
}

/// Where an inputrc reader gets the bytes of a file.
///
/// A trait rather than a path, because this module does no I/O: an `$include`
/// names a file that the *shell* has to resolve — relative to its own working
/// directory, not to the including file's (measured), and with `~` expanded, as
/// readline's `_rl_read_init_file` does before it opens anything.
pub trait Files {
    /// The bytes of `path`, or `None` when it cannot be read. An inputrc that
    /// is not there is not an error: readline says nothing and carries on
    /// (measured — `$include /nosuch/file` leaves status 0 and no output).
    fn read(&mut self, path: &[u8]) -> Option<Vec<u8>>;
}

/// The readline these tables were captured from, as `$if version` compares it:
/// major × 10 + minor. bash 5.2.37 links readline 8.2, and the version an
/// inputrc tests has to be the version whose defaults `bind_tables` holds —
/// answering 8.2 while carrying some other release's bindings would let a file
/// enable exactly the wrong half of itself.
const READLINE_VERSION: u32 = 82;

/// How deep `$include` may nest before the reader stops.
///
/// readline has no limit and a file that includes itself hangs it. A shell that
/// can be hung by a config file is not one this can ship, and no real inputrc
/// nests anywhere near this far, so the recursion is bounded and the overflow
/// is silent — the same nothing readline says for an include it cannot read.
const INCLUDE_DEPTH: u32 = 8;

/// One `$`-directive line, once the leading `$` has been taken off.
enum Directive<'a> {
    If(&'a [u8]),
    Else,
    Endif,
    Include(&'a [u8]),
    /// The word after the `$`, which readline does not recognise.
    Unknown(&'a [u8]),
}

/// Split a `$` line into its directive word and the rest.
fn directive(line: &[u8]) -> Directive<'_> {
    let end = line.iter().position(|&b| ws(b)).unwrap_or(line.len());
    let word = line.get(..end).unwrap_or(&[]);
    let rest = line.get(end..).unwrap_or(&[]);
    let start = rest.iter().position(|&b| !ws(b)).unwrap_or(rest.len());
    let args = rest.get(start..).unwrap_or(&[]);
    if word.eq_ignore_ascii_case(b"if") {
        Directive::If(args)
    } else if word.eq_ignore_ascii_case(b"else") {
        Directive::Else
    } else if word.eq_ignore_ascii_case(b"endif") {
        Directive::Endif
    } else if word.eq_ignore_ascii_case(b"include") {
        Directive::Include(args)
    } else {
        Directive::Unknown(word)
    }
}

/// `major[.minor]` as `$if version` counts it: 8.2 is 82, and a bare `4` is 40.
fn version_arg(s: &[u8]) -> u32 {
    let num = |b: &[u8]| -> u32 {
        b.iter()
            .take_while(|c| c.is_ascii_digit())
            .fold(0u32, |a, c| a.saturating_mul(10).saturating_add(u32::from(c - b'0')))
    };
    let (major, minor) = match s.iter().position(|&b| b == b'.') {
        Some(i) => (s.get(..i).unwrap_or(&[]), s.get(i.saturating_add(1)..).unwrap_or(&[])),
        None => (s, &[][..]),
    };
    num(major).saturating_mul(10).saturating_add(num(minor))
}

/// Evaluate `$if version OP N` — the one condition with an operator.
fn version_test(args: &[u8]) -> bool {
    let rest = args.get(7..).unwrap_or(&[]);
    let start = rest.iter().position(|&b| !ws(b)).unwrap_or(rest.len());
    let rest = rest.get(start..).unwrap_or(&[]);
    // Two-byte operators are tried first, so `>=` is not read as `>` with a
    // stray `=` in front of the number.
    let (op, tail): (&[u8], &[u8]) = match rest.get(..2) {
        Some(o @ (b"==" | b"!=" | b"<=" | b">=")) => (o, rest.get(2..).unwrap_or(&[])),
        _ => match rest.first() {
            Some(b'=') => (b"==", rest.get(1..).unwrap_or(&[])),
            Some(b'<') => (b"<", rest.get(1..).unwrap_or(&[])),
            Some(b'>') => (b">", rest.get(1..).unwrap_or(&[])),
            _ => return false,
        },
    };
    let start = tail.iter().position(|&b| !ws(b)).unwrap_or(tail.len());
    let want = version_arg(tail.get(start..).unwrap_or(&[]));
    let have = READLINE_VERSION;
    match op {
        b"==" => have == want,
        b"!=" => have != want,
        b"<=" => have <= want,
        b">=" => have >= want,
        b"<" => have < want,
        _ => have > want,
    }
}

impl Maps {
    /// Is `$if ARGS` true?
    ///
    /// Four forms, in readline's own order: `mode=`, `term=`, `version`, and —
    /// anything else — the application name, compared case-insensitively, which
    /// is why `$if Bash` is true and `$if application=bash` is not (measured:
    /// readline has no `application=` form, so that string is simply a name
    /// that is not `bash`).
    fn if_test(&self, args: &[u8], term: &[u8]) -> bool {
        if let Some(mode) = args.strip_prefix(b"mode=") {
            // The mode is `editing-mode`, not the current keymap: a file that
            // says `set keymap vi-insert` has not changed which editor this is.
            return mode == self.var("editing-mode");
        }
        if let Some(want) = args.strip_prefix(b"term=") {
            // readline matches the full name and the part before the first `-`,
            // so `$if term=xterm` fires on an `xterm-256color`.
            let short = term.split(|&b| b == b'-').next().unwrap_or(term);
            return want == term || want == short;
        }
        if args.get(..7).is_some_and(|w| w.eq_ignore_ascii_case(b"version")) {
            return version_test(args);
        }
        args.eq_ignore_ascii_case(b"bash")
    }

    /// Apply an inputrc — readline's `_rl_read_init_file`.
    ///
    /// `name` is what a complaint calls the file; each one is pushed onto `errs`
    /// already carrying its `FILE: line N: ` prefix and needing only the
    /// `readline: ` its caller prints. Nothing here is a failure: readline
    /// reports a line it cannot read and goes on to the next, and the builtin's
    /// status does not move (measured — a file of five bad lines still leaves
    /// `bind -f` at 0).
    pub fn read_inputrc(
        &mut self,
        text: &[u8],
        name: &[u8],
        term: &[u8],
        files: &mut dyn Files,
        errs: &mut Vec<Vec<u8>>,
    ) {
        self.read_inputrc_at(text, name, term, files, errs, 0);
    }

    fn read_inputrc_at(
        &mut self,
        text: &[u8],
        name: &[u8],
        term: &[u8],
        files: &mut dyn Files,
        errs: &mut Vec<Vec<u8>>,
        depth: u32,
    ) {
        // `off` is readline's `_rl_parsing_conditionalized_out`; `stack` holds
        // what it was outside each open `$if`, so a nested one restores rather
        // than clears. A `$if` inside a region already switched off is pushed
        // but never evaluated — nothing can turn parsing back on except the
        // matching `$endif`.
        let mut off = false;
        let mut stack: Vec<bool> = Vec::new();
        for (n, raw) in text.split(|&b| b == b'\n').enumerate() {
            let line_no = n.saturating_add(1);
            let at = |msg: &[u8]| {
                let mut e = name.to_vec();
                e.extend_from_slice(format!(": line {line_no}: ").as_bytes());
                e.extend_from_slice(msg);
                e
            };
            let start = raw.iter().position(|&b| !ws(b)).unwrap_or(raw.len());
            let line = raw.get(start..).unwrap_or(&[]);
            match line.first() {
                None | Some(b'#') => continue,
                // A directive is handled even inside a switched-off region —
                // that is how the matching `$endif` is ever found, and it is
                // also why an unknown one is reported there too.
                Some(b'$') => {
                    match directive(line.get(1..).unwrap_or(&[])) {
                        Directive::If(args) => {
                            stack.push(off);
                            if !off {
                                off = !self.if_test(args, term);
                            }
                        }
                        Directive::Else => match stack.last() {
                            None => errs.push(at(b"$else found without matching $if")),
                            // Enclosed by a region that is already off: the
                            // `$else` half is off too, whatever the `$if` said.
                            Some(&outer) => {
                                if !outer {
                                    off = !off;
                                }
                            }
                        },
                        Directive::Endif => match stack.pop() {
                            Some(outer) => off = outer,
                            None => errs.push(at(b"$endif without matching $if")),
                        },
                        Directive::Include(path) => {
                            if !off
                                && depth < INCLUDE_DEPTH
                                && let Some(text) = files.read(path)
                            {
                                self.read_inputrc_at(
                                    &text,
                                    path,
                                    term,
                                    files,
                                    errs,
                                    depth.saturating_add(1),
                                );
                            }
                        }
                        Directive::Unknown(word) => {
                            let mut m = word.to_vec();
                            m.extend_from_slice(b": unknown parser directive");
                            errs.push(at(&m));
                        }
                    }
                    continue;
                }
                _ => {}
            }
            if off {
                continue;
            }
            // Read afresh: a `set convert-meta off` on an earlier line of this
            // same file steers every binding after it.
            match parse_operand(line, self.var_on("convert-meta")) {
                Operand::Nothing => {}
                Operand::Error(msg) => errs.push(at(&msg)),
                Operand::Set(n, v) => {
                    if let Err(msg) = self.set_var(&n, &v) {
                        errs.push(at(&msg));
                    }
                }
                // A file's bindings go into whatever `set keymap` last named,
                // which is why a `set keymap vi-insert` halfway down one scopes
                // everything after it (measured).
                Operand::Bind(seq, target) => {
                    let km = self.keymap();
                    match target {
                        Some(t) => self.bind(km, &seq, t),
                        None => self.unbind_seq(km, &seq),
                    }
                }
            }
        }
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
        // octal it is, the hex it is, and the escape readline prints. No meta
        // is involved, so `convert-meta` cannot change any of them.
        for spec in [r"\C-y", r"\C-Y", r"\031", r"\x19"] {
            for cm in [true, false] {
                assert_eq!(decode(spec.as_bytes(), cm), vec![0x19], "{spec} cm={cm}");
            }
        }
        // `\M-` sets the eighth bit (bind.c:610, `c = META(c)`); it is only
        // *afterwards* that `convert-meta` splits the result into ESC + the
        // low seven bits (bind.c:640). So with convert-meta off — which is
        // where any non-C locale leaves readline — `\M-y` is one byte, and
        // `\M-y` and `\ey` stop being the same binding.
        assert_eq!(decode(br"\M-y", true), vec![0x1b, b'y']);
        assert_eq!(decode(br"\M-y", false), vec![0xf9]);
        for spec in [r"\ey", r"\033y"] {
            for cm in [true, false] {
                assert_eq!(decode(spec.as_bytes(), cm), vec![0x1b, b'y'], "{spec} cm={cm}");
            }
        }
        // The two modifiers commute: both are gathered as flags and applied
        // control-first (bind.c:600-611), whichever order they were written.
        for spec in [r"\M-\C-g", r"\C-\M-g"] {
            assert_eq!(decode(spec.as_bytes(), true), vec![0x1b, 0x07], "{spec}");
            assert_eq!(decode(spec.as_bytes(), false), vec![0x87], "{spec}");
        }
        for cm in [true, false] {
            assert_eq!(decode(br"\C-?", cm), vec![0x7f], "cm={cm}");
            assert_eq!(decode(br"\d", cm), vec![0x7f], "cm={cm}");
            assert_eq!(decode(br"\C-@", cm), vec![0x00], "cm={cm}");
            assert_eq!(decode(br"\C-\\", cm), vec![0x1c], "cm={cm}");
            // An unknown escape is the letter itself, and a trailing
            // backslash is a backslash.
            assert_eq!(decode(br"\q", cm), vec![b'q'], "cm={cm}");
            assert_eq!(decode(br"a\", cm), vec![b'a', b'\\'], "cm={cm}");
            // `\x` with nothing behind it is an `x`.
            assert_eq!(decode(br"\xz", cm), vec![b'x', b'z'], "cm={cm}");
        }
        // A modifier that runs off the end has nothing to modify but the NUL
        // the string ends with, so these bind *two* bytes, not one. Measured
        // against bash 5.2.37: `"x\C-": yank` lists as `"x\C-@"`, and
        // `"y\M-": yank` as `"y\200"`.
        assert_eq!(decode(br"x\C-", false), vec![b'x', 0x00]);
        assert_eq!(decode(br"y\M-", false), vec![b'y', 0x80]);
        assert_eq!(decode(br"y\M-", true), vec![b'y', 0x1b, 0x00]);
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

    /// The separator is one byte and readline takes the first one it meets, so
    /// a colon that has already been preceded by a space is not a separator at
    /// all — it is the target, and an unrecognised target *unbinds*. Every line
    /// here is measured against bash 5.2.
    #[test]
    fn the_separator_is_the_first_of_a_colon_or_a_space_and_only_one_byte() {
        let bound = |spec: &str| match super::parse_operand(spec.as_bytes(), false) {
            super::Operand::Bind(seq, target) => (
                String::from_utf8_lossy(&encode(&seq, false, Meta::Prefix)).into_owned(),
                match target {
                    Some(Target::Function(f)) => f.to_string(),
                    Some(Target::Macro(m)) => format!("macro {}", String::from_utf8_lossy(&m)),
                    Some(Target::Command(c)) => format!("cmd {}", String::from_utf8_lossy(&c)),
                    None => "(unbind)".to_string(),
                },
            ),
            _ => panic!("{spec}: not a binding"),
        };
        assert_eq!(bound(r#""\C-t": yank"#), ("\\C-t".into(), "yank".into()));
        // Whitespace separates as well as a colon does, and any run of it.
        assert_eq!(bound(r#""\C-t"   yank"#), ("\\C-t".into(), "yank".into()));
        assert_eq!(bound("Control-w: yank"), ("\\C-w".into(), "yank".into()));
        // The space came first, so the colon is what the target starts with.
        assert_eq!(bound(r#""\C-t" : yank"#), ("\\C-t".into(), "(unbind)".into()));
        // So is nothing at all, and so is a name readline does not know.
        assert_eq!(bound(r#""\C-t": "#), ("\\C-t".into(), "(unbind)".into()));
        assert_eq!(bound(r#""\C-t": nosuchfunc"#), ("\\C-t".into(), "(unbind)".into()));
        // The target ends at the first whitespace, so trailing space is not
        // part of the name.
        assert_eq!(bound(r#""\C-t": yank   "#), ("\\C-t".into(), "yank".into()));
        // Either quote makes a macro, and its text is decoded like a sequence.
        assert_eq!(bound(r#""\C-j": "hi""#), ("\\C-j".into(), "macro hi".into()));
        assert_eq!(bound(r#""\C-j": 'hi'"#), ("\\C-j".into(), "macro hi".into()));
        // An alias resolves to the one function it names — to that group's
        // representative, whichever of the names that is, so that binding
        // through either name is a binding a listing finds under both.
        assert_eq!(
            bound(r#""\C-t": insert-last-argument"#).1,
            bound(r#""\C-t": yank-last-arg"#).1
        );
    }

    /// An unquoted key name is a single key plus modifier bits read off the
    /// whole name — and `ESC-w` is a *control* binding, because readline's
    /// meta prefixes are only `Meta` and `M-` while `ESC-w` does contain `C-`.
    #[test]
    fn an_unquoted_key_name_gleans_one_key_and_its_modifiers() {
        let key = |spec: &str| match super::parse_operand(format!("{spec}: yank").as_bytes(), false)
        {
            super::Operand::Bind(seq, _) => seq,
            _ => panic!("{spec}: not a binding"),
        };
        assert_eq!(key("q"), vec![b'q']);
        assert_eq!(key("Control-w"), vec![0x17]);
        assert_eq!(key("C-w"), vec![0x17]);
        assert_eq!(key("ESC-w"), vec![0x17]);
        assert_eq!(key("Meta-t"), vec![0xf4]);
        assert_eq!(key("Control-Meta-t"), vec![0x94]);
        assert_eq!(key("space"), vec![b' ']);
        assert_eq!(key("rubout"), vec![super::RUBOUT]);
        assert_eq!(key("escape"), vec![super::ESC]);
    }

    /// A meta key is not stored as the byte it gleans: `convert-meta` sends it
    /// into the escape sub-map as it is bound, which is why `Meta-t` and
    /// `"\M-t"` are one binding and both list as `\M-t` (measured).
    #[test]
    fn a_meta_key_lands_in_the_escape_map_while_convert_meta_is_on() {
        let listed = |maps: &super::Maps| -> Vec<String> {
            maps.entries("emacs")
                .iter()
                .filter(|e| matches!(e.target, Target::Function("yank")))
                .map(|e| String::from_utf8_lossy(&encode(e.seq, e.is_prefix, maps.meta())).into_owned())
                .collect()
        };
        let mut maps = super::Maps::seeded();
        // The seeded default is *off* (readline's eight-bit set, which is the
        // one every locale but `C`/`POSIX` gets — see `VARIABLES`), so the
        // conversion this test is about has to be asked for.
        maps.set_var(b"convert-meta", b"on").expect("convert-meta is a variable");
        maps.unbind_function("emacs", "yank");
        maps.bind("emacs", &[0xf4], Target::Function("yank"));
        assert_eq!(listed(&maps), vec!["\\M-t"]);
        // The two spellings are the same slot, so binding the other one over it
        // leaves one entry and not two.
        maps.bind("emacs", &[super::ESC, b't'], Target::Function("yank"));
        assert_eq!(listed(&maps), vec!["\\M-t"]);
        // With the conversion off the byte stays a byte — and is printed in
        // octal, the escape sub-map being the only thing that prints `\M-`.
        maps.set_var(b"convert-meta", b"off").expect("convert-meta is a variable");
        maps.unbind_function("emacs", "yank");
        maps.bind("emacs", &[0xf4], Target::Function("yank"));
        assert_eq!(listed(&maps), vec!["\\364"]);
    }

    /// `set` normalises a boolean by readline's rule — on for an empty value,
    /// `1`, or `on` in any case, and off for *everything* else, a word that
    /// means nothing included — and stores everything else as written.
    #[test]
    fn a_variable_takes_the_value_readline_would_have_stored() {
        let mut maps = super::Maps::seeded();
        let get = |m: &super::Maps, n: &str| String::from_utf8_lossy(m.var(n)).into_owned();
        for (given, want) in [("1", "on"), ("On", "on"), ("", "on"), ("whatever", "off"), ("0", "off")] {
            maps.set_var(b"expand-tilde", given.as_bytes()).expect("a boolean");
            assert_eq!(get(&maps, "expand-tilde"), want, "set expand-tilde {given}");
        }
        maps.set_var(b"comment-begin", b";;").expect("a string variable");
        assert_eq!(get(&maps, "comment-begin"), ";;");
        // A keymap name is canonicalised, and an impossible one is refused
        // without disturbing what was there.
        maps.set_var(b"keymap", b"vi-move").expect("a keymap name");
        assert_eq!(maps.keymap(), "vi");
        assert!(maps.set_var(b"keymap", b"nosuchmap").is_err());
        assert_eq!(maps.keymap(), "vi");
        // `editing-mode` is the keymap under another name: vi starts insert.
        maps.set_var(b"editing-mode", b"vi").expect("an editing mode");
        assert_eq!(maps.keymap(), "vi-insert");
        maps.set_var(b"editing-mode", b"emacs").expect("an editing mode");
        assert_eq!(maps.keymap(), "emacs");
        assert!(maps.set_var(b"editing-mode", b"sideways").is_err());
        assert_eq!(maps.keymap(), "emacs");
        assert!(maps.set_var(b"nosuchvar", b"x").is_err());
    }

    /// The lines readline does nothing with, and the two it refuses.
    #[test]
    fn a_line_that_binds_nothing_is_read_as_nothing() {
        let kind = |spec: &str| match super::parse_operand(spec.as_bytes(), false) {
            super::Operand::Nothing => "nothing".to_string(),
            super::Operand::Error(e) => String::from_utf8_lossy(&e).into_owned(),
            super::Operand::Set(n, v) => {
                format!("set {} {}", String::from_utf8_lossy(&n), String::from_utf8_lossy(&v))
            }
            super::Operand::Bind(..) => "bind".to_string(),
        };
        assert_eq!(kind(""), "nothing");
        assert_eq!(kind("   # a comment"), "nothing");
        assert_eq!(kind("$if Bash"), "nothing");
        assert_eq!(kind("yank"), "yank: no key sequence terminator");
        assert_eq!(kind(r#""\C-t""#), "\"\\C-t\": no key sequence terminator");
        assert_eq!(kind(r#""\C-t"#), "\"\\C-t: no closing `\"' in key binding");
        // `set` is read before any of that, so its value needs no separator and
        // may hold anything — including nothing.
        assert_eq!(kind("set bell-style visible"), "set bell-style visible");
        assert_eq!(kind("set comment-begin ;;"), "set comment-begin ;;");
        assert_eq!(kind("set"), "set  ");
    }

    /// No `$include` resolves to anything, which is every inputrc test below
    /// except the one about including.
    struct NoFiles;
    impl super::Files for NoFiles {
        fn read(&mut self, _path: &[u8]) -> Option<Vec<u8>> {
            None
        }
    }

    /// Read `text` as an inputrc and report the sequences `yank` ends up on in
    /// the emacs map, plus whatever readline complained about.
    fn read(text: &str, files: &mut dyn super::Files) -> (Vec<String>, Vec<String>) {
        let mut maps = super::Maps::seeded();
        let mut errs: Vec<Vec<u8>> = Vec::new();
        maps.read_inputrc(text.as_bytes(), b"rc", b"xterm-256color", files, &mut errs);
        let seqs = maps
            .entries("emacs")
            .iter()
            .filter(|e| matches!(e.target, Target::Function("yank")))
            .map(|e| String::from_utf8_lossy(&encode(e.seq, e.is_prefix, Meta::Prefix)).into_owned())
            .collect();
        let errs = errs
            .iter()
            .map(|e| String::from_utf8_lossy(e).into_owned())
            .collect();
        (seqs, errs)
    }

    /// The keys `yank` gains over its one default, `\C-y`.
    fn added(text: &str) -> Vec<String> {
        let (mut seqs, errs) = read(text, &mut NoFiles);
        assert!(errs.is_empty(), "{errs:?}");
        seqs.retain(|s| s != "\\C-y");
        seqs
    }

    /// A `$if` that is false takes its whole body with it, including any `$if`
    /// nested inside — nothing but the matching `$endif` turns parsing back on.
    #[test]
    fn a_false_conditional_hides_everything_down_to_its_own_endif() {
        assert_eq!(added("$if Bash\n\"\\C-t\": yank\n$endif\n"), ["\\C-t"]);
        assert_eq!(added("$if nosuchapp\n\"\\C-t\": yank\n$endif\n"), [] as [&str; 0]);
        // The inner `$if` is true on its own and still contributes nothing.
        assert_eq!(
            added("$if nosuchapp\n$if Bash\n\"\\C-t\": yank\n$endif\n\"\\C-e\": yank\n$endif\n"),
            [] as [&str; 0]
        );
        // ... and the `$else` half of an outer false one is the half that runs.
        assert_eq!(
            added("$if nosuchapp\n\"\\C-t\": yank\n$else\n\"\\C-e\": yank\n$endif\n"),
            ["\\C-e"]
        );
        // An `$else` inside a region already switched off stays switched off,
        // rather than flipping the region back on.
        assert_eq!(
            added("$if nosuchapp\n$if Bash\n$else\n\"\\C-t\": yank\n$endif\n$endif\n"),
            [] as [&str; 0]
        );
        // A `$if` left open simply ends with the file (measured: no complaint).
        assert_eq!(added("$if Bash\n\"\\C-t\": yank\n"), ["\\C-t"]);
    }

    /// The four things a `$if` can ask about, as readline answers them.
    #[test]
    fn a_conditional_tests_the_application_the_mode_the_terminal_or_the_version() {
        // The bare form is the application name, case-insensitively — and there
        // is no `application=` form, so that whole string is just a name that
        // is not `bash` (measured).
        assert_eq!(added("$if BaSh\n\"\\C-t\": yank\n$endif\n"), ["\\C-t"]);
        assert_eq!(added("$if application=bash\n\"\\C-t\": yank\n$endif\n"), [] as [&str; 0]);
        assert_eq!(added("$if mode=emacs\n\"\\C-t\": yank\n$endif\n"), ["\\C-t"]);
        assert_eq!(added("$if mode=vi\n\"\\C-t\": yank\n$endif\n"), [] as [&str; 0]);
        // The terminal matches in full and up to the first `-`.
        assert_eq!(added("$if term=xterm-256color\n\"\\C-t\": yank\n$endif\n"), ["\\C-t"]);
        assert_eq!(added("$if term=xterm\n\"\\C-t\": yank\n$endif\n"), ["\\C-t"]);
        assert_eq!(added("$if term=xter\n\"\\C-t\": yank\n$endif\n"), [] as [&str; 0]);
        // 8.2, against every operator. A bare major number is `major.0`.
        for (test, want) in [
            ("version >= 4.0", true),
            ("version < 4.0", false),
            ("version == 8.2", true),
            ("version = 8.2", true),
            ("version != 8.2", false),
            ("version > 8.2", false),
            ("version <= 8.2", true),
            ("version > 8", true),
            ("version", false),
        ] {
            let got = !added(&format!("$if {test}\n\"\\C-t\": yank\n$endif\n")).is_empty();
            assert_eq!(got, want, "$if {test}");
        }
    }

    /// Everything a file can get wrong, worded as readline words it — and none
    /// of it stops the lines that follow.
    #[test]
    fn a_bad_line_is_reported_against_its_own_line_number_and_read_past() {
        let (seqs, errs) = read(
            "set nosuchvariable on\n$else\n$endif\n$nonsense arg\n\"\\C-t\": yank\n",
            &mut NoFiles,
        );
        assert_eq!(
            errs,
            [
                "rc: line 1: nosuchvariable: unknown variable name",
                "rc: line 2: $else found without matching $if",
                "rc: line 3: $endif without matching $if",
                "rc: line 4: nonsense: unknown parser directive",
            ]
        );
        assert!(seqs.contains(&"\\C-t".to_string()));
    }

    /// A file's bindings go into the keymap `set keymap` last named, and the
    /// naming outlives the file (measured).
    #[test]
    fn set_keymap_inside_a_file_steers_the_lines_after_it() {
        let mut maps = super::Maps::seeded();
        let mut errs: Vec<Vec<u8>> = Vec::new();
        maps.read_inputrc(
            b"set keymap vi-insert\n\"\\C-t\": yank\nset keymap vi-command\n\"\\C-e\": yank\n",
            b"rc",
            b"dumb",
            &mut NoFiles,
            &mut errs,
        );
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(maps.keymap(), "vi");
        let on = |map: &str| -> Vec<Vec<u8>> {
            maps.entries(map)
                .iter()
                .filter(|e| matches!(e.target, Target::Function("yank")))
                .map(|e| e.seq.to_vec())
                .collect()
        };
        assert!(on("vi-insert").contains(&vec![0x14]));
        assert!(!on("vi-insert").contains(&vec![0x05]));
        assert!(on("vi").contains(&vec![0x05]));
    }

    /// An `$include` is read; one that cannot be is silently nothing; and a
    /// file that includes itself stops rather than hanging the shell.
    #[test]
    fn an_include_is_read_once_and_cannot_recurse_forever() {
        struct Canned(&'static str, std::cell::Cell<u32>);
        impl super::Files for Canned {
            fn read(&mut self, _path: &[u8]) -> Option<Vec<u8>> {
                self.1.set(self.1.get().saturating_add(1));
                Some(self.0.as_bytes().to_vec())
            }
        }
        let mut files = Canned("\"\\C-t\": yank\n", std::cell::Cell::new(0));
        let (seqs, errs) = read("$include other\n", &mut files);
        assert!(errs.is_empty(), "{errs:?}");
        assert!(seqs.contains(&"\\C-t".to_string()));
        assert_eq!(files.1.get(), 1);

        // Nothing to read is not an error: readline says nothing (measured).
        let (_, errs) = read("$include /nosuch/file\n", &mut NoFiles);
        assert!(errs.is_empty(), "{errs:?}");

        // A self-including file reaches the depth cap instead of the stack.
        let mut files = Canned("$include self\n", std::cell::Cell::new(0));
        let (_, errs) = read("$include self\n", &mut files);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(files.1.get(), super::INCLUDE_DEPTH);

        // An include inside a false conditional is not even opened.
        let mut files = Canned("\"\\C-t\": yank\n", std::cell::Cell::new(0));
        let (_, errs) = read("$if nosuchapp\n$include other\n$endif\n", &mut files);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(files.1.get(), 0);
    }
}
