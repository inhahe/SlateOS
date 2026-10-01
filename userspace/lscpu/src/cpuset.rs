//! `lib/cpuset.c`: sets of CPUs, and the two ways the kernel writes one --
//! a list (`0-3,8,10-11`) and a hex mask (`00000f0d`, comma-separated in
//! groups of eight digits in `/sys`).
//!
//! A [`CpuSet`] is glibc's `cpu_set_t` as `CPU_ALLOC(ncpus)` makes it: as
//! many bits as `CPU_ALLOC_SIZE(ncpus)` bytes hold, which is `ncpus` rounded
//! up to a multiple of 64, and every operation bounds-checked against that
//! as the `_S` macros are -- a CPU past the end is never set and never
//! found. Storage grows only as far as the highest bit actually set, so a
//! `kernel_max` of two billion costs nothing until something is put in it.
//!
//! The parsers are upstream's scanners transcribed, because their edges
//! show in what `lscpu` prints for an odd `/sys`: `cpulist_parse` ignores
//! junk before a comma (`0x,1` is CPUs 0 and 1) and refuses it at the end,
//! and a mask's commas are skipped one at a time, so `f,,f` is refused.

/// `__NCPUBITS`: bits in one `__cpu_mask`.
const NCPUBITS: usize = 64;

/// `CPU_ALLOC_SIZE(count)`: the bytes a set for `count` CPUs takes --
/// `count` rounded up to whole 64-bit words.
#[must_use]
pub fn alloc_size(count: usize) -> usize {
    count.div_ceil(NCPUBITS).saturating_mul(8)
}

/// A `cpu_set_t` of a fixed size.
#[derive(Clone, Debug)]
pub struct CpuSet {
    /// `cpuset_nbits(setsize)`: bits the set holds; `CPU_SET_S` of anything
    /// at or past it does nothing.
    nbits: usize,
    /// The bits, lowest first. Past the end of this, every bit is clear.
    words: Vec<u64>,
}

impl PartialEq for CpuSet {
    /// `CPU_EQUAL_S`: the same bits. (Every set `lscpu` compares was made
    /// for the same `maxcpus`, so the sizes agree.)
    fn eq(&self, other: &Self) -> bool {
        self.nbits == other.nbits && trimmed(&self.words) == trimmed(&other.words)
    }
}

impl Eq for CpuSet {}

/// `words` without its trailing zero words.
fn trimmed(words: &[u64]) -> &[u64] {
    let len = words
        .iter()
        .rposition(|&w| w != 0)
        .map_or(0, |i| i.saturating_add(1));
    words.get(..len).unwrap_or_default()
}

impl CpuSet {
    /// `cpuset_alloc(ncpus)` followed by `CPU_ZERO_S`: an empty set sized
    /// for `ncpus` CPUs.
    #[must_use]
    pub fn new(ncpus: usize) -> Self {
        CpuSet {
            nbits: alloc_size(ncpus).saturating_mul(8),
            words: Vec::new(),
        }
    }

    /// `cpuset_nbits(setsize)`.
    #[must_use]
    pub fn nbits(&self) -> usize {
        self.nbits
    }

    /// `CPU_ISSET_S(cpu, setsize, set)`.
    #[must_use]
    pub fn is_set(&self, cpu: usize) -> bool {
        cpu < self.nbits
            && self
                .words
                .get(cpu / NCPUBITS)
                .is_some_and(|&w| w & (1u64 << (cpu % NCPUBITS)) != 0)
    }

    /// `CPU_SET_S(cpu, setsize, set)`: nothing for a CPU past the end.
    pub fn set(&mut self, cpu: usize) {
        if cpu >= self.nbits {
            return;
        }
        let word = cpu / NCPUBITS;
        if self.words.len() <= word {
            self.words.resize(word.saturating_add(1), 0);
        }
        if let Some(w) = self.words.get_mut(word) {
            *w |= 1u64 << (cpu % NCPUBITS);
        }
    }

    /// `CPU_COUNT_S(setsize, set)`.
    #[must_use]
    pub fn count(&self) -> usize {
        self.words
            .iter()
            .map(|w| usize::try_from(w.count_ones()).unwrap_or(0))
            .sum()
    }

    /// The set CPUs, lowest first.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.words.iter().enumerate().flat_map(|(i, &w)| {
            (0..NCPUBITS)
                .filter(move |b| w & (1u64 << b) != 0)
                .map(move |b| i.saturating_mul(NCPUBITS).saturating_add(b))
        })
    }

    /// The highest set CPU.
    fn highest(&self) -> Option<usize> {
        let (i, &w) = self
            .words
            .iter()
            .enumerate()
            .rev()
            .find(|&(_, &w)| w != 0)?;
        let bit = usize::try_from(63u32.saturating_sub(w.leading_zeros())).unwrap_or(0);
        Some(i.saturating_mul(NCPUBITS).saturating_add(bit))
    }

    /// The four bits from `cpu` up, as one hex digit's value.
    fn nibble(&self, cpu: usize) -> u8 {
        (0..4usize).fold(0u8, |v, k| {
            if self.is_set(cpu.saturating_add(k)) {
                v | (1u8 << k)
            } else {
                v
            }
        })
    }
}

/// `nexttoken(q, sep)`: the position just past the next `sep` at or after
/// `q`, if there is one.
fn nexttoken(s: &[u8], q: Option<usize>, sep: u8) -> Option<usize> {
    let q = q?;
    let found = s.get(q..)?.iter().position(|&b| b == sep)?;
    Some(q.saturating_add(found).saturating_add(1))
}

/// `nextnumber(str, &end, &result)`: a decimal number starting exactly at
/// `at` -- no blanks, no sign -- as `strtoul` reads it, cut to `unsigned
/// int`. `end` moves past it only on success, as upstream's does; `None` is
/// its `-EINVAL` or `-ERANGE`.
fn nextnumber(s: &[u8], at: Option<usize>, end: &mut Option<usize>) -> Option<u32> {
    let at = at?;
    if !s.get(at).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let digits = s
        .get(at..)?
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    let mut value: u64 = 0;
    for &d in s.get(at..at.saturating_add(digits))? {
        // `strtoul` overflowing an unsigned long is ERANGE.
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(u64::from(d.wrapping_sub(b'0'))))?;
    }
    *end = Some(at.saturating_add(digits));
    // `(unsigned int) strtoul(...)`: the low 32 bits.
    Some(value as u32)
}

/// `while (a <= b) { CPU_SET_S(a, ...); a += s; }` with `a`, `b` and `s`
/// all `unsigned int`: `a += s` wraps, and the walk goes on from the bottom
/// until it lands past `b` -- which never happens when no number the walk
/// can reach lies past `b` (a range ending at `4294967295`), and upstream
/// hangs. Here that case sets what the walk would eventually have set and
/// stops; every other walk sets exactly what upstream's does, without
/// stepping through the stretches where nothing can be set.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "the u64 sums stay below 2^33; the u32 steps are checked or wrap deliberately, as upstream's do"
)]
fn walk_range(set: &mut CpuSet, first: u32, b: u32, stride: u32, max: usize) {
    let below_max = |a: u32| usize::try_from(a).is_ok_and(|a| a < max);
    // The walk visits every number congruent to `first` modulo the largest
    // power of two dividing the stride, and nothing else.
    let g = 1u64 << stride.trailing_zeros();
    let largest = (1u64 << 32) - g + u64::from(first) % g;
    if largest <= u64::from(b) {
        let mut x = u64::from(first) % g;
        while x < (1u64 << 32) && usize::try_from(x).is_ok_and(|x| x < max) {
            set.set(usize::try_from(x).unwrap_or(usize::MAX));
            x += g;
        }
        return;
    }
    // Invariant at the top of the loop: `a <= b`.
    let mut a = first;
    loop {
        if below_max(a) {
            set.set(usize::try_from(a).unwrap_or(usize::MAX));
            match a.checked_add(stride) {
                Some(next) if next > b => return,
                Some(next) => a = next,
                None => {
                    a = a.wrapping_add(stride);
                    if a > b {
                        return;
                    }
                }
            }
        } else {
            // Nothing more is set before the wrap: go straight to the last
            // step before it, which ends the walk if it is past `b`.
            let last = a + (u32::MAX - a) / stride * stride;
            if last > b {
                return;
            }
            a = last.wrapping_add(stride);
            if a > b {
                return;
            }
        }
    }
}

/// Why a list or a mask was refused: upstream's `1` and `-1`, which callers
/// turn into `EINVAL`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseError;

/// `cpulist_parse(str, set, setsize, 0)`: a list such as `0-3,5,8-15:2`
/// into a set for `ncpus` CPUs. CPUs that do not fit are ignored, as with
/// `fail` clear.
///
/// A range whose end is `4294967295` never ends upstream: `a += s` wraps
/// before `a <= b` can fail, and `lscpu` hangs. Here the walk stops when it
/// would wrap.
///
/// # Errors
///
/// What upstream refuses: a token not starting with a digit, a number past
/// `ULONG_MAX`, a stride of 0, a range running backwards, and anything left
/// after the last number.
pub fn cpulist_parse(s: &[u8], ncpus: usize) -> Result<CpuSet, ParseError> {
    let mut set = CpuSet::new(ncpus);
    let max = set.nbits();
    let mut q = Some(0usize);
    let mut end: Option<usize> = None;
    loop {
        let p = q;
        q = nexttoken(s, q, b',');
        let Some(p) = p else {
            break;
        };
        let a = nextnumber(s, Some(p), &mut end).ok_or(ParseError)?;
        let mut b = a;
        let mut stride: u32 = 1;
        let after = end;
        let c1 = nexttoken(s, after, b'-');
        let c2 = nexttoken(s, after, b',');
        if c1.is_some() && (c2.is_none() || c1 < c2) {
            b = nextnumber(s, c1, &mut end).ok_or(ParseError)?;
            let c1 = match end {
                Some(e) if s.get(e).is_some() => nexttoken(s, Some(e), b':'),
                _ => None,
            };
            if c1.is_some() && (c2.is_none() || c1 < c2) {
                stride = nextnumber(s, c1, &mut end).ok_or(ParseError)?;
                if stride == 0 {
                    return Err(ParseError);
                }
            }
        }
        if a > b {
            return Err(ParseError);
        }
        walk_range(&mut set, a, b, stride, max);
    }
    if end.is_some_and(|e| s.get(e).is_some()) {
        return Err(ParseError);
    }
    Ok(set)
}

/// `char_to_val(c)`: a hex digit's value.
fn char_to_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(c.wrapping_sub(b'a').wrapping_add(10)),
        b'A'..=b'F' => Some(c.wrapping_sub(b'A').wrapping_add(10)),
        _ => None,
    }
}

/// `cpumask_parse(str, set, setsize)`: a hex mask, lowest digit last, into a
/// set for `ncpus` CPUs. A `0x` in front is skipped, and so is one comma
/// before each digit.
///
/// A comma at the very front makes upstream read the byte before the
/// string; it is refused here.
///
/// # Errors
///
/// A byte that is not a hex digit, or two commas in a row.
pub fn cpumask_parse(s: &[u8], ncpus: usize) -> Result<CpuSet, ParseError> {
    let mut set = CpuSet::new(ncpus);
    let start = if s.len() > 1 && s.starts_with(b"0x") {
        2
    } else {
        0
    };
    let mut ptr = s.len();
    let mut cpu = 0usize;
    // `ptr` here is one past upstream's pointer: `ptr >= str` is `ptr > start`.
    while ptr > start {
        let mut at = ptr.saturating_sub(1);
        if s.get(at) == Some(&b',') {
            if at == 0 {
                return Err(ParseError);
            }
            at = at.saturating_sub(1);
        }
        let val = s.get(at).copied().and_then(char_to_val).ok_or(ParseError)?;
        for k in 0..4usize {
            if val & (1u8 << k) != 0 {
                set.set(cpu.saturating_add(k));
            }
        }
        ptr = at;
        cpu = cpu.saturating_add(4);
    }
    Ok(set)
}

/// `cpulist_create(str, len, set, setsize)`: the set as a list, ranges of
/// three or more CPUs as `a-b` and pairs spelled out -- or `None` when it
/// does not fit in `len` bytes with its terminating NUL, which is what
/// upstream then prints as `(null)`.
#[must_use]
pub fn cpulist_create(set: &CpuSet, len: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut room = len;
    let mut cpus = set.iter().peekable();
    while let Some(first) = cpus.next() {
        let mut last = first;
        while cpus.peek() == Some(&last.saturating_add(1)) {
            last = last.saturating_add(1);
            cpus.next();
        }
        let piece = match last.saturating_sub(first) {
            0 => format!("{first},"),
            1 => format!("{first},{last},"),
            _ => format!("{first}-{last},"),
        };
        if piece.len() >= room {
            return None;
        }
        room = room.saturating_sub(piece.len());
        out.extend_from_slice(piece.as_bytes());
    }
    // The comma after the last entry.
    out.pop();
    Some(out)
}

/// `cpumask_create(str, len, set, setsize)`: the set as hex digits, highest
/// first and without leading zeros -- `0` for an empty set. Upstream writes
/// the digits from the top of the set down and stops after `len` of them, so
/// a buffer too small for the whole mask shows only its top.
#[must_use]
pub fn cpumask_create(set: &CpuSet, len: usize) -> Vec<u8> {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let total = set.nbits() / 4;
    let written = total.min(len);
    // The digits are numbered from the top: digit k covers the four CPUs
    // from `nbits - 4 - 4k` up.
    let Some(high) = set.highest() else {
        return b"0".to_vec();
    };
    let first = total.saturating_sub(1).saturating_sub(high / 4);
    if first >= written {
        return b"0".to_vec();
    }
    (first..written)
        .map(|k| {
            let cpu = set
                .nbits()
                .saturating_sub(4)
                .saturating_sub(k.saturating_mul(4));
            DIGITS
                .get(usize::from(set.nibble(cpu)))
                .copied()
                .unwrap_or(b'0')
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(s: &str) -> Option<Vec<usize>> {
        cpulist_parse(s.as_bytes(), 2048)
            .ok()
            .map(|set| set.iter().collect())
    }

    #[test]
    fn sizes_round_up_to_words() {
        assert_eq!(alloc_size(1), 8);
        assert_eq!(alloc_size(64), 8);
        assert_eq!(alloc_size(65), 16);
        assert_eq!(CpuSet::new(4).nbits(), 64);
        let mut set = CpuSet::new(4);
        // Past `ncpus` but inside the word: set, as upstream's is.
        set.set(63);
        set.set(64);
        assert!(set.is_set(63));
        assert!(!set.is_set(64));
        assert_eq!(set.count(), 1);
    }

    #[test]
    fn lists_parse_as_upstream() {
        assert_eq!(list("0-3,5"), Some(vec![0, 1, 2, 3, 5]));
        assert_eq!(list("0-10:3"), Some(vec![0, 3, 6, 9]));
        assert_eq!(list("7"), Some(vec![7]));
        // Junk before a comma is never looked at; junk at the end is.
        assert_eq!(list("0x,1"), Some(vec![0, 1]));
        assert_eq!(list("0-3x"), None);
        assert_eq!(list(""), None);
        assert_eq!(list("3-1"), None);
        assert_eq!(list("0-4:0"), None);
        assert_eq!(list(",1"), None);
        assert_eq!(list("1,"), None);
        assert_eq!(list("-1"), None);
        // Too large for the set: ignored, not refused.
        assert_eq!(list("2047-2049"), Some(vec![2047]));
        // Past ULONG_MAX is ERANGE; past UINT_MAX is cut to 32 bits.
        assert_eq!(list("99999999999999999999"), None);
        assert_eq!(list("4294967297"), Some(vec![1]));
        // Wraps, and goes on from the bottom until it lands past the end.
        assert_eq!(list("4294967290-4294967294:3"), {
            // 4294967290, 4294967293, then 0, 3, 6, ... below 2048.
            Some((0..2048).step_by(3).collect())
        });
        // 4294967293 + 4 wraps to 1, and every number the walk can reach is
        // 1 more than a multiple of 4 -- none past the end, so upstream
        // never ends. Here: what it would have set by then.
        assert_eq!(
            list("4294967289-4294967295:4"),
            Some((1..2048).step_by(4).collect())
        );
        assert_eq!(list("4294967290-4294967295"), Some((0..2048).collect()));
    }

    #[test]
    fn masks_parse_as_upstream() {
        let mask = |s: &str| {
            cpumask_parse(s.as_bytes(), 2048)
                .ok()
                .map(|set| set.iter().collect::<Vec<_>>())
        };
        assert_eq!(mask("f"), Some(vec![0, 1, 2, 3]));
        assert_eq!(mask("0x5"), Some(vec![0, 2]));
        assert_eq!(mask("00000001,00000000"), Some(vec![32]));
        assert_eq!(mask("0x"), Some(vec![]));
        assert_eq!(mask("f,,f"), None);
        assert_eq!(mask(",f"), None);
        assert_eq!(mask("g"), None);
        assert_eq!(mask("A"), Some(vec![1, 3]));
    }

    #[test]
    fn lists_and_masks_print_as_upstream() {
        let set = cpulist_parse(b"0-2,4,5,7", 2048).unwrap_or_else(|_| CpuSet::new(2048));
        assert_eq!(cpulist_create(&set, 100), Some(b"0-2,4,5,7".to_vec()));
        // "0-2,4,5,7," is ten bytes, and each piece must leave room for
        // the NUL: eleven.
        assert_eq!(cpulist_create(&set, 10), None);
        assert_eq!(cpulist_create(&set, 11), Some(b"0-2,4,5,7".to_vec()));
        assert_eq!(cpulist_create(&CpuSet::new(8), 10), Some(Vec::new()));
        assert_eq!(cpumask_create(&set, 1000), b"b7".to_vec());
        assert_eq!(cpumask_create(&CpuSet::new(8), 1000), b"0".to_vec());
        // Only the top of a mask too long for the buffer.
        let mut top = CpuSet::new(64);
        top.set(0);
        assert_eq!(cpumask_create(&top, 7), b"0".to_vec());
        top.set(63);
        assert_eq!(cpumask_create(&top, 7), b"8000000".to_vec());
    }

    #[test]
    fn equality_ignores_storage() {
        let mut a = CpuSet::new(128);
        let mut b = CpuSet::new(128);
        a.set(100);
        a.words.push(0);
        b.set(100);
        assert_eq!(a, b);
        b.set(1);
        assert_ne!(a, b);
    }
}
