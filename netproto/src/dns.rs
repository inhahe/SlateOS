//! DNS (RFC 1035) message construction and response parsing.
//!
//! Enough to drive a resolver: build a standard recursive `A`/`AAAA` query and
//! walk the answer section of a response, transparently following the
//! compression pointers (RFC 1035 §4.1.4) that appear in answer names. This is
//! allocation-free — queries are written into a caller buffer and answers
//! borrow the response buffer. The message rides inside a UDP datagram (see
//! [`crate::udp`]).
//!
//! Forward resolution walks the answer section by *skipping* names (via the
//! compression pointers) to reach record data. Reverse resolution (`PTR`)
//! additionally *decodes* a name into a dotted ASCII string — see
//! [`read_name`] and [`Message::first_ptr`] — with a jump-count guard against
//! maliciously self-referential compression pointers.

/// Fixed DNS header length (id, flags, and the four section counts).
pub const HEADER_LEN: usize = 12;

/// Resource-record type: IPv4 address (`A`).
pub const TYPE_A: u16 = 1;
/// Resource-record type: IPv6 address (`AAAA`).
pub const TYPE_AAAA: u16 = 28;
/// Resource-record type: pointer / reverse name (`PTR`).
pub const TYPE_PTR: u16 = 12;
/// Resource-record class: Internet (`IN`).
pub const CLASS_IN: u16 = 1;

/// Upper bound on compression-pointer jumps while decoding a name. RFC 1035
/// names are at most 255 bytes, so a legitimate name needs far fewer jumps;
/// this cap stops a self-referential pointer chain from looping forever.
const MAX_NAME_JUMPS: u32 = 128;

/// Maximum length of a single DNS label (RFC 1035 §2.3.4).
const MAX_LABEL: usize = 63;

/// A parsed DNS message header plus a borrow of the whole message (needed to
/// resolve compression pointers when walking the answer section).
#[derive(Debug, Clone, Copy)]
pub struct Message<'a> {
    /// Transaction id (echoes the query's id in a response).
    pub id: u16,
    /// Flags word (QR/Opcode/AA/TC/RD/RA/RCODE).
    pub flags: u16,
    /// Question count.
    pub qdcount: u16,
    /// Answer count.
    pub ancount: u16,
    /// Authority record count.
    pub nscount: u16,
    /// Additional record count.
    pub arcount: u16,
    buf: &'a [u8],
}

/// One resource record from the answer section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answer<'a> {
    /// Record type (e.g. [`TYPE_A`]).
    pub atype: u16,
    /// Record class (e.g. [`CLASS_IN`]).
    pub class: u16,
    /// Time-to-live, in seconds.
    pub ttl: u32,
    /// Record data (for `A`: 4 bytes; for `AAAA`: 16 bytes).
    pub rdata: &'a [u8],
}

/// Skip a (possibly compressed) name starting at `off`, returning the offset of
/// the byte just after it. A compression pointer terminates the name in two
/// bytes (RFC 1035 §4.1.4). Returns `None` on any out-of-bounds read.
fn skip_name(buf: &[u8], mut off: usize) -> Option<usize> {
    loop {
        let len = *buf.get(off)?;
        if len & 0xC0 == 0xC0 {
            // Pointer: two bytes total; ensure the second is present.
            buf.get(off.checked_add(1)?)?;
            return off.checked_add(2);
        }
        if len == 0 {
            return off.checked_add(1); // root terminator
        }
        // A normal label: 1 length byte + `len` label bytes.
        off = off.checked_add(1 + len as usize)?;
        if off > buf.len() {
            return None;
        }
    }
}

/// Decode a (possibly compressed) DNS name at offset `start` in `buf` into a
/// dotted-ASCII string written to `out` (no trailing dot, no NUL). Returns the
/// number of bytes written, or `None` on a malformed name, an out-of-bounds
/// read, an over-long / self-referential pointer chain, or if `out` is too
/// small. Compression pointers (RFC 1035 §4.1.4) are followed transparently.
#[must_use]
pub fn read_name(buf: &[u8], start: usize, out: &mut [u8]) -> Option<usize> {
    let mut off = start;
    let mut written = 0usize;
    let mut jumps = 0u32;
    let mut first = true;
    loop {
        let len = *buf.get(off)?;
        if len & 0xC0 == 0xC0 {
            // Compression pointer: 14-bit offset from the low bits + next byte.
            let b2 = *buf.get(off.checked_add(1)?)?;
            let ptr = (((len & 0x3F) as usize) << 8) | b2 as usize;
            jumps = jumps.checked_add(1)?;
            if jumps > MAX_NAME_JUMPS {
                return None;
            }
            off = ptr;
            continue;
        }
        if len == 0 {
            return Some(written); // root terminator: name complete
        }
        let label_len = len as usize;
        let label_start = off.checked_add(1)?;
        let label_end = label_start.checked_add(label_len)?;
        if label_end > buf.len() {
            return None;
        }
        if !first {
            *out.get_mut(written)? = b'.';
            written = written.checked_add(1)?;
        }
        first = false;
        for i in 0..label_len {
            let c = *buf.get(label_start.checked_add(i)?)?;
            *out.get_mut(written)? = c;
            written = written.checked_add(1)?;
        }
        off = label_end;
    }
}

impl<'a> Message<'a> {
    /// True if this is a response (QR bit set).
    #[must_use]
    pub fn is_response(&self) -> bool {
        (self.flags & 0x8000) != 0
    }

    /// True if the truncation (TC) bit is set — the reply was cut short and
    /// should be retried over TCP.
    #[must_use]
    pub fn truncated(&self) -> bool {
        (self.flags & 0x0200) != 0
    }

    /// Response code (RCODE): 0 = no error, 3 = NXDOMAIN, etc.
    #[must_use]
    pub fn rcode(&self) -> u8 {
        (self.flags & 0x000F) as u8
    }

    /// Parse a DNS message header. Returns `None` if the buffer is shorter than
    /// the 12-byte header.
    #[must_use]
    pub fn parse(buf: &'a [u8]) -> Option<Self> {
        if buf.len() < HEADER_LEN {
            return None;
        }
        Some(Message {
            id: u16::from_be_bytes([buf[0], buf[1]]),
            flags: u16::from_be_bytes([buf[2], buf[3]]),
            qdcount: u16::from_be_bytes([buf[4], buf[5]]),
            ancount: u16::from_be_bytes([buf[6], buf[7]]),
            nscount: u16::from_be_bytes([buf[8], buf[9]]),
            arcount: u16::from_be_bytes([buf[10], buf[11]]),
            buf,
        })
    }

    /// Offset of the answer section, i.e. just past the question section.
    /// Returns `None` if a question name/record runs off the buffer.
    fn answers_start(&self) -> Option<usize> {
        let mut off = HEADER_LEN;
        for _ in 0..self.qdcount {
            off = skip_name(self.buf, off)?;
            // Each question ends with qtype (2) + qclass (2).
            off = off.checked_add(4)?;
            if off > self.buf.len() {
                return None;
            }
        }
        Some(off)
    }

    /// Iterate the answer-section resource records.
    #[must_use]
    pub fn answers(&self) -> Answers<'a> {
        match self.answers_start() {
            Some(off) => Answers { buf: self.buf, off, remaining: self.ancount },
            None => Answers { buf: self.buf, off: 0, remaining: 0 },
        }
    }

    /// Copy the first `A` (IPv4) record's address into `out`, returning `true`
    /// if one was found.
    #[must_use]
    pub fn first_ipv4(&self, out: &mut [u8; 4]) -> bool {
        for a in self.answers() {
            if a.atype == TYPE_A && a.class == CLASS_IN && a.rdata.len() == 4 {
                out.copy_from_slice(a.rdata);
                return true;
            }
        }
        false
    }

    /// Copy the first `AAAA` (IPv6) record's address into `out`, returning
    /// `true` if one was found.
    #[must_use]
    pub fn first_ipv6(&self, out: &mut [u8; 16]) -> bool {
        for a in self.answers() {
            if a.atype == TYPE_AAAA && a.class == CLASS_IN && a.rdata.len() == 16 {
                out.copy_from_slice(a.rdata);
                return true;
            }
        }
        false
    }

    /// Decode the first `PTR` answer record's name into `out` (dotted ASCII),
    /// returning the number of bytes written. Used for reverse DNS. The record
    /// rdata is a name that may use compression back into the question, so this
    /// walks records with offset awareness (rather than via [`Answers`], which
    /// only exposes the raw rdata slice) and hands the rdata offset to
    /// [`read_name`]. Returns `None` if there is no `PTR` answer or the message
    /// is malformed.
    #[must_use]
    pub fn first_ptr(&self, out: &mut [u8]) -> Option<usize> {
        let mut off = self.answers_start()?;
        for _ in 0..self.ancount {
            let after_name = skip_name(self.buf, off)?;
            // Fixed RR fields: type(2) class(2) ttl(4) rdlength(2) = 10 bytes.
            let fixed_end = after_name.checked_add(10)?;
            if fixed_end > self.buf.len() {
                return None;
            }
            let atype = u16::from_be_bytes([
                *self.buf.get(after_name)?,
                *self.buf.get(after_name.checked_add(1)?)?,
            ]);
            let class = u16::from_be_bytes([
                *self.buf.get(after_name.checked_add(2)?)?,
                *self.buf.get(after_name.checked_add(3)?)?,
            ]);
            let rdlen = u16::from_be_bytes([
                *self.buf.get(after_name.checked_add(8)?)?,
                *self.buf.get(after_name.checked_add(9)?)?,
            ]) as usize;
            let rd_end = fixed_end.checked_add(rdlen)?;
            if rd_end > self.buf.len() {
                return None;
            }
            if atype == TYPE_PTR && class == CLASS_IN {
                return read_name(self.buf, fixed_end, out);
            }
            off = rd_end;
        }
        None
    }
}

/// Iterator over answer-section resource records. Stops (yields `None`) on any
/// malformed / truncated record rather than panicking.
pub struct Answers<'a> {
    buf: &'a [u8],
    off: usize,
    remaining: u16,
}

impl<'a> Iterator for Answers<'a> {
    type Item = Answer<'a>;

    fn next(&mut self) -> Option<Answer<'a>> {
        if self.remaining == 0 {
            return None;
        }
        let stop = |s: &mut Self| {
            s.remaining = 0;
            None
        };
        let after_name = match skip_name(self.buf, self.off) {
            Some(o) => o,
            None => return stop(self),
        };
        // Fixed RR fields: type(2) class(2) ttl(4) rdlength(2) = 10 bytes.
        let fixed_end = match after_name.checked_add(10) {
            Some(e) if e <= self.buf.len() => e,
            _ => return stop(self),
        };
        let atype = u16::from_be_bytes([self.buf[after_name], self.buf[after_name + 1]]);
        let class = u16::from_be_bytes([self.buf[after_name + 2], self.buf[after_name + 3]]);
        let ttl = u32::from_be_bytes([
            self.buf[after_name + 4],
            self.buf[after_name + 5],
            self.buf[after_name + 6],
            self.buf[after_name + 7],
        ]);
        let rdlen = u16::from_be_bytes([self.buf[after_name + 8], self.buf[after_name + 9]]) as usize;
        let rd_end = match fixed_end.checked_add(rdlen) {
            Some(e) if e <= self.buf.len() => e,
            _ => return stop(self),
        };
        let rdata = &self.buf[fixed_end..rd_end];
        self.off = rd_end;
        self.remaining -= 1;
        Some(Answer { atype, class, ttl, rdata })
    }
}

/// Encode a dotted `name` (e.g. `b"example.com"`) as DNS labels into
/// `out[pos..]`, terminated by the root byte. Returns the offset just past the
/// terminator, or `None` on a bad label or insufficient space.
fn encode_name(out: &mut [u8], mut pos: usize, name: &[u8]) -> Option<usize> {
    for label in name.split(|&b| b == b'.') {
        if label.is_empty() {
            continue; // tolerate leading/trailing/duplicate dots
        }
        let len = label.len();
        if len > MAX_LABEL {
            return None;
        }
        let next = pos.checked_add(1 + len)?;
        if next > out.len() {
            return None;
        }
        out[pos] = len as u8;
        out[pos + 1..next].copy_from_slice(label);
        pos = next;
    }
    if pos >= out.len() {
        return None;
    }
    out[pos] = 0; // root
    pos.checked_add(1)
}

/// Build a standard recursive query for `qname` of type `qtype` (e.g.
/// [`TYPE_A`]) with transaction id `id`, writing into `out`. Returns the number
/// of bytes written, or `None` if `out` is too small or a label is invalid.
#[must_use]
pub fn write_query(out: &mut [u8], id: u16, qname: &[u8], qtype: u16) -> Option<usize> {
    if out.len() < HEADER_LEN {
        return None;
    }
    out[0..2].copy_from_slice(&id.to_be_bytes());
    // Flags: standard query (opcode 0), recursion desired.
    out[2..4].copy_from_slice(&0x0100u16.to_be_bytes());
    out[4..6].copy_from_slice(&1u16.to_be_bytes()); // qdcount
    out[6..8].copy_from_slice(&0u16.to_be_bytes()); // ancount
    out[8..10].copy_from_slice(&0u16.to_be_bytes()); // nscount
    out[10..12].copy_from_slice(&0u16.to_be_bytes()); // arcount

    let pos = encode_name(out, HEADER_LEN, qname)?;
    let end = pos.checked_add(4)?;
    if end > out.len() {
        return None;
    }
    out[pos..pos + 2].copy_from_slice(&qtype.to_be_bytes());
    out[pos + 2..pos + 4].copy_from_slice(&CLASS_IN.to_be_bytes());
    Some(end)
}

/// Write `val` as minimal decimal ASCII (no leading zeros) into `out[pos..]`.
/// Returns the offset just past the digits, or `None` if `out` is too small.
fn write_u8_dec(out: &mut [u8], pos: usize, val: u8) -> Option<usize> {
    let mut pos = pos;
    if val >= 100 {
        *out.get_mut(pos)? = b'0' + val / 100;
        pos = pos.checked_add(1)?;
    }
    if val >= 10 {
        *out.get_mut(pos)? = b'0' + (val / 10) % 10;
        pos = pos.checked_add(1)?;
    }
    *out.get_mut(pos)? = b'0' + val % 10;
    pos.checked_add(1)
}

/// Build a reverse-DNS (`PTR`) query for IPv4 address `ip`, i.e. a query for the
/// name `d.c.b.a.in-addr.arpa` (RFC 1035 §3.5), with transaction id `id`,
/// writing into `out`. Returns the number of bytes written, or `None` if `out`
/// is too small.
#[must_use]
pub fn write_ptr_query(out: &mut [u8], id: u16, ip: &[u8; 4]) -> Option<usize> {
    // Longest reverse name: "255.255.255.255.in-addr.arpa" = 28 bytes.
    let mut name = [0u8; 32];
    let mut pos = 0usize;
    for i in (0..4).rev() {
        pos = write_u8_dec(&mut name, pos, *ip.get(i)?)?;
        *name.get_mut(pos)? = b'.';
        pos = pos.checked_add(1)?;
    }
    for &b in b"in-addr.arpa" {
        *name.get_mut(pos)? = b;
        pos = pos.checked_add(1)?;
    }
    write_query(out, id, name.get(..pos)?, TYPE_PTR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_encodes_name_and_header() {
        let mut buf = [0u8; 64];
        let n = write_query(&mut buf, 0x1234, b"example.com", TYPE_A).unwrap();
        // Header: id, flags RD, qdcount 1.
        assert_eq!(&buf[0..2], &[0x12, 0x34]);
        assert_eq!(&buf[2..4], &[0x01, 0x00]);
        assert_eq!(&buf[4..6], &[0x00, 0x01]);
        // Question name: 7"example" 3"com" 0.
        assert_eq!(buf[12], 7);
        assert_eq!(&buf[13..20], b"example");
        assert_eq!(buf[20], 3);
        assert_eq!(&buf[21..24], b"com");
        assert_eq!(buf[24], 0);
        // qtype A, qclass IN, then end.
        assert_eq!(&buf[25..27], &[0x00, 0x01]);
        assert_eq!(&buf[27..29], &[0x00, 0x01]);
        assert_eq!(n, 29);
        // The message parses back as a (non-)response header.
        let m = Message::parse(&buf[..n]).unwrap();
        assert_eq!(m.id, 0x1234);
        assert!(!m.is_response());
        assert_eq!(m.qdcount, 1);
        assert_eq!(m.ancount, 0);
    }

    /// Build a synthetic response: the question from `write_query`, plus one A
    /// answer whose name is a compression pointer back to the question.
    fn synth_response(qtype: u16, rdata: &[u8]) -> ([u8; 128], usize) {
        let mut buf = [0u8; 128];
        let qend = write_query(&mut buf, 0xABCD, b"example.com", qtype).unwrap();
        // Turn it into a response: QR + RD + RA, ancount 1.
        buf[2..4].copy_from_slice(&0x8180u16.to_be_bytes());
        buf[6..8].copy_from_slice(&1u16.to_be_bytes());
        let mut off = qend;
        // Answer name: pointer to the question name at offset 12.
        buf[off] = 0xC0;
        buf[off + 1] = 0x0C;
        off += 2;
        buf[off..off + 2].copy_from_slice(&qtype.to_be_bytes());
        buf[off + 2..off + 4].copy_from_slice(&CLASS_IN.to_be_bytes());
        buf[off + 4..off + 8].copy_from_slice(&300u32.to_be_bytes()); // ttl
        buf[off + 8..off + 10].copy_from_slice(&(rdata.len() as u16).to_be_bytes());
        off += 10;
        buf[off..off + rdata.len()].copy_from_slice(rdata);
        off += rdata.len();
        (buf, off)
    }

    #[test]
    fn parses_compressed_a_answer() {
        let (buf, len) = synth_response(TYPE_A, &[93, 184, 216, 34]);
        let m = Message::parse(&buf[..len]).unwrap();
        assert!(m.is_response());
        assert_eq!(m.rcode(), 0);
        assert_eq!(m.ancount, 1);
        let mut ip = [0u8; 4];
        assert!(m.first_ipv4(&mut ip));
        assert_eq!(ip, [93, 184, 216, 34]);
        // No AAAA present.
        let mut ip6 = [0u8; 16];
        assert!(!m.first_ipv6(&mut ip6));
    }

    #[test]
    fn parses_aaaa_answer() {
        let addr = [
            0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x99,
        ];
        let (buf, len) = synth_response(TYPE_AAAA, &addr);
        let m = Message::parse(&buf[..len]).unwrap();
        let mut ip6 = [0u8; 16];
        assert!(m.first_ipv6(&mut ip6));
        assert_eq!(ip6, addr);
    }

    #[test]
    fn iterates_answer_fields() {
        let (buf, len) = synth_response(TYPE_A, &[1, 2, 3, 4]);
        let m = Message::parse(&buf[..len]).unwrap();
        let mut it = m.answers();
        let a = it.next().unwrap();
        assert_eq!(a.atype, TYPE_A);
        assert_eq!(a.class, CLASS_IN);
        assert_eq!(a.ttl, 300);
        assert_eq!(a.rdata, &[1, 2, 3, 4]);
        assert!(it.next().is_none());
    }

    #[test]
    fn truncated_response_yields_no_answers() {
        let (buf, len) = synth_response(TYPE_A, &[1, 2, 3, 4]);
        // Cut the buffer mid-answer: header claims 1 answer but the RR is gone.
        let short = &buf[..len - 3];
        let m = Message::parse(short).unwrap();
        let mut ip = [0u8; 4];
        assert!(!m.first_ipv4(&mut ip));
    }

    #[test]
    fn rejects_short_header() {
        assert!(Message::parse(&[0u8; 8]).is_none());
    }

    #[test]
    fn query_buffer_too_small_is_none() {
        let mut out = [0u8; 8];
        assert!(write_query(&mut out, 1, b"example.com", TYPE_A).is_none());
    }

    #[test]
    fn oversized_label_rejected() {
        let mut out = [0u8; 128];
        let long = [b'a'; 64]; // 64 > 63
        assert!(write_query(&mut out, 1, &long, TYPE_A).is_none());
    }

    #[test]
    fn ptr_query_encodes_reverse_name() {
        let mut buf = [0u8; 64];
        let n = write_ptr_query(&mut buf, 0x2A2A, &[8, 8, 4, 4]).unwrap();
        let m = Message::parse(&buf[..n]).unwrap();
        assert_eq!(m.id, 0x2A2A);
        assert_eq!(m.qdcount, 1);
        // Reverse name is "4.4.8.8.in-addr.arpa": first label "4".
        assert_eq!(buf[12], 1);
        assert_eq!(buf[13], b'4');
        // qtype PTR trails the encoded name + root byte.
        let name_end = skip_name(&buf, 12).unwrap();
        assert_eq!(
            u16::from_be_bytes([buf[name_end], buf[name_end + 1]]),
            TYPE_PTR
        );
    }

    #[test]
    fn ptr_query_multi_digit_octets() {
        let mut buf = [0u8; 64];
        let n = write_ptr_query(&mut buf, 1, &[192, 168, 1, 254]).unwrap();
        // "254.1.168.192.in-addr.arpa" — first label "254" (len 3).
        assert_eq!(buf[12], 3);
        assert_eq!(&buf[13..16], b"254");
        assert!(Message::parse(&buf[..n]).is_some());
    }

    #[test]
    fn read_name_decodes_uncompressed() {
        // A bare encoded name at offset 0: 3"one" 3"two" 0.
        let name = [3, b'o', b'n', b'e', 3, b't', b'w', b'o', 0];
        let mut out = [0u8; 32];
        let n = read_name(&name, 0, &mut out).unwrap();
        assert_eq!(&out[..n], b"one.two");
    }

    #[test]
    fn read_name_follows_compression_pointer() {
        // buf: [pad][3"com"][0] at 1..6, then a pointer at 6 -> offset 1.
        let mut buf = [0u8; 16];
        buf[1] = 3;
        buf[2..5].copy_from_slice(b"com");
        buf[5] = 0;
        // Pointer to offset 1.
        buf[6] = 0xC0;
        buf[7] = 0x01;
        let mut out = [0u8; 32];
        let n = read_name(&buf, 6, &mut out).unwrap();
        assert_eq!(&out[..n], b"com");
    }

    #[test]
    fn read_name_rejects_pointer_loop() {
        // A pointer at offset 0 that points to itself.
        let buf = [0xC0u8, 0x00];
        let mut out = [0u8; 32];
        assert!(read_name(&buf, 0, &mut out).is_none());
    }

    #[test]
    fn read_name_out_too_small() {
        let name = [5, b'h', b'e', b'l', b'l', b'o', 0];
        let mut out = [0u8; 3];
        assert!(read_name(&name, 0, &mut out).is_none());
    }

    /// Build a synthetic PTR response: the reverse question, plus one PTR answer
    /// (name = compression pointer to the question) whose rdata is a name that
    /// itself uses compression back to the question's suffix.
    #[test]
    fn first_ptr_decodes_answer() {
        let mut buf = [0u8; 128];
        let qend = write_ptr_query(&mut buf, 0xBEEF, &[1, 1, 1, 1]).unwrap();
        // Make it a response with 1 answer.
        buf[2..4].copy_from_slice(&0x8180u16.to_be_bytes());
        buf[6..8].copy_from_slice(&1u16.to_be_bytes());
        let mut off = qend;
        // Answer name: pointer to question name at offset 12.
        buf[off] = 0xC0;
        buf[off + 1] = 0x0C;
        off += 2;
        buf[off..off + 2].copy_from_slice(&TYPE_PTR.to_be_bytes());
        buf[off + 2..off + 4].copy_from_slice(&CLASS_IN.to_be_bytes());
        buf[off + 4..off + 8].copy_from_slice(&300u32.to_be_bytes());
        // rdata: 3"dns" 0  (an uncompressed PTR target for simplicity).
        let rdata = [3u8, b'd', b'n', b's', 0];
        buf[off + 8..off + 10].copy_from_slice(&(rdata.len() as u16).to_be_bytes());
        off += 10;
        buf[off..off + rdata.len()].copy_from_slice(&rdata);
        off += rdata.len();

        let m = Message::parse(&buf[..off]).unwrap();
        assert!(m.is_response());
        assert_eq!(m.ancount, 1);
        let mut out = [0u8; 64];
        let n = m.first_ptr(&mut out).unwrap();
        assert_eq!(&out[..n], b"dns");
    }

    #[test]
    fn first_ptr_none_without_ptr_answer() {
        let (buf, len) = synth_response(TYPE_A, &[1, 2, 3, 4]);
        let m = Message::parse(&buf[..len]).unwrap();
        let mut out = [0u8; 64];
        assert!(m.first_ptr(&mut out).is_none());
    }
}
