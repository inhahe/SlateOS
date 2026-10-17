//! The kernel's hash functions as a socket (`AF_ALG`): a program binds a
//! socket to a hash by name (`sha256`, `crc32c`), accepts an operation
//! socket from it, sends data to that -- here with `sendfile` straight from
//! a file -- and reads the digest back. util-linux compares files this way
//! (`lib/fileeq.c`), and falls back to comparing bytes where the kernel has
//! no such socket, as SlateOS's has not.
//!
//! Off Unix every call answers [`ENOSYS`](crate::ENOSYS).

#[cfg(not(unix))]
use crate::ENOSYS;
#[cfg(unix)]
use crate::last_errno;

/// `AF_ALG`.
pub const AF_ALG: i32 = 38;
/// `SOCK_SEQPACKET`, the type an `AF_ALG` socket is.
#[cfg(unix)]
const SOCK_SEQPACKET: i32 = 5;

/// `struct sockaddr_alg`: the family, the type of algorithm (`"hash"`),
/// two words of features, and its name.
#[cfg(unix)]
#[repr(C)]
struct SockaddrAlg {
    salg_family: u16,
    salg_type: [u8; 14],
    salg_feat: u32,
    salg_mask: u32,
    salg_name: [u8; 64],
}

/// Declared as `netlink` declares the two it shares with it: an address
/// passed untyped.
#[cfg(unix)]
mod sys {
    unsafe extern "C" {
        pub fn socket(domain: i32, kind: i32, protocol: i32) -> i32;
        pub fn bind(fd: i32, addr: *const core::ffi::c_void, len: u32) -> i32;
        pub fn accept(fd: i32, addr: *mut core::ffi::c_void, len: *mut u32) -> i32;
        pub fn sendfile(out_fd: i32, in_fd: i32, offset: *mut i64, count: usize) -> isize;
    }
}

/// An open hash: the socket bound to it (`fd_api`) and the operation socket
/// accepted from it (`fd_cip`), both closed when it is dropped.
#[derive(Debug)]
pub struct Hash {
    api: i32,
    op: i32,
}

impl Drop for Hash {
    fn drop(&mut self) {
        // Sockets nothing was written through to be kept: a failed close
        // loses nothing.
        let _ = crate::fd::close(self.op);
        let _ = crate::fd::close(self.api);
    }
}

impl Hash {
    /// `socket (AF_ALG, SOCK_SEQPACKET, 0)`, bound to the hash called
    /// `kname`, and an operation socket accepted from it: `init_crypto_api`.
    ///
    /// # Errors
    ///
    /// `EAFNOSUPPORT` where the kernel has no such sockets, `ENOENT` for a
    /// hash it does not know, `ENAMETOOLONG` for a name of 64 bytes or
    /// more; [`ENOSYS`] off Unix.
    pub fn open(kname: &[u8]) -> Result<Self, i32> {
        #[cfg(unix)]
        {
            if kname.len() >= 64 {
                return Err(crate::ENAMETOOLONG);
            }
            let mut sa = SockaddrAlg {
                salg_family: 38,
                salg_type: [0; 14],
                salg_feat: 0,
                salg_mask: 0,
                salg_name: [0; 64],
            };
            if let Some(t) = sa.salg_type.get_mut(..4) {
                t.copy_from_slice(b"hash");
            }
            if let Some(n) = sa.salg_name.get_mut(..kname.len()) {
                n.copy_from_slice(kname);
            }
            // SAFETY: plain values; the call creates a descriptor or fails.
            let api = unsafe { sys::socket(AF_ALG, SOCK_SEQPACKET, 0) };
            if api < 0 {
                return Err(last_errno());
            }
            let len = u32::try_from(core::mem::size_of::<SockaddrAlg>()).unwrap_or(0);
            // SAFETY: `sa` is a whole `struct sockaddr_alg`, alive across
            // the call, which reads `len` bytes of it.
            if unsafe { sys::bind(api, (&raw const sa).cast(), len) } != 0 {
                let e = last_errno();
                let _ = crate::fd::close(api);
                return Err(e);
            }
            // SAFETY: null address and length: the peer is not asked for.
            let op = unsafe { sys::accept(api, core::ptr::null_mut(), core::ptr::null_mut()) };
            if op < 0 {
                let e = last_errno();
                let _ = crate::fd::close(api);
                return Err(e);
            }
            Ok(Hash { api, op })
        }
        #[cfg(not(unix))]
        {
            let _ = kname;
            Err(ENOSYS)
        }
    }

    /// `sendfile (fd_cip, fd, NULL, count)`: up to `count` bytes of `fd`,
    /// from where it stands, sent to the hash -- how many went.
    ///
    /// # Errors
    ///
    /// What `sendfile` reports; [`ENOSYS`] off Unix.
    pub fn send_file(&self, fd: i32, count: usize) -> Result<usize, i32> {
        #[cfg(unix)]
        {
            // SAFETY: a null offset: the file's own position is used and
            // advanced; nothing of ours is read or written.
            let n = unsafe { sys::sendfile(self.op, fd, core::ptr::null_mut(), count) };
            usize::try_from(n).map_err(|_| last_errno())
        }
        #[cfg(not(unix))]
        {
            let _ = (fd, count);
            Err(ENOSYS)
        }
    }

    /// The operation socket (`fd_cip`): what [`Hash::send_file`] sends
    /// to, and where a `read` then finds the digest of all it sent since
    /// the last one.
    #[must_use]
    pub fn op(&self) -> i32 {
        self.op
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    extern crate std;
    #[cfg(target_os = "linux")]
    use std::format;

    use super::*;

    #[test]
    fn the_address_is_the_kernel_s() {
        #[cfg(unix)]
        assert_eq!(core::mem::size_of::<SockaddrAlg>(), 88);
        assert!(Hash::open(&[b'x'; 64]).is_err(), "a name the address cannot hold");
    }

    /// Where the kernel has the socket (Linux), SHA-256 of "abc" is the
    /// standard's; where it has not, the open says so.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_is_hashed_by_the_kernel() {
        let Ok(h) = Hash::open(b"sha256") else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("libcall-alg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("abc");
        std::fs::write(&path, b"abc").unwrap();
        let c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        let fd = crate::fd::open_read(&c).unwrap();
        assert_eq!(h.send_file(fd, 1024), Ok(3));
        let mut digest = [0u8; 32];
        assert_eq!(crate::fd::read(h.op(), &mut digest), Ok(32));
        assert_eq!(digest[..4], [0xba, 0x78, 0x16, 0xbf]);
        crate::fd::close(fd).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
