//! A growable array whose storage comes from this library's own `malloc`:
//! what `Vec` is to code with an allocator crate, for one that has none.
//! `posix` is `no_std` without `alloc` -- its `malloc` *is* the allocator --
//! so a module that needs to grow a table asks here.
//!
//! Every growth can fail and says so: [`List::push`] and the rest return
//! [`NoMem`] rather than aborting, because the C functions built on a list
//! (`regcomp`, `regexec`) answer an exhausted heap with an error code, as the
//! standard has them do.
//!
//! glob.rs, gai.rs and wordexp.rs each carry a private list of their own,
//! written before this one; known-issues.md ->
//! D-POSIX-PRIVATE-GROWABLE-ARRAYS has them move onto it.

use core::ptr::NonNull;

/// The heap had no room for a list's growth.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct NoMem;

/// A growable array from `malloc`; its elements are dropped with it.
pub(crate) struct List<T> {
    ptr: *mut T,
    len: usize,
    cap: usize,
}

impl<T> List<T> {
    /// An empty list, holding no block.
    pub(crate) const fn new() -> Self {
        Self {
            ptr: core::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }

    /// An empty list with room for `n` elements.
    pub(crate) fn with_capacity(n: usize) -> Result<Self, NoMem> {
        let mut l = Self::new();
        l.reserve(n)?;
        Ok(l)
    }

    /// Room for `more` elements beyond those held, the block doubled at least
    /// so that pushing one at a time costs amortised constant time.
    pub(crate) fn reserve(&mut self, more: usize) -> Result<(), NoMem> {
        let need = self.len.checked_add(more).ok_or(NoMem)?;
        if need <= self.cap {
            return Ok(());
        }
        let cap = need.max(self.cap.saturating_mul(2)).max(4);
        let size = core::mem::size_of::<T>().max(1);
        let bytes = cap.checked_mul(size).ok_or(NoMem)?;
        if bytes > isize::MAX.cast_unsigned() {
            return Err(NoMem);
        }
        // SAFETY: `ptr` is NULL or this list's own block, from `malloc` or
        // `realloc`; the elements are moved bytewise, which is how a Rust
        // value moves.
        let p = unsafe { crate::malloc::realloc(self.ptr.cast(), bytes) }.cast::<T>();
        if p.is_null() {
            return Err(NoMem);
        }
        self.ptr = p;
        self.cap = cap;
        Ok(())
    }

    /// `v` at the end.
    pub(crate) fn push(&mut self, v: T) -> Result<(), NoMem> {
        self.reserve(1)?;
        // SAFETY: `len < cap` after the reserve; the slot is past the
        // initialised ones, so nothing is overwritten.
        unsafe { self.ptr.add(self.len).write(v) };
        self.len = self.len.wrapping_add(1);
        Ok(())
    }

    /// The last element, taken out.
    pub(crate) fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        self.len = self.len.wrapping_sub(1);
        // SAFETY: element `len` was initialised and is no longer counted, so
        // it is read out exactly once.
        Some(unsafe { self.ptr.add(self.len).read() })
    }

    /// The elements held.
    pub(crate) fn as_slice(&self) -> &[T] {
        let p = if self.ptr.is_null() {
            NonNull::<T>::dangling().as_ptr()
        } else {
            self.ptr
        };
        // SAFETY: `len` initialised elements at `p` (none, at a dangling
        // but aligned pointer, for an empty list).
        unsafe { core::slice::from_raw_parts(p, self.len) }
    }

    /// The elements held, to change.
    pub(crate) fn as_mut_slice(&mut self) -> &mut [T] {
        let p = if self.ptr.is_null() {
            NonNull::<T>::dangling().as_ptr()
        } else {
            self.ptr
        };
        // SAFETY: as in `as_slice`; `&mut self` makes the borrow unique.
        unsafe { core::slice::from_raw_parts_mut(p, self.len) }
    }

    /// Only the first `n` elements kept; the rest dropped.
    pub(crate) fn truncate(&mut self, n: usize) {
        if !core::mem::needs_drop::<T>() {
            // Nothing to run for each: forgetting them is dropping them.
            self.len = self.len.min(n);
            return;
        }
        while self.len > n {
            self.len = self.len.wrapping_sub(1);
            // SAFETY: element `len`, initialised, no longer counted: dropped
            // once.
            unsafe { self.ptr.add(self.len).drop_in_place() };
        }
    }

    /// Every element dropped; the block kept for reuse.
    pub(crate) fn clear(&mut self) {
        self.truncate(0);
    }
}

impl<T: Copy> List<T> {
    /// `n` copies of `v`.
    pub(crate) fn filled(n: usize, v: T) -> Result<Self, NoMem> {
        let mut l = Self::with_capacity(n)?;
        while l.len < n {
            // SAFETY: `len < n <= cap`.
            unsafe { l.ptr.add(l.len).write(v) };
            l.len = l.len.wrapping_add(1);
        }
        Ok(l)
    }

    /// `s`'s elements at the end.
    pub(crate) fn extend_from_slice(&mut self, s: &[T]) -> Result<(), NoMem> {
        self.reserve(s.len())?;
        // SAFETY: room for `s` reserved past the initialised elements; a
        // borrowed `s` cannot be this list's own storage, which `&mut self`
        // holds uniquely.
        unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), self.ptr.add(self.len), s.len()) };
        self.len = self.len.wrapping_add(s.len());
        Ok(())
    }

    /// `n` elements, those past the present length copies of `v`.
    pub(crate) fn resize(&mut self, n: usize, v: T) -> Result<(), NoMem> {
        if n <= self.len {
            self.truncate(n);
            return Ok(());
        }
        self.reserve(n.wrapping_sub(self.len))?;
        while self.len < n {
            // SAFETY: `len < n <= cap`.
            unsafe { self.ptr.add(self.len).write(v) };
            self.len = self.len.wrapping_add(1);
        }
        Ok(())
    }
}

impl<T> core::ops::Deref for List<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T> core::ops::DerefMut for List<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T> Drop for List<T> {
    fn drop(&mut self) {
        self.clear();
        // SAFETY: `ptr` is NULL or this list's own block.
        unsafe { crate::malloc::free(self.ptr.cast()) };
    }
}

impl<T> Default for List<T> {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: a list owns its elements outright, as a `Vec` does: sending it
// sends them.
unsafe impl<T: Send> Send for List<T> {}
// SAFETY: sharing a list shares only `&[T]`.
unsafe impl<T: Sync> Sync for List<T> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_pops_and_truncates() {
        let mut l = List::new();
        assert!(l.is_empty());
        for i in 0..1000u32 {
            l.push(i).unwrap();
        }
        assert_eq!(l.len(), 1000);
        assert_eq!(l.get(999), Some(&999));
        assert_eq!(l.pop(), Some(999));
        l.truncate(10);
        assert_eq!(&l[..], &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        l.clear();
        assert_eq!(l.pop(), None);
    }

    #[test]
    fn filled_resized_and_extended() {
        let mut l = List::filled(3, 7u8).unwrap();
        l.extend_from_slice(b"ab").unwrap();
        assert_eq!(&l[..], b"\x07\x07\x07ab");
        l.resize(6, b'z').unwrap();
        assert_eq!(&l[..], b"\x07\x07\x07abz");
        l.resize(2, 0).unwrap();
        assert_eq!(&l[..], b"\x07\x07");
    }

    #[test]
    fn drops_what_it_holds() {
        use core::sync::atomic::{AtomicUsize, Ordering};
        static DROPPED: AtomicUsize = AtomicUsize::new(0);
        struct D;
        impl Drop for D {
            fn drop(&mut self) {
                DROPPED.fetch_add(1, Ordering::Relaxed);
            }
        }
        {
            let mut l = List::new();
            for _ in 0..5 {
                l.push(D).unwrap();
            }
            l.truncate(3);
            assert_eq!(DROPPED.load(Ordering::Relaxed), 2);
        }
        assert_eq!(DROPPED.load(Ordering::Relaxed), 5);
    }

    #[test]
    fn refuses_sizes_past_the_address_space() {
        let mut l: List<u64> = List::new();
        assert_eq!(l.reserve(usize::MAX / 4), Err(NoMem));
        assert_eq!(l.reserve(usize::MAX), Err(NoMem));
        assert!(l.is_empty());
    }
}
