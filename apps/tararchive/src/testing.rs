//! Real TAR archives for other crates' tests.
//!
//! The archive manager's tests and the file manager's each need a `.tar` to
//! point themselves at, and a builder in a `#[cfg(test)]` module is invisible
//! outside its crate. One copy, read back by this crate's own tests, is the
//! cure -- the argument `audiotags::testing` and `mediaprobe::testing` make.
//!
//! Plain POSIX ustar, which every reader reads. A test of a *particular*
//! shape of header -- GNU's long names, PAX records, base-256 numbers, a bad
//! checksum -- lays its blocks out with [`header`] in this crate's own tests.

/// A ustar header for `name` of type `flag`, `size` bytes, with a checksum
/// that adds up. `name` must fit the 100-byte field.
#[must_use]
pub fn header(name: &[u8], flag: u8, size: u64, link: &[u8]) -> Vec<u8> {
    let mut h = vec![0_u8; 512];
    put(&mut h, 0, name);
    put(&mut h, 100, b"0000644\0");
    put(&mut h, 108, b"0001750\0");
    put(&mut h, 116, b"0001750\0");
    put(&mut h, 124, format!("{size:011o}\0").as_bytes());
    put(&mut h, 136, b"14712316540\0");
    if let Some(f) = h.get_mut(156) {
        *f = flag;
    }
    put(&mut h, 157, link);
    put(&mut h, 257, b"ustar\x0000");
    put(&mut h, 265, b"slate");
    put(&mut h, 297, b"slate");
    seal(&mut h);
    h
}

/// Set `h`'s checksum to what its bytes sum to.
pub fn seal(h: &mut [u8]) {
    put(h, 148, b"        ");
    let sum: u32 = h.iter().take(512).map(|&b| u32::from(b)).sum();
    put(h, 148, format!("{sum:06o}\0 ").as_bytes());
}

/// `bytes` into `h` at `at`, as far as `h` goes.
fn put(h: &mut [u8], at: usize, bytes: &[u8]) {
    for (i, &b) in bytes.iter().enumerate() {
        if let Some(slot) = at.checked_add(i).and_then(|j| h.get_mut(j)) {
            *slot = b;
        }
    }
}

/// `data` padded to a whole number of blocks.
#[must_use]
pub fn padded(data: &[u8]) -> Vec<u8> {
    let mut d = data.to_vec();
    d.resize(data.len().div_ceil(512).saturating_mul(512), 0);
    d
}

/// A file member: its header and its bytes, padded.
#[must_use]
pub fn file(name: &str, data: &[u8]) -> Vec<u8> {
    let mut m = header(
        name.as_bytes(),
        b'0',
        u64::try_from(data.len()).unwrap_or(0),
        &[],
    );
    m.extend(padded(data));
    m
}

/// A directory member.
#[must_use]
pub fn dir(name: &str) -> Vec<u8> {
    header(name.as_bytes(), b'5', 0, &[])
}

/// An archive of `members`, closed by its two blocks of zeros.
#[must_use]
pub fn archive(members: &[Vec<u8>]) -> Vec<u8> {
    let mut a = members.concat();
    a.extend(std::iter::repeat_n(0, 1024));
    a
}

/// An archive of files: `(name, bytes)` each.
#[must_use]
pub fn tar(files: &[(&str, &[u8])]) -> Vec<u8> {
    archive(&files.iter().map(|(n, d)| file(n, d)).collect::<Vec<_>>())
}
