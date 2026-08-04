//! The storage behind a shell associative array (`declare -A m; m[k]=v`).
//!
//! **This is a port of bash's `hashlib.c`, and it is one deliberately.** The
//! order an associative array enumerates in is observable — `${!m[@]}`,
//! `${m[@]}`, `declare -p m` and `for k in "${!m[@]}"` all show it — and bash's
//! order is neither insertion order nor sorted order: it is the walk of an open
//! hash table, buckets in index order and each bucket's chain from its head.
//! Nothing but the same table reproduces it, so this *is* the same table.
//!
//! The parameters below were not read out of a header, they were **measured**
//! against bash 5.2.37 and are the only combination that fits: the key sets
//! `key0..key{n}` for n of 4, 8, 16, 32, 40, 64, 100, 200, 300, 600, 2050 and
//! 4200 (which crosses two growths), plus assorted short-key sets, plus
//! high-byte keys, all reproduced byte for byte.
//!
//! * The hash is **FNV-1** — multiply *then* xor — seeded and stepped with the
//!   32-bit FNV constants, and it xors in a **signed** char, because bash's
//!   `hash_string` walks a `char *` and `char` is signed on x86.
//! * A fresh table has [`INITIAL_BUCKETS`] buckets, and the bucket is
//!   `hash & (nbuckets - 1)`.
//! * A new entry goes at the **head** of its chain, so within one bucket the
//!   order is the reverse of insertion.
//! * The table grows when the entry count *reaches* [`LOAD_FACTOR`] per bucket,
//!   multiplying the bucket count by [`GROW_BY`]; the rehash walks the old
//!   buckets in index order and pushes each entry onto the head of its new
//!   chain, which is why a growth reshuffles more than it redistributes.
//! * Removal only unlinks — the table never shrinks and nothing else moves.
//!
//! Lookup is on the hot path (every element read *and* every element write is
//! one), and this shape keeps it O(1): the load factor bounds a chain at a
//! handful of entries, so the scan inside a bucket is a constant. The previous
//! insertion-ordered `Vec` + index had the same property and this keeps it —
//! what it did not have was bash's order.

use crate::bytes::{BStr, Str};

/// bash grows the table once it holds this many entries per bucket.
const LOAD_FACTOR: usize = 2;

/// …and multiplies the bucket count by this much when it does.
const GROW_BY: usize = 4;

/// Which of bash's hash tables this is.
///
/// bash makes several `hash_create` calls with different initial sizes and then
/// runs all of them through the same `hashlib.c`. The size is the *only* thing
/// that differs, but it decides every bucket index and therefore the whole
/// enumeration order, so a table built with the wrong one is wrong from its
/// first key. Each size below is measured, not read out of a header — see
/// `TD-OILS-THE-ALIAS-AND-COMMAND-MIRRORS-ARE-NOT-THEIR-OWN-TABLES` in
/// `known-issues.md` for the fits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TableShape {
    /// An associative array (`declare -A`). bash's `assoc_create (0)`.
    #[default]
    Assoc,
    /// The alias table, mirrored by `BASH_ALIASES`.
    Alias,
    /// The hashed-command table, mirrored by `BASH_CMDS` and listed by `hash`.
    Command,
}

impl TableShape {
    /// The bucket count a fresh table of this shape starts with.
    ///
    /// Always a power of two: the bucket is masked, not divided.
    const fn initial_buckets(self) -> usize {
        match self {
            Self::Assoc => 1024,
            Self::Alias => 64,
            Self::Command => 256,
        }
    }
}

/// bash's `hash_string` — FNV-1 over the key's bytes.
///
/// Multiply first, then xor: that is FNV-**1**, not the more common FNV-1a, and
/// the two give different orders. The byte is xored in as a *signed* char
/// (bash's parameter is a `char *`), so `\xff` contributes `0xffff_ffff` rather
/// than `0xff` — which is observable the moment a key holds a high byte.
fn hash_string(key: BStr<'_>) -> u32 {
    let mut i: u32 = 2_166_136_261;
    for &b in key {
        i = i.wrapping_mul(16_777_619);
        i ^= b as i8 as u32;
    }
    i
}

/// A map from byte-string keys to values that enumerates in bash's hash order.
///
/// Generic over the value because bash's other tables are the same structure
/// with a different payload: the alias table maps to replacement text, the
/// hashed-command table to a path and a hit count. Sharing the implementation is
/// not tidiness — it is the only way those tables enumerate correctly, since
/// their order is a property of *this* algorithm and nothing else.
///
/// The invariant every method here maintains: `nentries` is the number of
/// `(key, value)` pairs across all of `buckets`, no key appears in two chains,
/// and `buckets.len()` is either zero (no table yet — a bare `declare -A m`
/// that has never been written) or a power of two at least
/// `shape.initial_buckets()`.
/// Equality is *structural*, over the buckets rather than the key set: two
/// tables holding the same pairs compare equal only if they also enumerate the
/// same way. That is the right test for the parser's "did the alias table
/// change" check — a reordering it did not notice would expand aliases in the
/// wrong precedence — and it costs nothing, since a table can only be reordered
/// by a mutation the check wanted to catch anyway.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AssocArray<V = Str> {
    /// Which of bash's tables this is, and so how many buckets it starts with.
    /// Fixed at construction: changing it would invalidate every bucket index.
    shape: TableShape,
    /// The bucket array; each chain runs from its head, which is where a new
    /// entry is pushed. Empty until the first element is assigned, so an
    /// untouched `declare -A m` does not pay for a table it never uses.
    buckets: Vec<Vec<(Str, V)>>,
    /// The number of entries across every chain — what the load factor is
    /// measured against, and what `${#m[@]}` reports.
    nentries: usize,
}

impl<V: Default> AssocArray<V> {
    /// An array with no elements — a bare `declare -A m`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty table of one of bash's other shapes.
    #[must_use]
    pub fn with_shape(shape: TableShape) -> Self {
        Self { shape, buckets: Vec::new(), nentries: 0 }
    }

    /// How many elements the array holds (`${#m[@]}`).
    #[must_use]
    pub fn len(&self) -> usize {
        self.nentries
    }

    /// Whether the array holds no elements. Note that this does *not* say
    /// whether it was ever assigned: `declare -A m` and `m=()` are both empty
    /// and print differently, which the caller tracks separately.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nentries == 0
    }

    /// The bucket `key` belongs in, or `None` while there is no table.
    fn bucket_of(&self, key: BStr<'_>) -> Option<usize> {
        let n = self.buckets.len();
        // A power-of-two bucket count is the invariant that lets bash mask
        // rather than divide; `n - 1` is the mask.
        (n != 0).then(|| hash_string(key) as usize & (n - 1))
    }

    /// The chain `key` belongs in, and its position within it if it is there.
    fn find(&self, key: BStr<'_>) -> Option<(usize, usize)> {
        let b = self.bucket_of(key)?;
        let i = self.buckets.get(b)?.iter().position(|(k, _)| k.as_slice() == key)?;
        Some((b, i))
    }

    /// The value at `key`, or `None` if the array has no such key.
    #[must_use]
    pub fn get(&self, key: BStr<'_>) -> Option<&V> {
        let (b, i) = self.find(key)?;
        self.buckets.get(b)?.get(i).map(|(_, v)| v)
    }

    /// The value at `key` to write through, or `None` if there is no such key.
    ///
    /// Deliberately does *not* create the key: a caller that wants that says so
    /// with [`set`](Self::set), and one that is only bumping a hit count must
    /// not accidentally link a new entry and reorder the table.
    pub fn get_mut(&mut self, key: BStr<'_>) -> Option<&mut V> {
        let (b, i) = self.find(key)?;
        self.buckets.get_mut(b)?.get_mut(i).map(|(_, v)| v)
    }

    /// Whether `key` is present. An element may be present and empty, which is
    /// not the same as absent — `${m[k]+set}` tells them apart.
    #[must_use]
    pub fn contains_key(&self, key: BStr<'_>) -> bool {
        self.find(key).is_some()
    }

    /// The elements in bash's hash order: buckets by index, each chain from its
    /// head.
    pub fn iter(&self) -> std::iter::Flatten<std::slice::Iter<'_, Vec<(Str, V)>>> {
        self.buckets.iter().flatten()
    }

    /// A copy of this table with every chain reversed and every value mapped —
    /// the `BASH_ALIASES` / `BASH_CMDS` mirrors.
    ///
    /// The reversal is not a choice between two orders: bash has readers of both
    /// kinds over the very same table. `hash -l` walks the hashed-command chains
    /// from the head, as [`iter`](Self::iter) does, while the mirrors come out
    /// the other way — a table whose chain holds `bfs atj aoo aft` lists in that
    /// order and mirrors as `aft aoo atj bfs`. Reversing here, rather than at
    /// the reader, is what lets the mirror be an ordinary array that the
    /// ordinary readers (`${!m[@]}`, `declare -p`, `@K`, …) walk unaware.
    ///
    /// The shape is carried over, which is the whole point: a mirror rebuilt as
    /// a *default* table would be 1024 buckets and would enumerate in an order
    /// that exists nowhere in bash.
    pub fn mirrored<W>(&self, mut value: impl FnMut(&V) -> W) -> AssocArray<W> {
        AssocArray {
            shape: self.shape,
            buckets: self
                .buckets
                .iter()
                .map(|chain| chain.iter().rev().map(|(k, v)| (k.clone(), value(v))).collect())
                .collect(),
            nentries: self.nentries,
        }
    }

    /// The keys in hash order (`${!m[@]}`).
    pub fn keys(&self) -> impl Iterator<Item = &Str> {
        self.iter().map(|(k, _)| k)
    }

    /// The values in hash order (`${m[@]}`).
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, v)| v)
    }

    /// Assign `key`, replacing any value it already had.
    ///
    /// Re-assigning an existing key leaves it exactly where it is: bash writes
    /// through the entry it found rather than relinking it, and so does this.
    pub fn set(&mut self, key: Str, val: V) {
        self.slot(key).1 = val;
    }

    /// The `(key, value)` slot for `key`, created empty at the head of its
    /// chain if it is not already there. The single place a new key is
    /// introduced, so the entry count and the chains cannot drift apart.
    fn slot(&mut self, key: Str) -> &mut (Str, V) {
        let (b, i) = self.place(key);
        // `place` returns a position it has just linked an entry into or found
        // one at, so this cannot fail; going through `get_mut` keeps the method
        // from reaching for an index that might not be there.
        self.buckets
            .get_mut(b)
            .and_then(|chain| chain.get_mut(i))
            .expect("place returns an existing slot")
    }

    /// Where `key` lives, linking it in at the head of its chain if it does not
    /// live anywhere yet.
    ///
    /// The load factor is checked *before* the entry is linked, which is bash's
    /// order and matters: the entry that trips a growth is placed into the
    /// grown table rather than rehashed out of the old one. The comparison is
    /// `>=`, not `>`, so the 2049th key is the one that grows a 1024-bucket
    /// table — see [`grows_on_the_key_that_reaches_the_load_factor`].
    ///
    /// [`grows_on_the_key_that_reaches_the_load_factor`]: self::tests::grows_on_the_key_that_reaches_the_load_factor
    fn place(&mut self, key: Str) -> (usize, usize) {
        if let Some(found) = self.find(&key) {
            return found;
        }
        if self.buckets.is_empty() {
            self.buckets = empty_buckets(self.shape.initial_buckets());
        } else if self.nentries >= self.buckets.len().saturating_mul(LOAD_FACTOR) {
            self.grow();
        }
        // The table is non-empty by here, so there is a bucket; `0` is a
        // fallback for the impossible case that stays inside the array.
        let b = self.bucket_of(&key).unwrap_or(0);
        if let Some(chain) = self.buckets.get_mut(b) {
            chain.insert(0, (key, V::default()));
            self.nentries = self.nentries.saturating_add(1);
        }
        (b, 0)
    }

    /// Multiply the bucket count and rehash into it.
    ///
    /// bash walks the old buckets in index order and pushes each entry onto the
    /// *head* of its new chain, so a chain that survives intact comes out
    /// reversed. That is why growing a table reorders far more of it than the
    /// redistribution alone would, and why the final order depends on the
    /// insertion history rather than only on the key set.
    fn grow(&mut self) {
        let old = std::mem::take(&mut self.buckets);
        self.buckets = empty_buckets(old.len().saturating_mul(GROW_BY));
        for chain in old {
            for e in chain {
                // The new table is larger than the old, so it is non-empty and
                // `bucket_of` answers.
                let b = self.bucket_of(&e.0).unwrap_or(0);
                if let Some(dst) = self.buckets.get_mut(b) {
                    dst.insert(0, e);
                }
            }
        }
    }

    /// Drop every entry but keep the bucket array — bash's `hash_flush`.
    ///
    /// Distinct from [`reset`](Self::reset), and the difference is observable: a
    /// table that had grown keeps its grown bucket count here, so the keys put
    /// back after a `hash -r` land where a *larger* table puts them. bash flushes
    /// (not disposes) on `hash -r` and on a `$PATH` change.
    pub fn clear(&mut self) {
        for chain in &mut self.buckets {
            chain.clear();
        }
        self.nentries = 0;
    }

    /// Throw the table away entirely — bash's `hash_dispose`, after which the
    /// next insertion builds a fresh one at the shape's initial size.
    ///
    /// This is what `unalias -a` does, and what `m=()` does to an associative
    /// array. See [`clear`](Self::clear) for the one that does not.
    pub fn reset(&mut self) {
        self.buckets = Vec::new();
        self.nentries = 0;
    }

    /// Remove `key` if present, reporting whether there was one.
    ///
    /// Only the one entry is unlinked: everything else keeps its place, and the
    /// table never shrinks — so a key added after a removal lands wherever its
    /// hash puts it, not in the hole the removal left.
    pub fn remove(&mut self, key: BStr<'_>) -> bool {
        let Some((b, i)) = self.find(key) else {
            return false;
        };
        if let Some(chain) = self.buckets.get_mut(b) {
            chain.remove(i);
            self.nentries = self.nentries.saturating_sub(1);
        }
        true
    }
}

impl AssocArray<Str> {
    /// Append to `key` (`m[k]+=v`), starting from empty if it is absent.
    pub fn append(&mut self, key: Str, val: BStr<'_>) {
        self.slot(key).1.extend_from_slice(val);
    }
}

/// `n` empty chains.
///
/// Not `vec![Vec::new(); n]`: that clones a prototype and so would demand
/// `V: Clone` from every table, which the hashed-command table's payload need
/// not satisfy. Each chain here is independently new.
fn empty_buckets<V>(n: usize) -> Vec<Vec<(Str, V)>> {
    (0..n).map(|_| Vec::new()).collect()
}

impl<V: Default> FromIterator<(Str, V)> for AssocArray<V> {
    /// Build from `(key, value)` pairs, keeping the first position of a
    /// repeated key and its *last* value — the same rule `set` follows.
    fn from_iter<I: IntoIterator<Item = (Str, V)>>(iter: I) -> Self {
        let mut m = Self::new();
        for (k, v) in iter {
            m.set(k, v);
        }
        m
    }
}

impl<'a, V: Default> IntoIterator for &'a AssocArray<V> {
    type Item = &'a (Str, V);
    type IntoIter = std::iter::Flatten<std::slice::Iter<'a, Vec<(Str, V)>>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::AssocArray;

    fn keys(m: &AssocArray) -> Vec<String> {
        m.keys().map(|k| String::from_utf8_lossy(k).into_owned()).collect()
    }

    fn pairs(m: &AssocArray) -> Vec<(String, String)> {
        m.iter()
            .map(|(k, v)| {
                (String::from_utf8_lossy(k).into_owned(), String::from_utf8_lossy(v).into_owned())
            })
            .collect()
    }

    fn built(ks: &[&str]) -> AssocArray {
        ks.iter().map(|k| (k.as_bytes().to_vec(), k.to_uppercase().into_bytes())).collect()
    }

    /// Every expectation below is bash 5.2.37's own output for the same script.
    #[test]
    fn enumerates_in_bashs_hash_order_and_finds_by_key() {
        let m = built(&["b", "a", "c"]);
        assert_eq!(m.len(), 3);
        // bash: `declare -A m; for k in b a c; do m[$k]=1; done; echo ${!m[@]}`
        // prints `c b a` — not insertion order, and not sorted either.
        assert_eq!(
            pairs(&m),
            [("c", "C"), ("b", "B"), ("a", "A")]
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .to_vec()
        );
        assert_eq!(m.get(b"a").map(Vec::as_slice), Some(&b"A"[..]));
        assert_eq!(m.get(b"zz"), None);
        assert!(m.contains_key(b"c") && !m.contains_key(b"zz"));
    }

    #[test]
    fn reassigning_a_key_does_not_move_it() {
        let mut m = AssocArray::new();
        m.set(b"x".to_vec(), b"1".to_vec());
        m.set(b"y".to_vec(), b"2".to_vec());
        m.set(b"x".to_vec(), b"3".to_vec());
        // bash prints `y x` for these three, the write to `x` having changed
        // nothing but its value.
        assert_eq!(pairs(&m), vec![("y".into(), "2".into()), ("x".into(), "3".into())]);
        // Appending is the same rule, and starts from empty for a new key.
        m.append(b"y".to_vec(), b"tail");
        m.append(b"z".to_vec(), b"new");
        assert_eq!(
            pairs(&m),
            vec![
                ("z".into(), "new".into()),
                ("y".into(), "2tail".into()),
                ("x".into(), "3".into()),
            ]
        );
    }

    #[test]
    fn removing_unlinks_only_that_entry() {
        let mut m = built(&["a", "b", "c", "d"]);
        assert_eq!(keys(&m), ["d", "c", "b", "a"]);
        assert!(m.remove(b"b"));
        assert!(!m.remove(b"b"), "removing twice reports the second as absent");
        assert_eq!(m.len(), 3);
        assert_eq!(keys(&m), ["d", "c", "a"], "the survivors did not move");
        for k in ["a", "c", "d"] {
            assert_eq!(
                m.get(k.as_bytes()).map(Vec::as_slice),
                Some(k.to_uppercase().as_bytes()),
                "{k}"
            );
        }
        assert_eq!(m.get(b"b"), None);
        // A key added after a removal lands where its hash puts it, which for
        // `e` is ahead of all three — not in the hole `b` left.
        m.set(b"e".to_vec(), b"E".to_vec());
        assert_eq!(keys(&m), ["e", "d", "c", "a"]);
    }

    #[test]
    fn a_repeated_key_in_a_literal_keeps_its_first_place() {
        let m: AssocArray = [("k", "1"), ("j", "2"), ("k", "3")]
            .into_iter()
            .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
            .collect();
        assert_eq!(pairs(&m), vec![("k".into(), "3".into()), ("j".into(), "2".into())]);
    }

    /// A high byte is xored in sign-extended, so `\xff` and `A` come out in
    /// this order and not the other. bash agrees.
    #[test]
    fn a_high_byte_key_hashes_as_a_signed_char() {
        let m: AssocArray =
            [b"\xff".to_vec(), b"A".to_vec()].into_iter().map(|k| (k, b"v".to_vec())).collect();
        assert_eq!(m.keys().cloned().collect::<Vec<_>>(), vec![b"A".to_vec(), b"\xff".to_vec()]);
    }

    /// Forty sequential keys is short of any growth — this is the plain
    /// 1024-bucket walk, and a run like `key6 key7 key4 key5` is two collided
    /// pairs that each came out head-first.
    #[test]
    fn forty_sequential_keys_match_bash() {
        let m: AssocArray =
            (0..40).map(|i| (format!("key{i}").into_bytes(), b"v".to_vec())).collect();
        let want = "key39 key38 key37 key36 key35 key34 key33 key32 key31 key30 key6 key7 key4 \
                    key5 key2 key3 key0 key1 key8 key9 key28 key29 key24 key25 key26 key27 key20 \
                    key21 key22 key23 key15 key14 key17 key16 key11 key10 key13 key12 key19 key18";
        assert_eq!(keys(&m).join(" "), want);
    }

    /// 4200 keys crosses a growth (at 2049, 1024 buckets becoming 4096), so
    /// this pins the rehash walk as well as the load factor.
    #[test]
    fn growth_rehashes_the_way_bash_does() {
        let m: AssocArray =
            (0..4200).map(|i| (format!("key{i}").into_bytes(), b"v".to_vec())).collect();
        assert_eq!(m.len(), 4200);
        let ks = keys(&m);
        let head = [
            "key2479", "key2478", "key2475", "key2474", "key2477", "key2476", "key2471", "key2470",
        ];
        let tail = [
            "key3915", "key2130", "key3914", "key2131", "key3917", "key2132", "key3916", "key2133",
        ];
        assert_eq!(ks.get(..8), Some(&head.map(String::from)[..]));
        assert_eq!(ks.get(ks.len() - 8..), Some(&tail.map(String::from)[..]));
        // And every key is still reachable across the rehash.
        for i in 0..4200 {
            assert!(m.contains_key(format!("key{i}").as_bytes()), "key{i}");
        }
    }

    /// The growth is tripped by the key that *reaches* two per bucket.
    ///
    /// bash's test is `nentries >= nbuckets * LOAD_FACTOR`, so a 1024-bucket
    /// table grows while binding its **2049th** key, not its 2050th. The
    /// difference is one key's worth of rehashing and it is almost always
    /// invisible: at 4096 buckets and ~2000 entries most chains are a single
    /// entry, and a lone entry lands the same way whether it was rehashed into
    /// its bucket or inserted there. 2049 is one of the few sizes where it
    /// shows, which is exactly why this table wants pinning at the boundary and
    /// not merely far past it — [`growth_rehashes_the_way_bash_does`] above
    /// passes under *either* rule.
    ///
    /// Both expectations are bash 5.2.37's own output for the same key set.
    #[test]
    fn grows_on_the_key_that_reaches_the_load_factor() {
        // 2048 keys: exactly at the load factor, and still not grown.
        let m: AssocArray =
            (0..2048).map(|i| (format!("key{i}").into_bytes(), b"v".to_vec())).collect();
        let ks = keys(&m);
        let head = ["key899", "key898", "key891", "key890", "key893", "key892", "key895", "key894"];
        let tail =
            ["key1535", "key640", "key1534", "key641", "key1537", "key642", "key1536", "key643"];
        assert_eq!(ks.get(..8), Some(&head.map(String::from)[..]));
        assert_eq!(ks.get(ks.len() - 8..), Some(&tail.map(String::from)[..]));

        // 2049: the one key that grows it. Under `>` this order is wrong.
        let m: AssocArray =
            (0..2049).map(|i| (format!("key{i}").into_bytes(), b"v".to_vec())).collect();
        let ks = keys(&m);
        let head =
            ["key217", "key1104", "key216", "key1105", "key215", "key1106", "key214", "key1107"];
        let tail = [
            "key1207", "key1206", "key1205", "key1204", "key1203", "key1202", "key1201", "key1200",
        ];
        assert_eq!(ks.get(..8), Some(&head.map(String::from)[..]));
        assert_eq!(ks.get(ks.len() - 8..), Some(&tail.map(String::from)[..]));
        for i in 0..2049 {
            assert!(m.contains_key(format!("key{i}").as_bytes()), "key{i}");
        }
    }
}
