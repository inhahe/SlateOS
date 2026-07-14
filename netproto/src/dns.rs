//! DNS (RFC 1035) message construction and response parsing.
//!
//! Enough to drive a resolver: build a standard recursive `A`/`AAAA` query and
//! walk the answer section of a response, transparently following the
//! compression pointers (RFC 1035 §4.1.4) that appear in answer names. This is
//! allocation-free — queries are written into a caller buffer and answers
//! borrow the response buffer. The message rides inside a UDP datagram (see
//! [`crate::udp`]).
//!
//! Full name *decoding* (materialising the dotted name) is intentionally not
//! provided: a resolver matching a known question only needs to skip answer
//! names to reach the record data, which this module does safely.

/// Fixed DNS header length (id, flags, and the four section counts).
pub const HEADER_LEN: usize = 12;

/// Resource-record type: IPv4 address (`A`).
pub const TYPE_A: u16 = 1;
/// Resource-record type: IPv6 address (`AAAA`).
pub const TYPE_AAAA: u16 = 28;
/// Resource-record class: Internet (`IN`).
pub const CLASS_IN: u16 = 1;

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
}
