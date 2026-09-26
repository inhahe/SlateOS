//! Real audio files for other crates' tests to read.
//!
//! # Why this is in the library and not in a test module
//!
//! The music player's tests and the explorer's each need a WAV or an MP3 to
//! point themselves at, and a builder in a `#[cfg(test)]` module is invisible
//! outside its own crate -- so each wrote its own WAV header and ID3 tag. A
//! fixture that is subtly wrong makes a correct reader look broken or, worse, a
//! broken one look correct; one copy, tested against the readers here
//! (`the_test_files_read_as_what_they_were_made_as`), is the cure.
//! `imagecodec::testing` is the same argument, for pictures.
//!
//! # What it deliberately is not
//!
//! Not an encoder. The sound is silence -- zero samples in a WAV, frames of
//! zeros in an MP3. What is real is every header and tag a reader looks at. A
//! test of how the readers take a *particular* shape of tag -- an encoding, a
//! version, a size that lies -- tests this crate, and lays its bytes out by
//! hand in this crate's own tests.

/// A length, as the `u32` a chunk's or a tag's header holds.
fn len32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// A PCM WAV file: `secs` seconds of silence at `rate` Hz, in `channels`
/// channels of `bits`-bit samples, with a `LIST`/`INFO` chunk of `info` --
/// `(b"INAM", "a title")`, `(b"IART", "an artist")`, `(b"IPRD", "an album")`
/// -- or none when `info` is empty.
#[must_use]
pub fn wav(rate: u32, channels: u16, bits: u16, secs: u32, info: &[(&[u8; 4], &str)]) -> Vec<u8> {
    let block = channels.saturating_mul(bits / 8);
    let byte_rate = rate.saturating_mul(u32::from(block));
    let mut fmt = Vec::with_capacity(16);
    fmt.extend_from_slice(&1u16.to_le_bytes()); // PCM
    fmt.extend_from_slice(&channels.to_le_bytes());
    fmt.extend_from_slice(&rate.to_le_bytes());
    fmt.extend_from_slice(&byte_rate.to_le_bytes());
    fmt.extend_from_slice(&block.to_le_bytes());
    fmt.extend_from_slice(&bits.to_le_bytes());
    let mut body = b"WAVE".to_vec();
    chunk(&mut body, b"fmt ", &fmt);
    if !info.is_empty() {
        let mut list = b"INFO".to_vec();
        for (id, text) in info {
            let mut value = text.as_bytes().to_vec();
            value.push(0);
            chunk(&mut list, id, &value);
        }
        chunk(&mut body, b"LIST", &list);
    }
    let samples = usize::try_from(byte_rate.saturating_mul(secs)).unwrap_or(0);
    chunk(&mut body, b"data", &vec![0; samples]);
    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&len32(body.len()).to_le_bytes());
    file.extend(body);
    file
}

/// A RIFF chunk onto `out`: its id, its length, its bytes, and the pad byte
/// an odd length takes.
fn chunk(out: &mut Vec<u8>, id: &[u8; 4], bytes: &[u8]) {
    out.extend_from_slice(id);
    out.extend_from_slice(&len32(bytes.len()).to_le_bytes());
    out.extend_from_slice(bytes);
    if !bytes.len().is_multiple_of(2) {
        out.push(0);
    }
}

/// `count` MPEG-1 layer III frames at 128 kbps, 44.1 kHz, stereo, 417 bytes
/// each, whose sound is zeros. A reader times a file of them by its size:
/// 417 bytes at 128 kbps is 26.0625 ms, so `count` frames play for
/// `count` times 0.0260625 seconds.
#[must_use]
pub fn mp3_frames(count: usize) -> Vec<u8> {
    // 144 * 128000 / 44100 = 417 bytes a frame, no padding.
    let mut frame = vec![0xFF, 0xFB, 0x90, 0x00];
    frame.resize(417, 0);
    frame.repeat(count)
}

/// An MP3 file: an ID3v2.4 tag of UTF-8 text frames `tags` -- `(b"TIT2", "a
/// title")`, `(b"TPE1", "an artist")`, `(b"TALB", "an album")` -- then
/// [`mp3_frames`]`(frames)`. No tag at all when `tags` is empty.
#[must_use]
pub fn mp3(frames: usize, tags: &[(&[u8; 4], &str)]) -> Vec<u8> {
    let mut file = Vec::new();
    if !tags.is_empty() {
        let mut body = Vec::new();
        for (id, text) in tags {
            let mut value = vec![3]; // UTF-8
            value.extend_from_slice(text.as_bytes());
            body.extend_from_slice(*id);
            body.extend_from_slice(&synchsafe_bytes(value.len()));
            body.extend_from_slice(&[0, 0]); // no flags
            body.extend(value);
        }
        file.extend_from_slice(b"ID3\x04\x00\x00");
        file.extend_from_slice(&synchsafe_bytes(body.len()));
        file.extend(body);
    }
    file.extend(mp3_frames(frames));
    file
}

/// `n` as the four bytes of seven bits each an ID3v2.4 size is written in.
fn synchsafe_bytes(n: usize) -> [u8; 4] {
    let n = len32(n);
    let byte = |shift: u32| u8::try_from(n.checked_shr(shift).unwrap_or(0) & 0x7F).unwrap_or(0);
    [byte(21), byte(14), byte(7), byte(0)]
}
