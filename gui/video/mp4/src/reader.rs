//! Reading a seekable source by position: big-endian numbers, bytes, and
//! the position kept, so that every box knows where it began. Every read is
//! bounded by the source's length, so a size claiming more than the file
//! holds is an error, not an allocation.

use std::io::{self, BufReader, Read, Seek, SeekFrom};

use crate::Error;

/// How much a [`Reader`] reads ahead at first: enough that reading the boxes
/// and a file's samples through takes few reads.
const READ_AHEAD: usize = 64 * 1024;

/// Reads a seekable source, keeping its position.
pub(crate) struct Reader<R> {
    /// The source, read ahead into a buffer. `None` only inside
    /// [`Reader::set_read_ahead`], while the buffer is changed.
    inner: Option<BufReader<R>>,
    pos: u64,
    len: u64,
}

impl<R: Read + Seek> Reader<R> {
    /// A reader at the start of `source`.
    pub(crate) fn new(mut source: R) -> Result<Self, Error> {
        let len = source.seek(SeekFrom::End(0))?;
        source.seek(SeekFrom::Start(0))?;
        Ok(Self {
            inner: Some(BufReader::with_capacity(READ_AHEAD, source)),
            pos: 0,
            len,
        })
    }

    fn source(&mut self) -> Result<&mut BufReader<R>, Error> {
        self.inner.as_mut().ok_or(Error::Io(io::ErrorKind::Other))
    }

    /// Read ahead `bytes` at a time from here on.
    ///
    /// # Errors
    ///
    /// When the source cannot be put back where reading is; reading goes on
    /// as it was.
    pub(crate) fn set_read_ahead(&mut self, bytes: usize) -> Result<(), Error> {
        let bytes = bytes.max(1);
        if self.inner.as_ref().is_some_and(|r| r.capacity() == bytes) {
            return Ok(());
        }
        let Some(mut old) = self.inner.take() else {
            return Err(Error::Io(io::ErrorKind::Other));
        };
        // The source back where reading is -- what was read ahead let go --
        // before it is taken out of its buffer.
        if let Err(e) = old.seek(SeekFrom::Start(self.pos)) {
            self.inner = Some(old);
            return Err(e.into());
        }
        self.inner = Some(BufReader::with_capacity(bytes, old.into_inner()));
        Ok(())
    }

    /// The position of the next byte.
    pub(crate) fn pos(&self) -> u64 {
        self.pos
    }

    /// The source's length.
    pub(crate) fn len(&self) -> u64 {
        self.len
    }

    /// How many bytes are left.
    pub(crate) fn remaining(&self) -> u64 {
        self.len.saturating_sub(self.pos)
    }

    /// Move to `pos`, which must lie within the source.
    pub(crate) fn seek_to(&mut self, pos: u64) -> Result<(), Error> {
        if pos > self.len {
            return Err(Error::Truncated);
        }
        let delta = i64::try_from(pos).ok().zip(i64::try_from(self.pos).ok());
        let source = self.source()?;
        match delta {
            // Within reach of the buffer: keep it.
            Some((to, from)) => source.seek_relative(to.wrapping_sub(from))?,
            None => {
                source.seek(SeekFrom::Start(pos))?;
            }
        }
        self.pos = pos;
        Ok(())
    }

    /// Move `n` bytes on, no further than the end.
    pub(crate) fn skip(&mut self, n: u64) -> Result<(), Error> {
        self.seek_to(self.pos.saturating_add(n).min(self.len))
    }

    /// Fill `buf`, or fail with [`Error::Truncated`].
    pub(crate) fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), Error> {
        let n = u64::try_from(buf.len()).map_err(|_| Error::Truncated)?;
        if n > self.remaining() {
            return Err(Error::Truncated);
        }
        self.source()?.read_exact(buf).map_err(|e| match e.kind() {
            io::ErrorKind::UnexpectedEof => Error::Truncated,
            kind => Error::Io(kind),
        })?;
        self.pos = self.pos.saturating_add(n);
        Ok(())
    }

    /// `n` bytes, refused past the source's end before anything is
    /// allocated.
    pub(crate) fn bytes(&mut self, n: u64) -> Result<Vec<u8>, Error> {
        if n > self.remaining() {
            return Err(Error::Truncated);
        }
        let mut v = vec![0u8; usize::try_from(n).map_err(|_| Error::Truncated)?];
        self.read_exact(&mut v)?;
        Ok(v)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let mut b = [0u8; N];
        self.read_exact(&mut b)?;
        Ok(b)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.array::<1>()?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    pub(crate) fn u24(&mut self) -> Result<u32, Error> {
        let [a, b, c] = self.array()?;
        Ok(u32::from_be_bytes([0, a, b, c]))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    /// A four-character code, as written.
    pub(crate) fn fourcc(&mut self) -> Result<[u8; 4], Error> {
        self.array()
    }
}
