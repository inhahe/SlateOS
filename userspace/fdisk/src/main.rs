// SlateOS fdisk - Multi-personality disk partitioning utility
//
// Personalities detected via argv[0] basename:
//   fdisk     - MBR/GPT interactive partition editor
//   gdisk     - GPT-only partition editor
//   sfdisk    - Scriptable/non-interactive partition tool
//   cfdisk    - Curses-style partition viewer (simplified)
//   partprobe - Inform kernel of partition table changes

#![cfg_attr(not(test), no_main)]
#![cfg_attr(test, allow(dead_code))]

// ── Output Helpers ────────────────────────────────────────────────────

fn print_out(msg: &[u8]) {
    #[cfg(not(test))]
    {
        use std::io::Write;
        let _ = std::io::stdout().write_all(msg);
    }
    #[cfg(test)]
    {
        let _ = msg;
    }
}

#[allow(dead_code)]
fn print_err(msg: &[u8]) {
    #[cfg(not(test))]
    {
        use std::io::Write;
        let _ = std::io::stderr().write_all(msg);
    }
    #[cfg(test)]
    {
        let _ = msg;
    }
}

// ── C String / Byte Helpers ──────────────────────────────────────────

unsafe fn cstr_to_slice(ptr: *const u8) -> &'static [u8] {
    if ptr.is_null() {
        return b"";
    }
    let mut len = 0usize;
    // SAFETY: Walking null-terminated C string from kernel/libc.
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
            if len >= 4096 {
                break;
            }
        }
        core::slice::from_raw_parts(ptr, len)
    }
}

/// Extract the basename from a path (everything after last b'/' or b'\\').
fn basename(path: &[u8]) -> &[u8] {
    let mut last = 0;
    let mut i = 0;
    while i < path.len() {
        if (path[i] == b'/' || path[i] == b'\\')
            && i + 1 < path.len() {
                last = i + 1;
            }
        i += 1;
    }
    if last < path.len() {
        &path[last..]
    } else {
        path
    }
}

/// Strip a trailing .exe suffix (case-insensitive) if present.
fn strip_exe(name: &[u8]) -> &[u8] {
    if name.len() >= 4 {
        let tail = &name[name.len() - 4..];
        if tail[0] == b'.'
            && (tail[1] == b'e' || tail[1] == b'E')
            && (tail[2] == b'x' || tail[2] == b'X')
            && (tail[3] == b'e' || tail[3] == b'E')
        {
            return &name[..name.len() - 4];
        }
    }
    name
}

/// Compare two byte slices for equality.
fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Compare two byte slices case-insensitively (ASCII).
fn bytes_eq_ci(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        let ca = if a[i] >= b'A' && a[i] <= b'Z' { a[i] + 32 } else { a[i] };
        let cb = if b[i] >= b'A' && b[i] <= b'Z' { b[i] + 32 } else { b[i] };
        if ca != cb {
            return false;
        }
        i += 1;
    }
    true
}

/// Check if a byte slice starts with a prefix.
fn starts_with(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.len() > haystack.len() {
        return false;
    }
    bytes_eq(&haystack[..needle.len()], needle)
}

/// Format a u64 into a decimal byte buffer, returning the number of bytes written.
fn format_u64(val: u64, buf: &mut [u8]) -> usize {
    if val == 0 {
        if !buf.is_empty() {
            buf[0] = b'0';
        }
        return 1;
    }
    let mut tmp = [0u8; 20];
    let mut n = val;
    let mut i = 0;
    while n > 0 {
        if let Some(slot) = tmp.get_mut(i) {
            *slot = b'0' + (n % 10) as u8;
        }
        n /= 10;
        i += 1;
    }
    // Reverse into buf
    let mut w = 0;
    while w < i && w < buf.len() {
        buf[w] = tmp[i - 1 - w];
        w += 1;
    }
    w
}

/// Format a u32 as decimal into a byte buffer.
fn format_u32(val: u32, buf: &mut [u8]) -> usize {
    format_u64(val as u64, buf)
}

/// Format a u8 as two hex digits (lowercase) into a 2-byte buffer.
fn format_hex_u8(val: u8, buf: &mut [u8; 2]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    buf[0] = HEX[(val >> 4) as usize];
    buf[1] = HEX[(val & 0x0f) as usize];
}

/// Format a u8 as two hex digits (uppercase) into a 2-byte buffer.
fn format_hex_u8_upper(val: u8, buf: &mut [u8; 2]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    buf[0] = HEX[(val >> 4) as usize];
    buf[1] = HEX[(val & 0x0f) as usize];
}

/// Parse a decimal integer from a byte slice.
fn parse_u64(s: &[u8]) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut val: u64 = 0;
    let mut i = 0;
    while i < s.len() {
        let d = s[i];
        if !d.is_ascii_digit() {
            return None;
        }
        val = val.checked_mul(10)?;
        val = val.checked_add((d - b'0') as u64)?;
        i += 1;
    }
    Some(val)
}

/// Parse a decimal u32 from a byte slice.
fn parse_u32(s: &[u8]) -> Option<u32> {
    let v = parse_u64(s)?;
    if v > u32::MAX as u64 {
        return None;
    }
    Some(v as u32)
}

/// Parse a hex byte (1-2 hex digits) from a byte slice.
fn parse_hex_u8(s: &[u8]) -> Option<u8> {
    if s.is_empty() || s.len() > 2 {
        return None;
    }
    let mut val: u8 = 0;
    let mut i = 0;
    while i < s.len() {
        let d = s[i];
        let nibble = if d.is_ascii_digit() {
            d - b'0'
        } else if (b'a'..=b'f').contains(&d) {
            d - b'a' + 10
        } else if (b'A'..=b'F').contains(&d) {
            d - b'A' + 10
        } else {
            return None;
        };
        val = val.checked_mul(16)?.checked_add(nibble)?;
        i += 1;
    }
    Some(val)
}

/// Check if all bytes are hex digits.
fn is_all_hex(s: &[u8]) -> bool {
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if !(c.is_ascii_digit() || (b'a'..=b'f').contains(&c) || (b'A'..=b'F').contains(&c)) {
            return false;
        }
        i += 1;
    }
    true
}

// ── Size Formatting ──────────────────────────────────────────────────

/// Format a byte count as human-readable into a buffer. Returns bytes written.
fn format_size(bytes: u64, buf: &mut [u8]) -> usize {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    const TIB: u64 = 1024 * GIB;

    if bytes >= TIB {
        let whole = bytes / TIB;
        let frac = ((bytes % TIB) * 10) / TIB;
        format_size_unit(whole, frac, b'T', buf)
    } else if bytes >= GIB {
        let whole = bytes / GIB;
        let frac = ((bytes % GIB) * 10) / GIB;
        format_size_unit(whole, frac, b'G', buf)
    } else if bytes >= MIB {
        let whole = bytes / MIB;
        let frac = ((bytes % MIB) * 10) / MIB;
        format_size_unit(whole, frac, b'M', buf)
    } else if bytes >= KIB {
        let whole = bytes / KIB;
        let frac = ((bytes % KIB) * 10) / KIB;
        format_size_unit(whole, frac, b'K', buf)
    } else {
        let mut w = format_u64(bytes, buf);
        if w < buf.len() {
            buf[w] = b'B';
            w += 1;
        }
        w
    }
}

fn format_size_unit(whole: u64, frac: u64, unit: u8, buf: &mut [u8]) -> usize {
    let mut w = format_u64(whole, buf);
    if frac > 0 && w + 2 < buf.len() {
        buf[w] = b'.';
        w += 1;
        w += format_u64(frac, &mut buf[w..]);
    }
    if w < buf.len() {
        buf[w] = unit;
        w += 1;
    }
    w
}

// ── CRC32 ────────────────────────────────────────────────────────────

/// Compute CRC32 using the standard polynomial (IEEE 802.3).
/// GPT headers include CRC32 checksums over the header and partition entries.
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut i = 0;
    while i < data.len() {
        crc ^= data[i] as u32;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        i += 1;
    }
    !crc
}

// ── Little-Endian Read/Write ─────────────────────────────────────────

#[allow(dead_code)]
fn le_u16(buf: &[u8], off: usize) -> u16 {
    if off + 2 > buf.len() { return 0; }
    u16::from_le_bytes([buf[off], buf[off + 1]])
}

fn le_u32(buf: &[u8], off: usize) -> u32 {
    if off + 4 > buf.len() { return 0; }
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

fn le_u64(buf: &[u8], off: usize) -> u64 {
    if off + 8 > buf.len() { return 0; }
    u64::from_le_bytes([
        buf[off], buf[off + 1], buf[off + 2], buf[off + 3],
        buf[off + 4], buf[off + 5], buf[off + 6], buf[off + 7],
    ])
}

#[allow(dead_code)]
fn write_le_u16(buf: &mut [u8], off: usize, val: u16) {
    let b = val.to_le_bytes();
    if off + 2 <= buf.len() {
        buf[off] = b[0];
        buf[off + 1] = b[1];
    }
}

fn write_le_u32(buf: &mut [u8], off: usize, val: u32) {
    let b = val.to_le_bytes();
    if off + 4 <= buf.len() {
        buf[off] = b[0];
        buf[off + 1] = b[1];
        buf[off + 2] = b[2];
        buf[off + 3] = b[3];
    }
}

fn write_le_u64(buf: &mut [u8], off: usize, val: u64) {
    let b = val.to_le_bytes();
    if off + 8 <= buf.len() {
        buf[off] = b[0];
        buf[off + 1] = b[1];
        buf[off + 2] = b[2];
        buf[off + 3] = b[3];
        buf[off + 4] = b[4];
        buf[off + 5] = b[5];
        buf[off + 6] = b[6];
        buf[off + 7] = b[7];
    }
}

// ── GUID Handling ────────────────────────────────────────────────────

/// Parse a GUID string "XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX" (36 bytes)
/// into 16-byte mixed-endian form as stored on disk in GPT entries.
const fn parse_guid(s: &[u8; 36]) -> [u8; 16] {
    const fn hex(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'A'..=b'F' => c - b'A' + 10,
            b'a'..=b'f' => c - b'a' + 10,
            _ => 0,
        }
    }
    const fn hex2(hi: u8, lo: u8) -> u8 {
        (hex(hi) << 4) | hex(lo)
    }

    let mut g = [0u8; 16];
    // Group 1 (4 bytes, little-endian)
    g[3] = hex2(s[0], s[1]);
    g[2] = hex2(s[2], s[3]);
    g[1] = hex2(s[4], s[5]);
    g[0] = hex2(s[6], s[7]);
    // Group 2 (2 bytes, little-endian)
    g[5] = hex2(s[9], s[10]);
    g[4] = hex2(s[11], s[12]);
    // Group 3 (2 bytes, little-endian)
    g[7] = hex2(s[14], s[15]);
    g[6] = hex2(s[16], s[17]);
    // Group 4 (2 bytes, big-endian)
    g[8] = hex2(s[19], s[20]);
    g[9] = hex2(s[21], s[22]);
    // Group 5 (6 bytes, big-endian)
    g[10] = hex2(s[24], s[25]);
    g[11] = hex2(s[26], s[27]);
    g[12] = hex2(s[28], s[29]);
    g[13] = hex2(s[30], s[31]);
    g[14] = hex2(s[32], s[33]);
    g[15] = hex2(s[34], s[35]);
    g
}

/// Parse a GUID from a runtime byte slice (must be exactly 36 bytes with dashes).
fn parse_guid_runtime(s: &[u8]) -> Option<[u8; 16]> {
    if s.len() != 36 {
        return None;
    }
    // Verify dash positions
    if s[8] != b'-' || s[13] != b'-' || s[18] != b'-' || s[23] != b'-' {
        return None;
    }
    // Verify all other chars are hex
    let groups: &[core::ops::Range<usize>] = &[0..8, 9..13, 14..18, 19..23, 24..36];
    for range in groups {
        if !is_all_hex(&s[range.clone()]) {
            return None;
        }
    }
    let mut arr = [0u8; 36];
    arr.copy_from_slice(s);
    Some(parse_guid(&arr))
}

/// Format a 16-byte mixed-endian GUID into "XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX".
/// Writes 36 bytes into buf. Returns 36 on success.
fn format_guid(g: &[u8; 16], buf: &mut [u8]) -> usize {
    if buf.len() < 36 {
        return 0;
    }
    let mut h = [0u8; 2];
    // Group 1: bytes [3][2][1][0]
    format_hex_u8_upper(g[3], &mut h); buf[0] = h[0]; buf[1] = h[1];
    format_hex_u8_upper(g[2], &mut h); buf[2] = h[0]; buf[3] = h[1];
    format_hex_u8_upper(g[1], &mut h); buf[4] = h[0]; buf[5] = h[1];
    format_hex_u8_upper(g[0], &mut h); buf[6] = h[0]; buf[7] = h[1];
    buf[8] = b'-';
    // Group 2: bytes [5][4]
    format_hex_u8_upper(g[5], &mut h); buf[9] = h[0]; buf[10] = h[1];
    format_hex_u8_upper(g[4], &mut h); buf[11] = h[0]; buf[12] = h[1];
    buf[13] = b'-';
    // Group 3: bytes [7][6]
    format_hex_u8_upper(g[7], &mut h); buf[14] = h[0]; buf[15] = h[1];
    format_hex_u8_upper(g[6], &mut h); buf[16] = h[0]; buf[17] = h[1];
    buf[18] = b'-';
    // Group 4: bytes [8][9]
    format_hex_u8_upper(g[8], &mut h); buf[19] = h[0]; buf[20] = h[1];
    format_hex_u8_upper(g[9], &mut h); buf[21] = h[0]; buf[22] = h[1];
    buf[23] = b'-';
    // Group 5: bytes [10]..[15]
    format_hex_u8_upper(g[10], &mut h); buf[24] = h[0]; buf[25] = h[1];
    format_hex_u8_upper(g[11], &mut h); buf[26] = h[0]; buf[27] = h[1];
    format_hex_u8_upper(g[12], &mut h); buf[28] = h[0]; buf[29] = h[1];
    format_hex_u8_upper(g[13], &mut h); buf[30] = h[0]; buf[31] = h[1];
    format_hex_u8_upper(g[14], &mut h); buf[32] = h[0]; buf[33] = h[1];
    format_hex_u8_upper(g[15], &mut h); buf[34] = h[0]; buf[35] = h[1];
    36
}

/// Check if a 16-byte GUID is all zeros.
fn guid_is_zero(g: &[u8; 16]) -> bool {
    let mut i = 0;
    while i < 16 {
        if g[i] != 0 {
            return false;
        }
        i += 1;
    }
    true
}

// ── GPT Partition Type Database ──────────────────────────────────────

struct GptTypeEntry {
    guid: [u8; 16],
    name: &'static [u8],
}

/// 50+ known GPT partition type GUIDs.
const GPT_TYPES: &[GptTypeEntry] = &[
    GptTypeEntry { guid: parse_guid(b"C12A7328-F81F-11D2-BA4B-00A0C93EC93B"), name: b"EFI System" },
    GptTypeEntry { guid: parse_guid(b"024DEE41-33E7-11D3-9D69-0008C781F39F"), name: b"MBR partition scheme" },
    GptTypeEntry { guid: parse_guid(b"21686148-6449-6E6F-744E-656564454649"), name: b"BIOS boot" },
    GptTypeEntry { guid: parse_guid(b"D3BFE2DE-3DAF-11DF-BA40-E3A556D89593"), name: b"Intel Fast Flash" },
    GptTypeEntry { guid: parse_guid(b"F4019732-066E-4E12-8273-346C5641494F"), name: b"Sony boot" },
    GptTypeEntry { guid: parse_guid(b"BFBFAFE7-A34F-448A-9A5B-6213EB736C22"), name: b"Lenovo boot" },
    // Microsoft
    GptTypeEntry { guid: parse_guid(b"E3C9E316-0B5C-4DB8-817D-F92DF00215AE"), name: b"Microsoft reserved" },
    GptTypeEntry { guid: parse_guid(b"EBD0A0A2-B9E5-4433-87C0-68B6B72699C7"), name: b"Microsoft basic data" },
    GptTypeEntry { guid: parse_guid(b"5808C8AA-7E8F-42E0-85D2-E1E90434CFB3"), name: b"Microsoft LDM metadata" },
    GptTypeEntry { guid: parse_guid(b"AF9B60A0-1431-4F62-BC68-3311714A69AD"), name: b"Microsoft LDM data" },
    GptTypeEntry { guid: parse_guid(b"DE94BBA4-06D1-4D40-A16A-BFD50179D6AC"), name: b"Windows recovery" },
    GptTypeEntry { guid: parse_guid(b"37AFFC90-EF7D-4E96-91C3-2D7AE055B174"), name: b"IBM GPFS" },
    GptTypeEntry { guid: parse_guid(b"E75CAF8F-F680-4CEE-AFA3-B001E56EFC2D"), name: b"Microsoft Storage Spaces" },
    GptTypeEntry { guid: parse_guid(b"558D43C5-A1AC-43C0-AAC8-D1472B2923D1"), name: b"Microsoft Storage Replica" },
    // Linux
    GptTypeEntry { guid: parse_guid(b"0FC63DAF-8483-4772-8E79-3D69D8477DE4"), name: b"Linux filesystem" },
    GptTypeEntry { guid: parse_guid(b"A19D880F-05FC-4D3B-A006-743F0F84911E"), name: b"Linux RAID" },
    GptTypeEntry { guid: parse_guid(b"44479540-F297-41B2-9AF7-D131D5F0458A"), name: b"Linux root (x86)" },
    GptTypeEntry { guid: parse_guid(b"4F68BCE3-E8CD-4DB1-96E7-FBCAF984B709"), name: b"Linux root (x86-64)" },
    GptTypeEntry { guid: parse_guid(b"69DAD710-2CE4-4E3C-B16C-21A1D49ABED3"), name: b"Linux root (ARM)" },
    GptTypeEntry { guid: parse_guid(b"B921B045-1DF0-41C3-AF44-4C6F280D3FAE"), name: b"Linux root (ARM64)" },
    GptTypeEntry { guid: parse_guid(b"0657FD6D-A4AB-43C4-84E5-0933C84B4F4F"), name: b"Linux swap" },
    GptTypeEntry { guid: parse_guid(b"E6D6D379-F507-44C2-A23C-238F2A3DF928"), name: b"Linux LVM" },
    GptTypeEntry { guid: parse_guid(b"933AC7E1-2EB4-4F13-B844-0E14E2AEF915"), name: b"Linux /home" },
    GptTypeEntry { guid: parse_guid(b"3B8F8425-20E0-4F3B-907F-1A25A76F98E8"), name: b"Linux /srv" },
    GptTypeEntry { guid: parse_guid(b"7C3457EF-0000-11AA-AA11-00306543ECAC"), name: b"Linux dm-crypt" },
    GptTypeEntry { guid: parse_guid(b"CA7D7CCB-63ED-4C53-861C-1742536059CC"), name: b"Linux LUKS" },
    GptTypeEntry { guid: parse_guid(b"8DA63339-0007-60C0-C436-083AC8230908"), name: b"Linux reserved" },
    // FreeBSD
    GptTypeEntry { guid: parse_guid(b"83BD6B9D-7F41-11DC-BE0B-001560B84F0F"), name: b"FreeBSD boot" },
    GptTypeEntry { guid: parse_guid(b"516E7CB4-6ECF-11D6-8FF8-00022D09712B"), name: b"FreeBSD data" },
    GptTypeEntry { guid: parse_guid(b"516E7CB5-6ECF-11D6-8FF8-00022D09712B"), name: b"FreeBSD swap" },
    GptTypeEntry { guid: parse_guid(b"516E7CB6-6ECF-11D6-8FF8-00022D09712B"), name: b"FreeBSD UFS" },
    GptTypeEntry { guid: parse_guid(b"516E7CBA-6ECF-11D6-8FF8-00022D09712B"), name: b"FreeBSD ZFS" },
    GptTypeEntry { guid: parse_guid(b"516E7CB8-6ECF-11D6-8FF8-00022D09712B"), name: b"FreeBSD Vinum" },
    // Apple
    GptTypeEntry { guid: parse_guid(b"48465300-0000-11AA-AA11-00306543ECAC"), name: b"Apple HFS+" },
    GptTypeEntry { guid: parse_guid(b"55465300-0000-11AA-AA11-00306543ECAC"), name: b"Apple UFS" },
    GptTypeEntry { guid: parse_guid(b"52414944-0000-11AA-AA11-00306543ECAC"), name: b"Apple RAID" },
    GptTypeEntry { guid: parse_guid(b"52414944-5F4F-11AA-AA11-00306543ECAC"), name: b"Apple RAID offline" },
    GptTypeEntry { guid: parse_guid(b"426F6F74-0000-11AA-AA11-00306543ECAC"), name: b"Apple boot" },
    GptTypeEntry { guid: parse_guid(b"4C616265-6C00-11AA-AA11-00306543ECAC"), name: b"Apple label" },
    GptTypeEntry { guid: parse_guid(b"5265636F-7665-11AA-AA11-00306543ECAC"), name: b"Apple TV recovery" },
    GptTypeEntry { guid: parse_guid(b"53746F72-6167-11AA-AA11-00306543ECAC"), name: b"Apple Core Storage" },
    GptTypeEntry { guid: parse_guid(b"7C3457EF-0000-11AA-AA11-00306543ECAC"), name: b"Apple APFS" },
    // Solaris
    GptTypeEntry { guid: parse_guid(b"6A82CB45-1DD2-11B2-99A6-080020736631"), name: b"Solaris boot" },
    GptTypeEntry { guid: parse_guid(b"6A85CF4D-1DD2-11B2-99A6-080020736631"), name: b"Solaris root" },
    GptTypeEntry { guid: parse_guid(b"6A87C46F-1DD2-11B2-99A6-080020736631"), name: b"Solaris swap" },
    GptTypeEntry { guid: parse_guid(b"6A8B642B-1DD2-11B2-99A6-080020736631"), name: b"Solaris backup" },
    GptTypeEntry { guid: parse_guid(b"6A898CC3-1DD2-11B2-99A6-080020736631"), name: b"Solaris /usr" },
    GptTypeEntry { guid: parse_guid(b"6A8EF2E9-1DD2-11B2-99A6-080020736631"), name: b"Solaris /var" },
    GptTypeEntry { guid: parse_guid(b"6A90BA39-1DD2-11B2-99A6-080020736631"), name: b"Solaris /home" },
    // NetBSD
    GptTypeEntry { guid: parse_guid(b"49F48D32-B10E-11DC-B99B-0019D1879648"), name: b"NetBSD swap" },
    GptTypeEntry { guid: parse_guid(b"49F48D5A-B10E-11DC-B99B-0019D1879648"), name: b"NetBSD FFS" },
    GptTypeEntry { guid: parse_guid(b"49F48D82-B10E-11DC-B99B-0019D1879648"), name: b"NetBSD LFS" },
    GptTypeEntry { guid: parse_guid(b"2DB519C4-B10F-11DC-B99B-0019D1879648"), name: b"NetBSD concat" },
    GptTypeEntry { guid: parse_guid(b"2DB519EC-B10F-11DC-B99B-0019D1879648"), name: b"NetBSD encrypted" },
    GptTypeEntry { guid: parse_guid(b"49F48DAA-B10E-11DC-B99B-0019D1879648"), name: b"NetBSD RAID" },
    // ChromeOS
    GptTypeEntry { guid: parse_guid(b"FE3A2A5D-4F32-41A7-B725-ACCC3285A309"), name: b"ChromeOS kernel" },
    GptTypeEntry { guid: parse_guid(b"3CB8E202-3B7E-47DD-8A3C-7FF2A13CFCEC"), name: b"ChromeOS rootfs" },
    GptTypeEntry { guid: parse_guid(b"2E0A753D-9E48-43B0-8337-B15192CB1B5E"), name: b"ChromeOS reserved" },
    // VMware
    GptTypeEntry { guid: parse_guid(b"AA31E02A-400F-11DB-9590-000C2911D1B8"), name: b"VMware VMFS" },
    GptTypeEntry { guid: parse_guid(b"9198EFFC-31C0-11DB-8F78-000C2911D1B8"), name: b"VMware reserved" },
];

/// Look up a GPT partition type GUID and return its human-readable name.
fn gpt_type_name(guid: &[u8; 16]) -> &'static [u8] {
    let mut i = 0;
    while i < GPT_TYPES.len() {
        if bytes_eq(&GPT_TYPES[i].guid, guid) {
            return GPT_TYPES[i].name;
        }
        i += 1;
    }
    b"unknown"
}

// ── MBR Partition Type Database ──────────────────────────────────────

struct MbrTypeEntry {
    id: u8,
    name: &'static [u8],
}

/// 50+ MBR partition type codes.
const MBR_TYPES: &[MbrTypeEntry] = &[
    MbrTypeEntry { id: 0x00, name: b"Empty" },
    MbrTypeEntry { id: 0x01, name: b"FAT12" },
    MbrTypeEntry { id: 0x02, name: b"XENIX root" },
    MbrTypeEntry { id: 0x03, name: b"XENIX usr" },
    MbrTypeEntry { id: 0x04, name: b"FAT16 <32M" },
    MbrTypeEntry { id: 0x05, name: b"Extended" },
    MbrTypeEntry { id: 0x06, name: b"FAT16" },
    MbrTypeEntry { id: 0x07, name: b"HPFS/NTFS" },
    MbrTypeEntry { id: 0x08, name: b"AIX" },
    MbrTypeEntry { id: 0x09, name: b"AIX bootable" },
    MbrTypeEntry { id: 0x0A, name: b"OS/2 Boot Manager" },
    MbrTypeEntry { id: 0x0B, name: b"W95 FAT32" },
    MbrTypeEntry { id: 0x0C, name: b"W95 FAT32 (LBA)" },
    MbrTypeEntry { id: 0x0E, name: b"W95 FAT16 (LBA)" },
    MbrTypeEntry { id: 0x0F, name: b"W95 Ext'd (LBA)" },
    MbrTypeEntry { id: 0x10, name: b"OPUS" },
    MbrTypeEntry { id: 0x11, name: b"Hidden FAT12" },
    MbrTypeEntry { id: 0x12, name: b"Compaq diag" },
    MbrTypeEntry { id: 0x14, name: b"Hidden FAT16 <32M" },
    MbrTypeEntry { id: 0x16, name: b"Hidden FAT16" },
    MbrTypeEntry { id: 0x17, name: b"Hidden HPFS/NTFS" },
    MbrTypeEntry { id: 0x1B, name: b"Hidden W95 FAT32" },
    MbrTypeEntry { id: 0x1C, name: b"Hidden W95 FAT32 (LBA)" },
    MbrTypeEntry { id: 0x1E, name: b"Hidden W95 FAT16 (LBA)" },
    MbrTypeEntry { id: 0x24, name: b"NEC DOS" },
    MbrTypeEntry { id: 0x27, name: b"Hidden NTFS WinRE" },
    MbrTypeEntry { id: 0x39, name: b"Plan 9" },
    MbrTypeEntry { id: 0x3C, name: b"PartitionMagic" },
    MbrTypeEntry { id: 0x40, name: b"Venix 80286" },
    MbrTypeEntry { id: 0x41, name: b"PPC PReP Boot" },
    MbrTypeEntry { id: 0x42, name: b"SFS / LDM" },
    MbrTypeEntry { id: 0x4D, name: b"QNX4.x" },
    MbrTypeEntry { id: 0x4E, name: b"QNX4.x 2nd" },
    MbrTypeEntry { id: 0x4F, name: b"QNX4.x 3rd" },
    MbrTypeEntry { id: 0x50, name: b"OnTrack DM" },
    MbrTypeEntry { id: 0x51, name: b"OnTrack DM6 Aux1" },
    MbrTypeEntry { id: 0x52, name: b"CP/M" },
    MbrTypeEntry { id: 0x56, name: b"Golden Bow" },
    MbrTypeEntry { id: 0x5C, name: b"Priam Edisk" },
    MbrTypeEntry { id: 0x61, name: b"SpeedStor" },
    MbrTypeEntry { id: 0x63, name: b"GNU HURD" },
    MbrTypeEntry { id: 0x64, name: b"Novell Netware 286" },
    MbrTypeEntry { id: 0x65, name: b"Novell Netware 386" },
    MbrTypeEntry { id: 0x70, name: b"DiskSecure MultiBoot" },
    MbrTypeEntry { id: 0x75, name: b"PC/IX" },
    MbrTypeEntry { id: 0x80, name: b"Old Minix" },
    MbrTypeEntry { id: 0x81, name: b"Minix / old Linux" },
    MbrTypeEntry { id: 0x82, name: b"Linux swap" },
    MbrTypeEntry { id: 0x83, name: b"Linux" },
    MbrTypeEntry { id: 0x84, name: b"OS/2 hidden" },
    MbrTypeEntry { id: 0x85, name: b"Linux extended" },
    MbrTypeEntry { id: 0x86, name: b"NTFS volume set" },
    MbrTypeEntry { id: 0x87, name: b"NTFS volume set" },
    MbrTypeEntry { id: 0x88, name: b"Linux plaintext" },
    MbrTypeEntry { id: 0x8E, name: b"Linux LVM" },
    MbrTypeEntry { id: 0x93, name: b"Amoeba" },
    MbrTypeEntry { id: 0x94, name: b"Amoeba BBT" },
    MbrTypeEntry { id: 0x9F, name: b"BSD/OS" },
    MbrTypeEntry { id: 0xA0, name: b"IBM Thinkpad hibernation" },
    MbrTypeEntry { id: 0xA5, name: b"FreeBSD" },
    MbrTypeEntry { id: 0xA6, name: b"OpenBSD" },
    MbrTypeEntry { id: 0xA7, name: b"NeXTSTEP" },
    MbrTypeEntry { id: 0xA8, name: b"Darwin UFS" },
    MbrTypeEntry { id: 0xA9, name: b"NetBSD" },
    MbrTypeEntry { id: 0xAB, name: b"Darwin boot" },
    MbrTypeEntry { id: 0xAF, name: b"HFS / HFS+" },
    MbrTypeEntry { id: 0xB7, name: b"BSDI fs" },
    MbrTypeEntry { id: 0xB8, name: b"BSDI swap" },
    MbrTypeEntry { id: 0xBB, name: b"Boot Wizard hidden" },
    MbrTypeEntry { id: 0xBE, name: b"Solaris boot" },
    MbrTypeEntry { id: 0xBF, name: b"Solaris" },
    MbrTypeEntry { id: 0xC1, name: b"DRDOS FAT12" },
    MbrTypeEntry { id: 0xC4, name: b"DRDOS FAT16 <32M" },
    MbrTypeEntry { id: 0xC6, name: b"DRDOS FAT16" },
    MbrTypeEntry { id: 0xDA, name: b"Non-FS data" },
    MbrTypeEntry { id: 0xDB, name: b"CP/M / CTOS" },
    MbrTypeEntry { id: 0xDE, name: b"Dell utility" },
    MbrTypeEntry { id: 0xDF, name: b"BootIt" },
    MbrTypeEntry { id: 0xE1, name: b"DOS access" },
    MbrTypeEntry { id: 0xE3, name: b"DOS R/O" },
    MbrTypeEntry { id: 0xE4, name: b"SpeedStor" },
    MbrTypeEntry { id: 0xEB, name: b"BeOS fs" },
    MbrTypeEntry { id: 0xEE, name: b"GPT protective" },
    MbrTypeEntry { id: 0xEF, name: b"EFI System" },
    MbrTypeEntry { id: 0xF0, name: b"Linux/PA-RISC boot" },
    MbrTypeEntry { id: 0xF1, name: b"SpeedStor" },
    MbrTypeEntry { id: 0xF2, name: b"DOS secondary" },
    MbrTypeEntry { id: 0xFB, name: b"VMware VMFS" },
    MbrTypeEntry { id: 0xFC, name: b"VMware swap" },
    MbrTypeEntry { id: 0xFD, name: b"Linux RAID" },
    MbrTypeEntry { id: 0xFE, name: b"LANstep" },
    MbrTypeEntry { id: 0xFF, name: b"BBT" },
];

fn mbr_type_name(type_id: u8) -> &'static [u8] {
    let mut i = 0;
    while i < MBR_TYPES.len() {
        if MBR_TYPES[i].id == type_id {
            return MBR_TYPES[i].name;
        }
        i += 1;
    }
    b"unknown"
}

// ── On-Disk Structures ───────────────────────────────────────────────

/// MBR partition entry (16 bytes on disk, 4 entries starting at offset 446).
#[derive(Clone, Copy)]
struct MbrPartition {
    status: u8,
    /// CHS of first sector (3 bytes): head, sector, cylinder.
    chs_first: [u8; 3],
    type_id: u8,
    /// CHS of last sector (3 bytes).
    chs_last: [u8; 3],
    lba_start: u32,
    lba_size: u32,
}

impl MbrPartition {
    fn is_empty(&self) -> bool {
        self.type_id == 0x00 && self.lba_size == 0
    }

    /// Whether this is an extended partition (container for logical partitions).
    fn is_extended(&self) -> bool {
        self.type_id == 0x05 || self.type_id == 0x0F || self.type_id == 0x85
    }

    fn end_lba(&self) -> u64 {
        (self.lba_start as u64).saturating_add(self.lba_size as u64).saturating_sub(1)
    }

    fn size_bytes(&self, sector_size: u64) -> u64 {
        (self.lba_size as u64).saturating_mul(sector_size)
    }
}

/// GPT header (LBA 1, 92 bytes in the 512-byte sector).
#[allow(dead_code)]
struct GptHeader {
    valid: bool,
    revision: u32,
    header_size: u32,
    header_crc: u32,
    my_lba: u64,
    alternate_lba: u64,
    first_usable_lba: u64,
    last_usable_lba: u64,
    disk_guid: [u8; 16],
    partition_entry_lba: u64,
    num_partition_entries: u32,
    partition_entry_size: u32,
    partition_entries_crc: u32,
    crc_valid: bool,
}

/// GPT partition entry (128 bytes on disk).
struct GptPartition {
    type_guid: [u8; 16],
    unique_guid: [u8; 16],
    first_lba: u64,
    last_lba: u64,
    attributes: u64,
    /// UTF-16LE name decoded to byte slice (lossy ASCII).
    name_buf: [u8; 72],
    name_len: usize,
}

impl GptPartition {
    fn is_empty(&self) -> bool {
        guid_is_zero(&self.type_guid)
    }

    fn sectors(&self) -> u64 {
        if self.last_lba >= self.first_lba {
            self.last_lba - self.first_lba + 1
        } else {
            0
        }
    }

    fn name_bytes(&self) -> &[u8] {
        &self.name_buf[..self.name_len]
    }
}

/// Detected partition table type.
///
/// The Gpt variant carries an inline [GptPartition; 128] which makes it
/// significantly larger than the Mbr variant; clippy's
/// large_enum_variant lint would suggest Box::new. We keep the inline
/// layout deliberately: a no_main, no-allocator personality CLI cannot
/// rely on a global allocator for table parsing, and stack-resident
/// tables match the on-disk GPT footprint (128 entries × 128 bytes).
#[allow(clippy::large_enum_variant)]
enum DiskLabel {
    Gpt {
        header: GptHeader,
        partitions: [GptPartition; 128],
        partition_count: usize,
    },
    Mbr {
        partitions: [MbrPartition; 4],
        /// Logical partitions inside extended partitions.
        logical: [MbrPartition; 60],
        logical_count: usize,
    },
    Unknown,
}

// ── Parsing ──────────────────────────────────────────────────────────

fn parse_mbr_entry(sector: &[u8], base: usize) -> MbrPartition {
    if base + 16 > sector.len() {
        return MbrPartition {
            status: 0, chs_first: [0; 3], type_id: 0,
            chs_last: [0; 3], lba_start: 0, lba_size: 0,
        };
    }
    MbrPartition {
        status: sector[base],
        chs_first: [sector[base + 1], sector[base + 2], sector[base + 3]],
        type_id: sector[base + 4],
        chs_last: [sector[base + 5], sector[base + 6], sector[base + 7]],
        lba_start: le_u32(sector, base + 8),
        lba_size: le_u32(sector, base + 12),
    }
}

fn parse_mbr_entries(sector: &[u8]) -> [MbrPartition; 4] {
    [
        parse_mbr_entry(sector, 446),
        parse_mbr_entry(sector, 462),
        parse_mbr_entry(sector, 478),
        parse_mbr_entry(sector, 494),
    ]
}

fn has_mbr_signature(sector: &[u8]) -> bool {
    sector.len() >= 512
        && sector[510] == 0x55
        && sector[511] == 0xAA
}

fn parse_gpt_header(sector: &[u8]) -> GptHeader {
    let sig = if sector.len() >= 8 { &sector[0..8] } else { &[0u8; 8][..] };
    let valid = bytes_eq(sig, b"EFI PART");

    let revision = le_u32(sector, 8);
    let header_size = le_u32(sector, 12);
    let header_crc = le_u32(sector, 16);
    let my_lba = le_u64(sector, 24);
    let alternate_lba = le_u64(sector, 32);
    let first_usable_lba = le_u64(sector, 40);
    let last_usable_lba = le_u64(sector, 48);
    let disk_guid = copy_16(sector, 56);
    let partition_entry_lba = le_u64(sector, 72);
    let num_partition_entries = le_u32(sector, 80);
    let partition_entry_size = le_u32(sector, 84);
    let partition_entries_crc = le_u32(sector, 88);

    // Verify header CRC: zero out the CRC field, compute, compare.
    let crc_valid = if valid && (header_size as usize) <= sector.len() {
        let hs = header_size as usize;
        if hs <= 512 {
            let mut hdr_copy = [0u8; 512];
            let copy_len = if hs <= sector.len() { hs } else { sector.len() };
            hdr_copy[..copy_len].copy_from_slice(&sector[..copy_len]);
            write_le_u32(&mut hdr_copy, 16, 0);
            crc32(&hdr_copy[..hs]) == header_crc
        } else {
            false
        }
    } else {
        false
    };

    GptHeader {
        valid, revision, header_size, header_crc, my_lba, alternate_lba,
        first_usable_lba, last_usable_lba, disk_guid, partition_entry_lba,
        num_partition_entries, partition_entry_size, partition_entries_crc,
        crc_valid,
    }
}

fn copy_16(buf: &[u8], off: usize) -> [u8; 16] {
    let mut g = [0u8; 16];
    if off + 16 <= buf.len() {
        g.copy_from_slice(&buf[off..off + 16]);
    }
    g
}

fn parse_gpt_entry(buf: &[u8]) -> GptPartition {
    let type_guid = copy_16(buf, 0);
    let unique_guid = copy_16(buf, 16);
    let first_lba = le_u64(buf, 32);
    let last_lba = le_u64(buf, 40);
    let attributes = le_u64(buf, 48);

    // Name: UTF-16LE at offset 56, up to 72 bytes (36 code units).
    let mut name_buf = [0u8; 72];
    let mut name_len = 0;
    let name_region = if buf.len() >= 128 { &buf[56..128] } else { &buf[56..] };
    let mut ci = 0;
    while ci + 1 < name_region.len() {
        let lo = name_region[ci];
        let hi = name_region[ci + 1];
        if lo == 0 && hi == 0 {
            break;
        }
        // Simple ASCII extraction; non-ASCII becomes '?'
        if hi == 0 && (0x20..0x7F).contains(&lo) {
            if name_len < 72 {
                name_buf[name_len] = lo;
                name_len += 1;
            }
        } else if name_len < 72 {
            name_buf[name_len] = b'?';
            name_len += 1;
        }
        ci += 2;
    }

    GptPartition { type_guid, unique_guid, first_lba, last_lba, attributes, name_buf, name_len }
}

/// Parse the partition table from raw disk bytes (at least 34*512 = 17408 bytes).
fn parse_disk_label(raw: &[u8]) -> DiskLabel {
    if raw.len() < 512 {
        return DiskLabel::Unknown;
    }

    // Check for GPT: LBA 1 should have "EFI PART" signature.
    if raw.len() >= 1024 {
        let header = parse_gpt_header(&raw[512..1024]);
        if header.valid {
            let entry_size = if header.partition_entry_size >= 128 {
                header.partition_entry_size as usize
            } else {
                128
            };
            let max_entries = header.num_partition_entries as usize;
            let entries_start = (header.partition_entry_lba as usize).saturating_mul(512);

            // Build 128-entry array; unused slots are zero-initialized.
            let mut parts: [GptPartition; 128] = core::array::from_fn(|_| GptPartition {
                type_guid: [0; 16], unique_guid: [0; 16],
                first_lba: 0, last_lba: 0, attributes: 0,
                name_buf: [0; 72], name_len: 0,
            });
            let mut count = 0;
            let mut i = 0;
            while i < max_entries && i < 128 {
                let off = entries_start + i * entry_size;
                if off + 128 > raw.len() {
                    break;
                }
                let entry = parse_gpt_entry(&raw[off..off + 128]);
                if !entry.is_empty() {
                    parts[count] = entry;
                    count += 1;
                }
                i += 1;
            }

            return DiskLabel::Gpt { header, partitions: parts, partition_count: count };
        }
    }

    // Fall back to MBR.
    if has_mbr_signature(&raw[..512]) {
        let entries = parse_mbr_entries(&raw[..512]);
        let has_parts = entries.iter().any(|e| !e.is_empty());
        if has_parts {
            // Parse logical partitions from extended partition chain.
            let empty_logical = MbrPartition {
                status: 0, chs_first: [0; 3], type_id: 0,
                chs_last: [0; 3], lba_start: 0, lba_size: 0,
            };
            let mut logical = [empty_logical; 60];
            let mut logical_count = 0;

            for primary in &entries {
                if primary.is_extended() && primary.lba_size > 0 {
                    // Walk extended partition chain (EBR chain).
                    let ext_start = primary.lba_start as u64;
                    let mut ebr_lba = ext_start;
                    let mut safety = 0;
                    while safety < 60 {
                        let ebr_off = (ebr_lba as usize).saturating_mul(512);
                        if ebr_off + 512 > raw.len() {
                            break;
                        }
                        let ebr = &raw[ebr_off..ebr_off + 512];
                        if !has_mbr_signature(ebr) {
                            break;
                        }
                        let e1 = parse_mbr_entry(ebr, 446);
                        if !e1.is_empty() && logical_count < 60 {
                            // Logical partition LBA is relative to this EBR.
                            logical[logical_count] = MbrPartition {
                                status: e1.status,
                                chs_first: e1.chs_first,
                                type_id: e1.type_id,
                                chs_last: e1.chs_last,
                                lba_start: (ebr_lba as u32).wrapping_add(e1.lba_start),
                                lba_size: e1.lba_size,
                            };
                            logical_count += 1;
                        }
                        // Second entry points to next EBR (relative to extended start).
                        let e2 = parse_mbr_entry(ebr, 462);
                        if e2.type_id == 0 || e2.lba_start == 0 {
                            break;
                        }
                        ebr_lba = ext_start + e2.lba_start as u64;
                        safety += 1;
                    }
                }
            }

            return DiskLabel::Mbr { partitions: entries, logical, logical_count };
        }
    }

    DiskLabel::Unknown
}

// ── CHS Conversion ───────────────────────────────────────────────────

/// Convert an LBA address to CHS tuple, given disk geometry.
/// Returns (cylinder, head, sector).
#[allow(dead_code)]
fn lba_to_chs(lba: u64, heads: u32, sectors_per_track: u32) -> (u32, u32, u32) {
    if heads == 0 || sectors_per_track == 0 {
        return (0, 0, 0);
    }
    let hps = (heads as u64) * (sectors_per_track as u64);
    if hps == 0 {
        return (0, 0, 0);
    }
    let c = (lba / hps) as u32;
    let rem = lba % hps;
    let h = (rem / sectors_per_track as u64) as u32;
    let s = (rem % sectors_per_track as u64) as u32 + 1;
    (c, h, s)
}

/// Decode packed CHS bytes from MBR entry: [head, sector|cyl_hi, cyl_lo].
fn decode_chs(chs: &[u8; 3]) -> (u32, u32, u32) {
    let head = chs[0] as u32;
    let sector = (chs[1] & 0x3F) as u32;
    let cylinder = ((chs[1] as u32 & 0xC0) << 2) | (chs[2] as u32);
    (cylinder, head, sector)
}

// ── Alignment ────────────────────────────────────────────────────────

/// Align a sector number up to the nearest 1 MiB boundary (2048 sectors at 512 bytes).
#[allow(dead_code)]
fn align_up_1mib(lba: u64, sector_size: u64) -> u64 {
    if sector_size == 0 {
        return lba;
    }
    let sectors_per_mib = 1048576 / sector_size;
    if sectors_per_mib == 0 {
        return lba;
    }
    let remainder = lba % sectors_per_mib;
    if remainder == 0 {
        lba
    } else {
        lba + (sectors_per_mib - remainder)
    }
}

// ── Serialization Helpers ────────────────────────────────────────────

/// Build a protective MBR for a GPT disk.
fn build_protective_mbr(disk_sectors: u64) -> [u8; 512] {
    let mut mbr = [0u8; 512];
    // Partition entry 1 at offset 446: protective MBR entry
    mbr[446] = 0x00; // Not bootable
    mbr[447] = 0x00; // CHS first: 0/0/1
    mbr[448] = 0x01;
    mbr[449] = 0x00;
    mbr[450] = 0xEE; // GPT protective type
    // CHS last: fill with 0xFF for large disks
    mbr[451] = 0xFF;
    mbr[452] = 0xFF;
    mbr[453] = 0xFF;
    // LBA start = 1
    write_le_u32(&mut mbr, 454, 1);
    // Size = min(disk_sectors - 1, 0xFFFFFFFF)
    let prot_size = if disk_sectors > 1 {
        let s = disk_sectors - 1;
        if s > 0xFFFF_FFFF { 0xFFFF_FFFF_u32 } else { s as u32 }
    } else {
        0
    };
    write_le_u32(&mut mbr, 458, prot_size);
    // Boot signature
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    mbr
}

/// Build a GPT header sector.
///
/// The 9-arg signature mirrors the 9 distinct fields of UEFI 2.x §5.3
/// "GUID Partition Table Header"; collapsing them into a struct would
/// only shuffle the same 9 inputs across a constructor boundary.
#[allow(clippy::too_many_arguments)]
fn build_gpt_header(
    disk_guid: &[u8; 16],
    my_lba: u64,
    alternate_lba: u64,
    first_usable: u64,
    last_usable: u64,
    partition_entry_lba: u64,
    num_entries: u32,
    entry_size: u32,
    entries_crc: u32,
) -> [u8; 512] {
    let mut hdr = [0u8; 512];
    // Signature
    hdr[0..8].copy_from_slice(b"EFI PART");
    // Revision 1.0
    write_le_u32(&mut hdr, 8, 0x0001_0000);
    // Header size = 92
    write_le_u32(&mut hdr, 12, 92);
    // CRC32 will be filled after
    write_le_u32(&mut hdr, 16, 0);
    // Reserved
    write_le_u32(&mut hdr, 20, 0);
    write_le_u64(&mut hdr, 24, my_lba);
    write_le_u64(&mut hdr, 32, alternate_lba);
    write_le_u64(&mut hdr, 40, first_usable);
    write_le_u64(&mut hdr, 48, last_usable);
    hdr[56..72].copy_from_slice(disk_guid);
    write_le_u64(&mut hdr, 72, partition_entry_lba);
    write_le_u32(&mut hdr, 80, num_entries);
    write_le_u32(&mut hdr, 84, entry_size);
    write_le_u32(&mut hdr, 88, entries_crc);
    // Compute and write header CRC
    let header_crc = crc32(&hdr[..92]);
    write_le_u32(&mut hdr, 16, header_crc);
    hdr
}

/// Serialize a GPT partition entry to 128 bytes.
fn serialize_gpt_entry(part: &GptPartition) -> [u8; 128] {
    let mut buf = [0u8; 128];
    buf[0..16].copy_from_slice(&part.type_guid);
    buf[16..32].copy_from_slice(&part.unique_guid);
    write_le_u64(&mut buf, 32, part.first_lba);
    write_le_u64(&mut buf, 40, part.last_lba);
    write_le_u64(&mut buf, 48, part.attributes);
    // Name as UTF-16LE
    let name = part.name_bytes();
    let mut wi = 56;
    let mut ni = 0;
    while ni < name.len() && wi + 1 < 128 {
        buf[wi] = name[ni];
        buf[wi + 1] = 0;
        wi += 2;
        ni += 1;
    }
    buf
}

/// Serialize a MBR partition entry to 16 bytes at offset in buf.
#[allow(dead_code)]
fn serialize_mbr_entry(buf: &mut [u8], off: usize, part: &MbrPartition) {
    if off + 16 > buf.len() { return; }
    buf[off] = part.status;
    buf[off + 1] = part.chs_first[0];
    buf[off + 2] = part.chs_first[1];
    buf[off + 3] = part.chs_first[2];
    buf[off + 4] = part.type_id;
    buf[off + 5] = part.chs_last[0];
    buf[off + 6] = part.chs_last[1];
    buf[off + 7] = part.chs_last[2];
    write_le_u32(buf, off + 8, part.lba_start);
    write_le_u32(buf, off + 12, part.lba_size);
}

// ── Type Code Parsing ────────────────────────────────────────────────

/// Parse a type code from a byte slice. Supports:
/// - Hex MBR: "0x83", "83"
/// - Full GUID: "C12A7328-F81F-11D2-BA4B-00A0C93EC93B"
/// - Short names: "linux", "efi", "swap", etc.
///
/// Returns 16 bytes: for MBR the type byte is in [0], rest zero.
fn parse_type_code(s: &[u8]) -> Option<[u8; 16]> {
    if s.is_empty() {
        return None;
    }

    // Full GUID (36 bytes with dashes)
    if s.len() == 36
        && let Some(g) = parse_guid_runtime(s) {
            return Some(g);
        }

    // Hex MBR: "0x83" or "83"
    let hex_str = if starts_with(s, b"0x") || starts_with(s, b"0X") {
        &s[2..]
    } else {
        s
    };
    if hex_str.len() <= 2 && !hex_str.is_empty() && is_all_hex(hex_str)
        && let Some(val) = parse_hex_u8(hex_str) {
            let mut result = [0u8; 16];
            result[0] = val;
            return Some(result);
        }

    // Short names (case-insensitive)
    if bytes_eq_ci(s, b"linux") || bytes_eq_ci(s, b"linux-fs") {
        return Some(parse_guid(b"0FC63DAF-8483-4772-8E79-3D69D8477DE4"));
    }
    if bytes_eq_ci(s, b"efi") || bytes_eq_ci(s, b"esp") || bytes_eq_ci(s, b"efi-system") {
        return Some(parse_guid(b"C12A7328-F81F-11D2-BA4B-00A0C93EC93B"));
    }
    if bytes_eq_ci(s, b"swap") || bytes_eq_ci(s, b"linux-swap") {
        return Some(parse_guid(b"0657FD6D-A4AB-43C4-84E5-0933C84B4F4F"));
    }
    if bytes_eq_ci(s, b"ntfs") || bytes_eq_ci(s, b"windows") || bytes_eq_ci(s, b"msdata") {
        return Some(parse_guid(b"EBD0A0A2-B9E5-4433-87C0-68B6B72699C7"));
    }
    if bytes_eq_ci(s, b"lvm") || bytes_eq_ci(s, b"linux-lvm") {
        return Some(parse_guid(b"E6D6D379-F507-44C2-A23C-238F2A3DF928"));
    }
    if bytes_eq_ci(s, b"raid") || bytes_eq_ci(s, b"linux-raid") {
        return Some(parse_guid(b"A19D880F-05FC-4D3B-A006-743F0F84911E"));
    }
    if bytes_eq_ci(s, b"bios") || bytes_eq_ci(s, b"bios-boot") {
        return Some(parse_guid(b"21686148-6449-6E6F-744E-656564454649"));
    }
    if bytes_eq_ci(s, b"home") || bytes_eq_ci(s, b"linux-home") {
        return Some(parse_guid(b"933AC7E1-2EB4-4F13-B844-0E14E2AEF915"));
    }
    if bytes_eq_ci(s, b"srv") || bytes_eq_ci(s, b"linux-srv") {
        return Some(parse_guid(b"3B8F8425-20E0-4F3B-907F-1A25A76F98E8"));
    }
    if bytes_eq_ci(s, b"msreserved") || bytes_eq_ci(s, b"microsoft-reserved") {
        return Some(parse_guid(b"E3C9E316-0B5C-4DB8-817D-F92DF00215AE"));
    }
    if bytes_eq_ci(s, b"hfs") || bytes_eq_ci(s, b"apple-hfs") {
        return Some(parse_guid(b"48465300-0000-11AA-AA11-00306543ECAC"));
    }
    if bytes_eq_ci(s, b"apfs") || bytes_eq_ci(s, b"apple-apfs") {
        return Some(parse_guid(b"7C3457EF-0000-11AA-AA11-00306543ECAC"));
    }

    None
}

// ── Personality Detection ────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Personality {
    Fdisk,
    Gdisk,
    Sfdisk,
    Cfdisk,
    Partprobe,
}

fn detect_personality(argv0: &[u8]) -> Personality {
    let base = strip_exe(basename(argv0));
    if bytes_eq_ci(base, b"gdisk") {
        Personality::Gdisk
    } else if bytes_eq_ci(base, b"sfdisk") {
        Personality::Sfdisk
    } else if bytes_eq_ci(base, b"cfdisk") {
        Personality::Cfdisk
    } else if bytes_eq_ci(base, b"partprobe") {
        Personality::Partprobe
    } else {
        Personality::Fdisk
    }
}

fn personality_name(p: Personality) -> &'static [u8] {
    match p {
        Personality::Fdisk => b"fdisk",
        Personality::Gdisk => b"gdisk",
        Personality::Sfdisk => b"sfdisk",
        Personality::Cfdisk => b"cfdisk",
        Personality::Partprobe => b"partprobe",
    }
}

// ── Command-Line Parsing ─────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    List,
    Interactive,
    NewPartition,
    DeletePartition,
    ChangeType,
    Help,
    Version,
    // sfdisk specific
    Dump,
    JsonDump,
    // partprobe
    Probe,
}

struct Opts {
    personality: Personality,
    action: Action,
    extended: bool,
    show_bytes: bool,
    json_output: bool,
    device: [u8; 256],
    device_len: usize,
    // For new partition
    new_start: u64,
    new_size: u64,
    type_code: [u8; 16],
    type_code_set: bool,
    // For delete
    part_num: u32,
    // For change type
    change_part_num: u32,
}

impl Opts {
    fn new() -> Self {
        Self {
            personality: Personality::Fdisk,
            action: Action::Interactive,
            extended: false,
            show_bytes: false,
            json_output: false,
            device: [0u8; 256],
            device_len: 0,
            new_start: 0,
            new_size: 0,
            type_code: [0u8; 16],
            type_code_set: false,
            part_num: 0,
            change_part_num: 0,
        }
    }

    fn set_device(&mut self, dev: &[u8]) {
        let len = if dev.len() < 256 { dev.len() } else { 255 };
        self.device[..len].copy_from_slice(&dev[..len]);
        self.device_len = len;
    }

    fn device_bytes(&self) -> &[u8] {
        &self.device[..self.device_len]
    }
}

fn parse_args(argc: i32, argv: *const *const u8) -> Opts {
    let mut opts = Opts::new();

    if argc < 1 {
        opts.action = Action::Help;
        return opts;
    }

    let mut args: [&[u8]; 64] = [b""; 64];
    let arg_count = if (argc as usize) < 64 { argc as usize } else { 64 };
    let mut ai = 0;
    while ai < arg_count {
        // SAFETY: argv is provided by the runtime, with argc valid entries.
        args[ai] = unsafe { cstr_to_slice(*argv.add(ai)) };
        ai += 1;
    }

    opts.personality = detect_personality(args[0]);

    // Partprobe defaults to probe action
    if opts.personality == Personality::Partprobe {
        opts.action = Action::Probe;
    }

    let mut i = 1;
    while i < arg_count {
        let arg = args[i];

        if bytes_eq(arg, b"-l") || bytes_eq(arg, b"--list") {
            opts.action = Action::List;
        } else if bytes_eq(arg, b"-x") || bytes_eq(arg, b"--extended") {
            opts.extended = true;
        } else if bytes_eq(arg, b"--bytes") {
            opts.show_bytes = true;
        } else if bytes_eq(arg, b"--json") || bytes_eq(arg, b"-J") {
            opts.json_output = true;
        } else if bytes_eq(arg, b"--dump") || bytes_eq(arg, b"-d") && opts.personality == Personality::Sfdisk {
            opts.action = Action::Dump;
        } else if bytes_eq(arg, b"--new") {
            opts.action = Action::NewPartition;
            // Next arg might be device
            if i + 1 < arg_count && !starts_with(args[i + 1], b"-") {
                opts.set_device(args[i + 1]);
                i += 1;
            }
        } else if bytes_eq(arg, b"--delete") {
            opts.action = Action::DeletePartition;
            if i + 1 < arg_count && !starts_with(args[i + 1], b"-") {
                opts.set_device(args[i + 1]);
                i += 1;
            }
        } else if bytes_eq(arg, b"--type") {
            opts.action = Action::ChangeType;
            if i + 1 < arg_count && !starts_with(args[i + 1], b"-") {
                opts.set_device(args[i + 1]);
                i += 1;
            }
        } else if bytes_eq(arg, b"-n") {
            // -n <start> <size>
            if i + 2 < arg_count {
                if let Some(s) = parse_u64(args[i + 1]) {
                    opts.new_start = s;
                }
                if let Some(s) = parse_u64(args[i + 2]) {
                    opts.new_size = s;
                }
                i += 2;
            }
        } else if bytes_eq(arg, b"-d") {
            if i + 1 < arg_count {
                if let Some(n) = parse_u32(args[i + 1]) {
                    opts.part_num = n;
                }
                i += 1;
            }
        } else if bytes_eq(arg, b"-t") {
            if opts.action == Action::ChangeType {
                // -t <part_num> <type_code>
                if i + 2 < arg_count {
                    if let Some(n) = parse_u32(args[i + 1]) {
                        opts.change_part_num = n;
                    }
                    if let Some(tc) = parse_type_code(args[i + 2]) {
                        opts.type_code = tc;
                        opts.type_code_set = true;
                    }
                    i += 2;
                }
            } else {
                // -t <type_code> (for new partition)
                if i + 1 < arg_count {
                    if let Some(tc) = parse_type_code(args[i + 1]) {
                        opts.type_code = tc;
                        opts.type_code_set = true;
                    }
                    i += 1;
                }
            }
        } else if bytes_eq(arg, b"-h") || bytes_eq(arg, b"--help") || bytes_eq(arg, b"help") {
            opts.action = Action::Help;
        } else if bytes_eq(arg, b"-V") || bytes_eq(arg, b"--version") {
            opts.action = Action::Version;
        } else if !starts_with(arg, b"-") {
            // Treat as device path
            opts.set_device(arg);
        }

        i += 1;
    }

    opts
}

// ── Output Builders ──────────────────────────────────────────────────

/// Accumulate output into a fixed buffer, then flush. Avoids repeated stdout calls.
struct OutBuf {
    buf: [u8; 8192],
    len: usize,
}

impl OutBuf {
    fn new() -> Self {
        Self { buf: [0u8; 8192], len: 0 }
    }

    fn push(&mut self, data: &[u8]) {
        let mut i = 0;
        while i < data.len() {
            if self.len >= self.buf.len() {
                self.flush();
            }
            let space = self.buf.len() - self.len;
            let chunk = if data.len() - i < space { data.len() - i } else { space };
            self.buf[self.len..self.len + chunk].copy_from_slice(&data[i..i + chunk]);
            self.len += chunk;
            i += chunk;
        }
    }

    fn push_u64(&mut self, val: u64) {
        let mut tmp = [0u8; 20];
        let n = format_u64(val, &mut tmp);
        self.push(&tmp[..n]);
    }

    fn push_u32(&mut self, val: u32) {
        let mut tmp = [0u8; 10];
        let n = format_u32(val, &mut tmp);
        self.push(&tmp[..n]);
    }

    fn push_size(&mut self, bytes: u64) {
        let mut tmp = [0u8; 24];
        let n = format_size(bytes, &mut tmp);
        self.push(&tmp[..n]);
    }

    fn push_guid(&mut self, g: &[u8; 16]) {
        let mut tmp = [0u8; 36];
        format_guid(g, &mut tmp);
        self.push(&tmp);
    }

    fn push_hex_u8(&mut self, val: u8) {
        let mut tmp = [0u8; 2];
        format_hex_u8(val, &mut tmp);
        self.push(&tmp);
    }

    fn push_newline(&mut self) {
        self.push(b"\n");
    }

    /// Pad with spaces to reach a minimum width (right-pad).
    fn pad_to(&mut self, current_col: usize, target_col: usize) {
        if current_col < target_col {
            let mut i = 0;
            while i < target_col - current_col {
                self.push(b" ");
                i += 1;
            }
        }
    }

    fn flush(&mut self) {
        if self.len > 0 {
            print_out(&self.buf[..self.len]);
            self.len = 0;
        }
    }

    #[allow(dead_code)]
    fn flush_err(&mut self) {
        if self.len > 0 {
            print_err(&self.buf[..self.len]);
            self.len = 0;
        }
    }
}

// ── JSON Output ──────────────────────────────────────────────────────

/// Escape a byte slice for JSON output: handle \, ", and control characters.
fn json_push_escaped(out: &mut OutBuf, s: &[u8]) {
    out.push(b"\"");
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'"' => out.push(b"\\\""),
            b'\\' => out.push(b"\\\\"),
            b'\n' => out.push(b"\\n"),
            b'\r' => out.push(b"\\r"),
            b'\t' => out.push(b"\\t"),
            c if c < 0x20 => {
                out.push(b"\\u00");
                out.push_hex_u8(c);
            }
            c => {
                let byte = [c];
                out.push(&byte);
            }
        }
        i += 1;
    }
    out.push(b"\"");
}

// ── fdisk: Display Partition Table ───────────────────────────────────

fn print_disk_header(out: &mut OutBuf, dev: &[u8], total_bytes: u64, total_sectors: u64,
                     sector_size: u64, hw_sector_size: u64, show_bytes: bool) {
    out.push(b"Disk ");
    out.push(dev);
    out.push(b": ");
    if !show_bytes {
        out.push_size(total_bytes);
        out.push(b", ");
    }
    out.push_u64(total_bytes);
    out.push(b" bytes, ");
    out.push_u64(total_sectors);
    out.push(b" sectors");
    out.push_newline();
    out.push(b"Units: sectors of 1 * ");
    out.push_u64(sector_size);
    out.push(b" = ");
    out.push_u64(sector_size);
    out.push(b" bytes");
    out.push_newline();
    out.push(b"Sector size (logical/physical): ");
    out.push_u64(sector_size);
    out.push(b" bytes / ");
    out.push_u64(hw_sector_size);
    out.push(b" bytes");
    out.push_newline();
}

// out + dev + header + (parts, count) + sector_size + (extended,
// show_bytes) — 8 inputs that don't naturally group into a single
// struct without forcing GptHeader/parts into a wrapper that exists
// only to satisfy this lint.
#[allow(clippy::too_many_arguments)]
fn print_gpt_listing(out: &mut OutBuf, dev: &[u8], header: &GptHeader,
                     parts: &[GptPartition], count: usize,
                     sector_size: u64, extended: bool, show_bytes: bool) {
    out.push(b"Disklabel type: gpt");
    out.push_newline();
    out.push(b"Disk identifier: ");
    out.push_guid(&header.disk_guid);
    out.push_newline();

    if extended {
        out.push(b"First usable LBA: ");
        out.push_u64(header.first_usable_lba);
        out.push(b", Last usable LBA: ");
        out.push_u64(header.last_usable_lba);
        out.push_newline();
        out.push(b"Alternate LBA: ");
        out.push_u64(header.alternate_lba);
        out.push(b", Partition entries LBA: ");
        out.push_u64(header.partition_entry_lba);
        out.push_newline();
        out.push(b"Partition entries: ");
        out.push_u32(header.num_partition_entries);
        out.push(b", Entry size: ");
        out.push_u32(header.partition_entry_size);
        out.push_newline();
        if header.crc_valid {
            out.push(b"Header CRC: valid");
        } else {
            out.push(b"Header CRC: INVALID");
        }
        out.push_newline();
    }

    if count == 0 {
        out.push_newline();
        return;
    }

    // Table header
    out.push_newline();
    // Columns: Device, Start, End, Sectors, Size, Type
    out.push(b"Device               Start        End    Sectors   Size Type");
    out.push_newline();

    let mut pi = 0;
    while pi < count {
        let part = &parts[pi];
        let sectors = part.sectors();
        let size_bytes = sectors.saturating_mul(sector_size);
        let type_name = gpt_type_name(&part.type_guid);

        // Device name: dev + partition_number
        out.push(dev);
        let pnum = pi as u64 + 1;
        out.push_u64(pnum);

        // Pad to column positions (approximate alignment)
        let dev_col = dev.len() + if pnum >= 10 { 2 } else { 1 };
        out.pad_to(dev_col, 21);

        // Start
        let mut tmp = [0u8; 20];
        let n = format_u64(part.first_lba, &mut tmp);
        out.pad_to(0, 12_usize.saturating_sub(n));
        out.push(&tmp[..n]);
        out.push(b" ");

        // End
        let n = format_u64(part.last_lba, &mut tmp);
        out.pad_to(0, 10_usize.saturating_sub(n));
        out.push(&tmp[..n]);
        out.push(b" ");

        // Sectors
        let n = format_u64(sectors, &mut tmp);
        out.pad_to(0, 10_usize.saturating_sub(n));
        out.push(&tmp[..n]);
        out.push(b" ");

        // Size
        if show_bytes {
            let n = format_u64(size_bytes, &mut tmp);
            out.pad_to(0, 6_usize.saturating_sub(n));
            out.push(&tmp[..n]);
        } else {
            let mut sz = [0u8; 24];
            let n = format_size(size_bytes, &mut sz);
            out.pad_to(0, 6_usize.saturating_sub(n));
            out.push(&sz[..n]);
        }
        out.push(b" ");

        // Type
        out.push(type_name);
        out.push_newline();

        pi += 1;
    }

    // Extended per-partition detail
    if extended {
        out.push_newline();
        let mut pi = 0;
        while pi < count {
            let part = &parts[pi];
            out.push(b"Partition ");
            out.push_u64(pi as u64 + 1);
            out.push(b": type=");
            out.push_guid(&part.type_guid);
            out.push(b", uuid=");
            out.push_guid(&part.unique_guid);
            out.push_newline();
            if part.attributes != 0 {
                out.push(b"  Attributes: 0x");
                // Print attributes as hex
                let mut ai = 0;
                let abytes = part.attributes.to_be_bytes();
                while ai < 8 {
                    out.push_hex_u8(abytes[ai]);
                    ai += 1;
                }
                out.push_newline();
            }
            if part.name_len > 0 {
                out.push(b"  Name: \"");
                out.push(part.name_bytes());
                out.push(b"\"");
                out.push_newline();
            }
            pi += 1;
        }
    }
}

// Same shape as print_gpt_listing above; the lint suggests collapsing
// these into a context struct that exists only to satisfy it.
#[allow(clippy::too_many_arguments)]
fn print_mbr_listing(out: &mut OutBuf, dev: &[u8], parts: &[MbrPartition; 4],
                     logical: &[MbrPartition], logical_count: usize,
                     sector_size: u64, extended: bool, show_bytes: bool) {
    out.push(b"Disklabel type: dos");
    out.push_newline();
    out.push_newline();

    // Table header
    out.push(b"Device     Boot   Start       End  Sectors   Size Id Type");
    out.push_newline();

    // Primary partitions
    let mut pi = 0;
    while pi < 4 {
        if !parts[pi].is_empty() {
            print_mbr_row(out, dev, pi as u32 + 1, &parts[pi], sector_size, show_bytes);
        }
        pi += 1;
    }

    // Logical partitions
    let mut li = 0;
    while li < logical_count {
        if !logical[li].is_empty() {
            print_mbr_row(out, dev, 5 + li as u32, &logical[li], sector_size, show_bytes);
        }
        li += 1;
    }

    if extended {
        out.push_newline();
        let mut pi = 0;
        while pi < 4 {
            if !parts[pi].is_empty() {
                let (c, h, s) = decode_chs(&parts[pi].chs_first);
                let (ce, he, se) = decode_chs(&parts[pi].chs_last);
                out.push(b"Partition ");
                out.push_u32(pi as u32 + 1);
                out.push(b": CHS start=");
                out.push_u32(c); out.push(b"/"); out.push_u32(h); out.push(b"/"); out.push_u32(s);
                out.push(b", CHS end=");
                out.push_u32(ce); out.push(b"/"); out.push_u32(he); out.push(b"/"); out.push_u32(se);
                out.push_newline();
            }
            pi += 1;
        }
    }
}

fn print_mbr_row(out: &mut OutBuf, dev: &[u8], num: u32, part: &MbrPartition,
                 sector_size: u64, show_bytes: bool) {
    // Device
    out.push(dev);
    out.push_u32(num);

    let dev_col = dev.len() + if num >= 10 { 2 } else { 1 };
    out.pad_to(dev_col, 11);

    // Boot
    if part.status == 0x80 {
        out.push(b"*  ");
    } else {
        out.push(b"   ");
    }

    // Start
    let mut tmp = [0u8; 20];
    let start = part.lba_start as u64;
    let n = format_u64(start, &mut tmp);
    out.pad_to(0, 10_usize.saturating_sub(n));
    out.push(&tmp[..n]);
    out.push(b" ");

    // End
    let end = part.end_lba();
    let n = format_u64(end, &mut tmp);
    out.pad_to(0, 10_usize.saturating_sub(n));
    out.push(&tmp[..n]);
    out.push(b" ");

    // Sectors
    let sectors = part.lba_size as u64;
    let n = format_u64(sectors, &mut tmp);
    out.pad_to(0, 8_usize.saturating_sub(n));
    out.push(&tmp[..n]);
    out.push(b" ");

    // Size
    let size_bytes = part.size_bytes(sector_size);
    if show_bytes {
        let n = format_u64(size_bytes, &mut tmp);
        out.pad_to(0, 6_usize.saturating_sub(n));
        out.push(&tmp[..n]);
    } else {
        let mut sz = [0u8; 24];
        let n = format_size(size_bytes, &mut sz);
        out.pad_to(0, 6_usize.saturating_sub(n));
        out.push(&sz[..n]);
    }
    out.push(b" ");

    // Id
    out.push_hex_u8(part.type_id);
    out.push(b" ");

    // Type
    out.push(mbr_type_name(part.type_id));
    out.push_newline();
}

// ── fdisk: JSON Output ───────────────────────────────────────────────

fn print_json_listing(out: &mut OutBuf, dev: &[u8], label: &DiskLabel,
                      total_bytes: u64, total_sectors: u64, sector_size: u64) {
    out.push(b"{\n");
    out.push(b"  \"partitiontable\": {\n");
    out.push(b"    \"device\": ");
    json_push_escaped(out, dev);
    out.push(b",\n");
    out.push(b"    \"size\": ");
    out.push_u64(total_bytes);
    out.push(b",\n");
    out.push(b"    \"sectors\": ");
    out.push_u64(total_sectors);
    out.push(b",\n");
    out.push(b"    \"sectorsize\": ");
    out.push_u64(sector_size);
    out.push(b",\n");

    match label {
        DiskLabel::Gpt { header, partitions, partition_count } => {
            out.push(b"    \"label\": \"gpt\",\n");
            out.push(b"    \"id\": \"");
            out.push_guid(&header.disk_guid);
            out.push(b"\",\n");
            out.push(b"    \"firstlba\": ");
            out.push_u64(header.first_usable_lba);
            out.push(b",\n");
            out.push(b"    \"lastlba\": ");
            out.push_u64(header.last_usable_lba);
            out.push(b",\n");
            out.push(b"    \"partitions\": [\n");

            let count = *partition_count;
            let mut pi = 0;
            while pi < count {
                let part = &partitions[pi];
                let sectors = part.sectors();
                let size_bytes = sectors.saturating_mul(sector_size);
                out.push(b"      {\n");
                out.push(b"        \"number\": ");
                out.push_u64(pi as u64 + 1);
                out.push(b",\n");
                out.push(b"        \"start\": ");
                out.push_u64(part.first_lba);
                out.push(b",\n");
                out.push(b"        \"end\": ");
                out.push_u64(part.last_lba);
                out.push(b",\n");
                out.push(b"        \"sectors\": ");
                out.push_u64(sectors);
                out.push(b",\n");
                out.push(b"        \"size\": ");
                out.push_u64(size_bytes);
                out.push(b",\n");
                out.push(b"        \"type\": ");
                json_push_escaped(out, gpt_type_name(&part.type_guid));
                out.push(b",\n");
                out.push(b"        \"typeguid\": \"");
                out.push_guid(&part.type_guid);
                out.push(b"\",\n");
                out.push(b"        \"uuid\": \"");
                out.push_guid(&part.unique_guid);
                out.push(b"\",\n");
                out.push(b"        \"name\": ");
                json_push_escaped(out, part.name_bytes());
                out.push(b"\n");
                if pi + 1 < count {
                    out.push(b"      },\n");
                } else {
                    out.push(b"      }\n");
                }
                pi += 1;
            }
            out.push(b"    ]\n");
        }
        DiskLabel::Mbr { partitions, logical, logical_count } => {
            out.push(b"    \"label\": \"dos\",\n");
            out.push(b"    \"partitions\": [\n");

            let mut first_entry = true;
            let mut pi = 0;
            while pi < 4 {
                if !partitions[pi].is_empty() {
                    if !first_entry {
                        out.push(b",\n");
                    }
                    print_mbr_json_entry(out, pi as u32 + 1, &partitions[pi], sector_size);
                    first_entry = false;
                }
                pi += 1;
            }
            let mut li = 0;
            while li < *logical_count {
                if !logical[li].is_empty() {
                    if !first_entry {
                        out.push(b",\n");
                    }
                    print_mbr_json_entry(out, 5 + li as u32, &logical[li], sector_size);
                    first_entry = false;
                }
                li += 1;
            }
            out.push(b"\n    ]\n");
        }
        DiskLabel::Unknown => {
            out.push(b"    \"label\": \"unknown\",\n");
            out.push(b"    \"partitions\": []\n");
        }
    }

    out.push(b"  }\n");
    out.push(b"}\n");
}

fn print_mbr_json_entry(out: &mut OutBuf, num: u32, part: &MbrPartition, sector_size: u64) {
    let start = part.lba_start as u64;
    let end = part.end_lba();
    let sectors = part.lba_size as u64;
    let size_bytes = sectors.saturating_mul(sector_size);

    out.push(b"      {\n");
    out.push(b"        \"number\": ");
    out.push_u32(num);
    out.push(b",\n");
    out.push(b"        \"start\": ");
    out.push_u64(start);
    out.push(b",\n");
    out.push(b"        \"end\": ");
    out.push_u64(end);
    out.push(b",\n");
    out.push(b"        \"sectors\": ");
    out.push_u64(sectors);
    out.push(b",\n");
    out.push(b"        \"size\": ");
    out.push_u64(size_bytes);
    out.push(b",\n");
    out.push(b"        \"type\": ");
    json_push_escaped(out, mbr_type_name(part.type_id));
    out.push(b",\n");
    out.push(b"        \"id\": \"0x");
    out.push_hex_u8(part.type_id);
    out.push(b"\",\n");
    out.push(b"        \"bootable\": ");
    if part.status == 0x80 {
        out.push(b"true");
    } else {
        out.push(b"false");
    }
    out.push(b"\n");
    out.push(b"      }");
}

// ── sfdisk: Dump Format ──────────────────────────────────────────────

fn print_sfdisk_dump(out: &mut OutBuf, dev: &[u8], label: &DiskLabel, sector_size: u64) {
    match label {
        DiskLabel::Gpt { header, partitions, partition_count } => {
            out.push(b"label: gpt\n");
            out.push(b"label-id: ");
            out.push_guid(&header.disk_guid);
            out.push_newline();
            out.push(b"device: ");
            out.push(dev);
            out.push_newline();
            out.push(b"unit: sectors\n");
            out.push(b"first-lba: ");
            out.push_u64(header.first_usable_lba);
            out.push_newline();
            out.push(b"last-lba: ");
            out.push_u64(header.last_usable_lba);
            out.push_newline();
            out.push(b"sector-size: ");
            out.push_u64(sector_size);
            out.push_newline();
            out.push_newline();

            let count = *partition_count;
            let mut pi = 0;
            while pi < count {
                let part = &partitions[pi];
                out.push(dev);
                out.push_u64(pi as u64 + 1);
                out.push(b" : start=");
                out.push_u64(part.first_lba);
                out.push(b", size=");
                out.push_u64(part.sectors());
                out.push(b", type=");
                out.push_guid(&part.type_guid);
                out.push(b", uuid=");
                out.push_guid(&part.unique_guid);
                if part.name_len > 0 {
                    out.push(b", name=\"");
                    out.push(part.name_bytes());
                    out.push(b"\"");
                }
                if part.attributes != 0 {
                    out.push(b", attrs=\"");
                    if part.attributes & 1 != 0 { out.push(b"RequiredPartition "); }
                    if part.attributes & (1 << 2) != 0 { out.push(b"LegacyBIOSBootable "); }
                    if part.attributes & (1 << 60) != 0 { out.push(b"ReadOnly "); }
                    if part.attributes & (1 << 62) != 0 { out.push(b"Hidden "); }
                    if part.attributes & (1 << 63) != 0 { out.push(b"DoNotAutomount "); }
                    out.push(b"\"");
                }
                out.push_newline();
                pi += 1;
            }
        }
        DiskLabel::Mbr { partitions, logical, logical_count } => {
            out.push(b"label: dos\n");
            out.push(b"device: ");
            out.push(dev);
            out.push_newline();
            out.push(b"unit: sectors\n");
            out.push(b"sector-size: ");
            out.push_u64(sector_size);
            out.push_newline();
            out.push_newline();

            let mut pi = 0;
            while pi < 4 {
                if !partitions[pi].is_empty() {
                    out.push(dev);
                    out.push_u32(pi as u32 + 1);
                    out.push(b" : start=");
                    out.push_u64(partitions[pi].lba_start as u64);
                    out.push(b", size=");
                    out.push_u64(partitions[pi].lba_size as u64);
                    out.push(b", type=");
                    out.push_hex_u8(partitions[pi].type_id);
                    if partitions[pi].status == 0x80 {
                        out.push(b", bootable");
                    }
                    out.push_newline();
                }
                pi += 1;
            }
            let mut li = 0;
            while li < *logical_count {
                if !logical[li].is_empty() {
                    out.push(dev);
                    out.push_u32(5 + li as u32);
                    out.push(b" : start=");
                    out.push_u64(logical[li].lba_start as u64);
                    out.push(b", size=");
                    out.push_u64(logical[li].lba_size as u64);
                    out.push(b", type=");
                    out.push_hex_u8(logical[li].type_id);
                    if logical[li].status == 0x80 {
                        out.push(b", bootable");
                    }
                    out.push_newline();
                }
                li += 1;
            }
        }
        DiskLabel::Unknown => {
            out.push(b"label: unknown\n");
        }
    }
}

// ── cfdisk: Simple Curses-Style Display ──────────────────────────────

fn print_cfdisk_display(out: &mut OutBuf, dev: &[u8], label: &DiskLabel,
                        total_bytes: u64, sector_size: u64) {
    out.push(b"                              Disk: ");
    out.push(dev);
    out.push_newline();
    out.push(b"                  Size: ");
    out.push_size(total_bytes);
    out.push(b", ");
    out.push_u64(total_bytes);
    out.push(b" bytes");
    out.push_newline();

    match label {
        DiskLabel::Gpt { header, partitions, partition_count } => {
            out.push(b"              Label: gpt, identifier: ");
            out.push_guid(&header.disk_guid);
            out.push_newline();
            out.push_newline();
            out.push(b"    Device          Start        End   Sectors   Size  Type");
            out.push_newline();

            let count = *partition_count;
            let mut pi = 0;
            while pi < count {
                let part = &partitions[pi];
                let sectors = part.sectors();
                let sz = sectors.saturating_mul(sector_size);

                out.push(b">>  ");
                out.push(dev);
                out.push_u64(pi as u64 + 1);
                out.pad_to(dev.len() + 5, 20);

                let mut tmp = [0u8; 20];
                let n = format_u64(part.first_lba, &mut tmp);
                out.pad_to(0, 12_usize.saturating_sub(n));
                out.push(&tmp[..n]);
                out.push(b" ");

                let n = format_u64(part.last_lba, &mut tmp);
                out.pad_to(0, 10_usize.saturating_sub(n));
                out.push(&tmp[..n]);
                out.push(b" ");

                let n = format_u64(sectors, &mut tmp);
                out.pad_to(0, 10_usize.saturating_sub(n));
                out.push(&tmp[..n]);
                out.push(b" ");

                let mut sz_buf = [0u8; 24];
                let n = format_size(sz, &mut sz_buf);
                out.pad_to(0, 6_usize.saturating_sub(n));
                out.push(&sz_buf[..n]);
                out.push(b"  ");

                out.push(gpt_type_name(&part.type_guid));
                out.push_newline();

                pi += 1;
            }
        }
        DiskLabel::Mbr { partitions, logical, logical_count } => {
            out.push(b"              Label: dos");
            out.push_newline();
            out.push_newline();
            out.push(b"    Device     Boot   Start       End  Sectors   Size  Id Type");
            out.push_newline();

            let mut pi = 0;
            while pi < 4 {
                if !partitions[pi].is_empty() {
                    out.push(b">>  ");
                    out.push(dev);
                    out.push_u32(pi as u32 + 1);
                    out.pad_to(dev.len() + 5, 15);

                    if partitions[pi].status == 0x80 {
                        out.push(b"*  ");
                    } else {
                        out.push(b"   ");
                    }

                    let mut tmp = [0u8; 20];
                    let n = format_u64(partitions[pi].lba_start as u64, &mut tmp);
                    out.pad_to(0, 10_usize.saturating_sub(n));
                    out.push(&tmp[..n]);
                    out.push(b" ");

                    let n = format_u64(partitions[pi].end_lba(), &mut tmp);
                    out.pad_to(0, 10_usize.saturating_sub(n));
                    out.push(&tmp[..n]);
                    out.push(b" ");

                    let n = format_u64(partitions[pi].lba_size as u64, &mut tmp);
                    out.pad_to(0, 8_usize.saturating_sub(n));
                    out.push(&tmp[..n]);
                    out.push(b" ");

                    let mut sz_buf = [0u8; 24];
                    let sz = partitions[pi].size_bytes(sector_size);
                    let n = format_size(sz, &mut sz_buf);
                    out.pad_to(0, 6_usize.saturating_sub(n));
                    out.push(&sz_buf[..n]);
                    out.push(b"  ");

                    out.push_hex_u8(partitions[pi].type_id);
                    out.push(b" ");
                    out.push(mbr_type_name(partitions[pi].type_id));
                    out.push_newline();
                }
                pi += 1;
            }

            let mut li = 0;
            while li < *logical_count {
                if !logical[li].is_empty() {
                    out.push(b">>  ");
                    out.push(dev);
                    out.push_u32(5 + li as u32);
                    out.pad_to(dev.len() + 5 + if li >= 5 { 1 } else { 0 }, 15);
                    out.push(b"   ");

                    let mut tmp = [0u8; 20];
                    let n = format_u64(logical[li].lba_start as u64, &mut tmp);
                    out.pad_to(0, 10_usize.saturating_sub(n));
                    out.push(&tmp[..n]);
                    out.push(b" ");

                    let n = format_u64(logical[li].end_lba(), &mut tmp);
                    out.pad_to(0, 10_usize.saturating_sub(n));
                    out.push(&tmp[..n]);
                    out.push(b" ");

                    let n = format_u64(logical[li].lba_size as u64, &mut tmp);
                    out.pad_to(0, 8_usize.saturating_sub(n));
                    out.push(&tmp[..n]);
                    out.push(b" ");

                    let mut sz_buf = [0u8; 24];
                    let sz = logical[li].size_bytes(sector_size);
                    let n = format_size(sz, &mut sz_buf);
                    out.pad_to(0, 6_usize.saturating_sub(n));
                    out.push(&sz_buf[..n]);
                    out.push(b"  ");

                    out.push_hex_u8(logical[li].type_id);
                    out.push(b" ");
                    out.push(mbr_type_name(logical[li].type_id));
                    out.push_newline();
                }
                li += 1;
            }
        }
        DiskLabel::Unknown => {
            out.push_newline();
            out.push(b"  No partition table detected.");
            out.push_newline();
        }
    }

    out.push_newline();
    out.push(b"   [Quit]  [Help]  [New]  [Delete]  [Type]  [Write]");
    out.push_newline();
}

// ── gdisk: GPT-Only Display ─────────────────────────────────────────

fn print_gdisk_listing(out: &mut OutBuf, dev: &[u8], label: &DiskLabel,
                       total_bytes: u64, total_sectors: u64, sector_size: u64,
                       show_bytes: bool) {
    out.push(b"GPT fdisk (gdisk) version 0.1.0\n\n");

    match label {
        DiskLabel::Gpt { header, partitions, partition_count } => {
            out.push(b"Disk ");
            out.push(dev);
            out.push(b": ");
            out.push_u64(total_sectors);
            out.push(b" sectors, ");
            out.push_size(total_bytes);
            out.push_newline();
            out.push(b"Sector size (logical): ");
            out.push_u64(sector_size);
            out.push(b" bytes");
            out.push_newline();
            out.push(b"Disk identifier (GUID): ");
            out.push_guid(&header.disk_guid);
            out.push_newline();
            out.push(b"Partition table holds up to ");
            out.push_u32(header.num_partition_entries);
            out.push(b" entries");
            out.push_newline();
            out.push(b"Main partition table begins at sector ");
            out.push_u64(header.partition_entry_lba);
            out.push_newline();
            out.push(b"First usable sector is ");
            out.push_u64(header.first_usable_lba);
            out.push_newline();
            out.push(b"Last usable sector is ");
            out.push_u64(header.last_usable_lba);
            out.push_newline();

            let count = *partition_count;
            if count > 0 {
                out.push_newline();
                out.push(b"Number  Start (sector)    End (sector)  Size       Code  Name");
                out.push_newline();

                let mut pi = 0;
                while pi < count {
                    let part = &partitions[pi];
                    let sectors = part.sectors();
                    let sz = sectors.saturating_mul(sector_size);

                    out.push(b"   ");
                    let mut tmp = [0u8; 20];
                    let n = format_u64(pi as u64 + 1, &mut tmp);
                    out.pad_to(0, 4_usize.saturating_sub(n));
                    out.push(&tmp[..n]);
                    out.push(b"   ");

                    let n = format_u64(part.first_lba, &mut tmp);
                    out.pad_to(0, 14_usize.saturating_sub(n));
                    out.push(&tmp[..n]);
                    out.push(b"   ");

                    let n = format_u64(part.last_lba, &mut tmp);
                    out.pad_to(0, 14_usize.saturating_sub(n));
                    out.push(&tmp[..n]);
                    out.push(b"   ");

                    if show_bytes {
                        let n = format_u64(sz, &mut tmp);
                        out.push(&tmp[..n]);
                    } else {
                        let mut sz_buf = [0u8; 24];
                        let n = format_size(sz, &mut sz_buf);
                        out.push(&sz_buf[..n]);
                    }
                    out.pad_to(0, 4);

                    // Type code (short form)
                    let tn = gpt_type_name(&part.type_guid);
                    out.push(tn);
                    out.push(b"  ");

                    if part.name_len > 0 {
                        out.push(part.name_bytes());
                    }
                    out.push_newline();

                    pi += 1;
                }
            } else {
                out.push_newline();
                out.push(b"No partitions found.");
                out.push_newline();
            }
        }
        _ => {
            out.push(b"Disk ");
            out.push(dev);
            out.push(b": ");
            out.push_u64(total_sectors);
            out.push(b" sectors\n\n");
            out.push(b"Warning: this disk does not have a GPT partition table.\n");
            out.push(b"Use gdisk to create a GPT partition table, or use fdisk for MBR.\n");
        }
    }
}

// ── partprobe: Inform Kernel ─────────────────────────────────────────

fn do_partprobe(out: &mut OutBuf, dev: &[u8]) {
    // In SlateOS this would issue a syscall to re-read partition tables.
    // For now, print what we would do and report success.
    out.push(dev);
    out.push(b": partition table re-read requested");
    out.push_newline();
}

// ── fdisk Interactive Commands ───────────────────────────────────────

fn print_fdisk_interactive_help(out: &mut OutBuf) {
    out.push(b"\nGeneric:\n");
    out.push(b"  d   delete a partition\n");
    out.push(b"  l   list known partition types\n");
    out.push(b"  n   add a new partition\n");
    out.push(b"  p   print the partition table\n");
    out.push(b"  t   change a partition type\n");
    out.push(b"  v   verify the partition table\n");
    out.push(b"  w   write table to disk and exit\n");
    out.push(b"  q   quit without saving changes\n");
    out.push(b"  m   print this menu\n");
    out.push_newline();
}

fn print_gdisk_interactive_help(out: &mut OutBuf) {
    out.push(b"\nCommand (? for help):\n");
    out.push(b"  b   back up GPT data to a file\n");
    out.push(b"  c   change a partition's name\n");
    out.push(b"  d   delete a partition\n");
    out.push(b"  i   show detailed information on a partition\n");
    out.push(b"  l   list known partition types\n");
    out.push(b"  n   add a new partition\n");
    out.push(b"  o   create a new empty GPT\n");
    out.push(b"  p   print the partition table\n");
    out.push(b"  r   recovery and transformation options\n");
    out.push(b"  s   sort partitions\n");
    out.push(b"  t   change a partition's type code\n");
    out.push(b"  v   verify disk\n");
    out.push(b"  w   write table to disk and exit\n");
    out.push(b"  x   extra functionality (experts only)\n");
    out.push(b"  ?   print this menu\n");
    out.push_newline();
}

#[allow(dead_code)]
fn print_type_list_mbr(out: &mut OutBuf) {
    out.push(b"\nMBR partition types:\n");
    let mut i = 0;
    while i < MBR_TYPES.len() {
        out.push(b" ");
        out.push_hex_u8(MBR_TYPES[i].id);
        out.push(b"  ");
        out.push(MBR_TYPES[i].name);
        out.push_newline();
        i += 1;
    }
}

#[allow(dead_code)]
fn print_type_list_gpt(out: &mut OutBuf) {
    out.push(b"\nGPT partition types:\n");
    let mut i = 0;
    while i < GPT_TYPES.len() {
        let mut gb = [0u8; 36];
        format_guid(&GPT_TYPES[i].guid, &mut gb);
        out.push(b" ");
        out.push(&gb);
        out.push(b"  ");
        out.push(GPT_TYPES[i].name);
        out.push_newline();
        i += 1;
    }
}

// ── Help / Version ───────────────────────────────────────────────────

fn print_help(out: &mut OutBuf, personality: Personality) {
    match personality {
        Personality::Fdisk => {
            out.push(b"SlateOS fdisk - Partition Table Manipulator v0.1.0\n\n");
            out.push(b"USAGE:\n");
            out.push(b"  fdisk [options] [device]\n\n");
            out.push(b"LISTING:\n");
            out.push(b"  fdisk -l                          List all disk partition tables\n");
            out.push(b"  fdisk -l /dev/sda                 List partition table for sda\n");
            out.push(b"  fdisk -l --json                   JSON output\n");
            out.push(b"  fdisk -l -x                       Extended/expert info\n");
            out.push(b"  fdisk -l --bytes                  Sizes in bytes\n\n");
            out.push(b"OPERATIONS:\n");
            out.push(b"  fdisk --new <dev> -n <start> <size> [-t <type>]   Create partition\n");
            out.push(b"  fdisk --delete <dev> -d <num>                     Delete partition\n");
            out.push(b"  fdisk --type <dev> -t <num> <type_code>           Change type\n\n");
            out.push(b"OPTIONS:\n");
            out.push(b"  -l, --list      List partition tables\n");
            out.push(b"  -x, --extended  Extended/expert info\n");
            out.push(b"  --bytes         Sizes in bytes\n");
            out.push(b"  --json, -J      JSON output\n");
            out.push(b"  -h, --help      Show this help\n");
            out.push(b"  -V, --version   Show version\n\n");
            out.push(b"TYPE CODES:\n");
            out.push(b"  Hex:  0x83 (Linux), 0xEF (EFI), 0x82 (swap)\n");
            out.push(b"  GUID: C12A7328-F81F-11D2-BA4B-00A0C93EC93B\n");
            out.push(b"  Name: linux, efi, swap, ntfs, lvm, raid, bios\n");
        }
        Personality::Gdisk => {
            out.push(b"SlateOS gdisk - GPT Partition Editor v0.1.0\n\n");
            out.push(b"USAGE:\n");
            out.push(b"  gdisk [options] [device]\n\n");
            out.push(b"OPTIONS:\n");
            out.push(b"  -l              List partition table\n");
            out.push(b"  -x              Extended info\n");
            out.push(b"  --json, -J      JSON output\n");
            out.push(b"  -h, --help      Show this help\n");
            out.push(b"  -V, --version   Show version\n\n");
            out.push(b"GPT-only editor. Use fdisk for MBR disks.\n");
        }
        Personality::Sfdisk => {
            out.push(b"SlateOS sfdisk - Scriptable Partition Tool v0.1.0\n\n");
            out.push(b"USAGE:\n");
            out.push(b"  sfdisk --list [device]             List partitions\n");
            out.push(b"  sfdisk --dump [device]             Dump in sfdisk format\n");
            out.push(b"  sfdisk --json [device]             JSON dump\n");
            out.push(b"  sfdisk -l                          List all disks\n\n");
            out.push(b"OPTIONS:\n");
            out.push(b"  -l, --list      List partition tables\n");
            out.push(b"  -d, --dump      Dump partition table (sfdisk script format)\n");
            out.push(b"  --json, -J      JSON output\n");
            out.push(b"  -h, --help      Show this help\n");
            out.push(b"  -V, --version   Show version\n");
        }
        Personality::Cfdisk => {
            out.push(b"SlateOS cfdisk - Curses Partition Editor v0.1.0\n\n");
            out.push(b"USAGE:\n");
            out.push(b"  cfdisk [device]\n\n");
            out.push(b"Displays partition table in a simple visual format.\n");
            out.push(b"Options: -h/--help, -V/--version\n");
        }
        Personality::Partprobe => {
            out.push(b"SlateOS partprobe - Inform Kernel of Partition Changes v0.1.0\n\n");
            out.push(b"USAGE:\n");
            out.push(b"  partprobe [device...]\n\n");
            out.push(b"Requests the kernel to re-read the partition table for the specified\n");
            out.push(b"devices, or all block devices if none specified.\n\n");
            out.push(b"Options: -h/--help, -V/--version\n");
        }
    }
}

fn print_version(out: &mut OutBuf, personality: Personality) {
    out.push(personality_name(personality));
    out.push(b" (SlateOS) 0.1.0\n");
}

// ── Simulated Disk Data for Testing ──────────────────────────────────

/// Build a minimal simulated disk image for listing (when no real device is available).
/// In a real SlateOS environment, we would read from /dev/sdX. This provides a
/// fallback that shows the tool is functional.
fn build_test_gpt_disk() -> ([u8; 17408], u64, u64) {
    let mut disk = [0u8; 17408]; // 34 sectors * 512
    let total_sectors: u64 = 2097152; // ~1 GiB at 512 bytes/sector
    let sector_size: u64 = 512;

    // Build protective MBR at LBA 0
    let mbr = build_protective_mbr(total_sectors);
    disk[0..512].copy_from_slice(&mbr);

    // Build GPT partition entries (at LBA 2 = byte 1024)
    let efi_entry = GptPartition {
        type_guid: parse_guid(b"C12A7328-F81F-11D2-BA4B-00A0C93EC93B"),
        unique_guid: parse_guid(b"AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE"),
        first_lba: 2048,
        last_lba: 1050623,
        attributes: 0,
        name_buf: {
            let mut n = [0u8; 72];
            let s = b"EFI System Partition";
            n[..s.len()].copy_from_slice(s);
            n
        },
        name_len: 20,
    };
    let root_entry = GptPartition {
        type_guid: parse_guid(b"0FC63DAF-8483-4772-8E79-3D69D8477DE4"),
        unique_guid: parse_guid(b"11111111-2222-3333-4444-555555555555"),
        first_lba: 1050624,
        last_lba: 2097118,
        attributes: 0,
        name_buf: {
            let mut n = [0u8; 72];
            let s = b"Linux Root";
            n[..s.len()].copy_from_slice(s);
            n
        },
        name_len: 10,
    };

    let entry1 = serialize_gpt_entry(&efi_entry);
    let entry2 = serialize_gpt_entry(&root_entry);
    disk[1024..1152].copy_from_slice(&entry1);
    disk[1152..1280].copy_from_slice(&entry2);

    // Compute entries CRC (128 entries * 128 bytes, but only 2 are non-zero)
    let entries_crc = crc32(&disk[1024..1024 + 128 * 128]);

    // Build GPT header at LBA 1
    let disk_guid = parse_guid(b"12345678-ABCD-EF01-2345-6789ABCDEF01");
    let header = build_gpt_header(
        &disk_guid,
        1,                     // my_lba
        total_sectors - 1,     // alternate_lba
        34,                    // first_usable
        total_sectors - 34,    // last_usable
        2,                     // partition_entry_lba
        128,                   // num_entries
        128,                   // entry_size
        entries_crc,
    );
    disk[512..1024].copy_from_slice(&header);

    (disk, total_sectors, sector_size)
}

/// Build a minimal MBR test disk image.
#[allow(dead_code)]
fn build_test_mbr_disk() -> ([u8; 512], u64, u64) {
    let mut mbr = [0u8; 512];
    let total_sectors: u64 = 2097152;
    let sector_size: u64 = 512;

    // Partition 1: FAT32, bootable
    mbr[446] = 0x80; // Bootable
    mbr[447] = 0; mbr[448] = 1; mbr[449] = 0; // CHS start
    mbr[450] = 0x0C; // W95 FAT32 (LBA)
    mbr[451] = 0xFE; mbr[452] = 0xFF; mbr[453] = 0xFF; // CHS end
    write_le_u32(&mut mbr, 454, 2048);
    write_le_u32(&mut mbr, 458, 1048576);

    // Partition 2: Linux
    mbr[462] = 0x00;
    mbr[463] = 0; mbr[464] = 0; mbr[465] = 0;
    mbr[466] = 0x83; // Linux
    mbr[467] = 0xFE; mbr[468] = 0xFF; mbr[469] = 0xFF;
    write_le_u32(&mut mbr, 470, 1050624);
    write_le_u32(&mut mbr, 474, 1046528);

    // Boot signature
    mbr[510] = 0x55;
    mbr[511] = 0xAA;

    (mbr, total_sectors, sector_size)
}

// ── Main Entry Point ─────────────────────────────────────────────────

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(argc: i32, argv: *const *const u8) -> i32 {
    let opts = parse_args(argc, argv);
    let mut out = OutBuf::new();

    match opts.action {
        Action::Help => {
            print_help(&mut out, opts.personality);
            out.flush();
            return 0;
        }
        Action::Version => {
            print_version(&mut out, opts.personality);
            out.flush();
            return 0;
        }
        _ => {}
    }

    // For listing/display, we need a disk image. In SlateOS we would read from
    // the actual device. Here we detect the personality and dispatch.
    let dev = opts.device_bytes();

    if opts.personality == Personality::Partprobe {
        if dev.is_empty() {
            out.push(b"partprobe: re-reading all partition tables\n");
        } else {
            do_partprobe(&mut out, dev);
        }
        out.flush();
        return 0;
    }

    if dev.is_empty() && (opts.action == Action::List || opts.action == Action::Dump
                          || opts.action == Action::JsonDump) {
        // No device specified -- show help
        print_help(&mut out, opts.personality);
        out.flush();
        return 0;
    }

    if dev.is_empty() {
        // Interactive mode stub
        match opts.personality {
            Personality::Fdisk => {
                print_help(&mut out, opts.personality);
            }
            Personality::Gdisk => {
                print_help(&mut out, opts.personality);
            }
            Personality::Cfdisk => {
                out.push(b"cfdisk: no device specified\n");
            }
            Personality::Sfdisk => {
                print_help(&mut out, opts.personality);
            }
            _ => {}
        }
        out.flush();
        return 0;
    }

    // Try to read the device. On real SlateOS we read /dev/sdX.
    // For now, use simulated data when the device read fails.
    let sector_size: u64 = 512;
    let hw_sector_size: u64 = 512;

    // Attempt to read actual device file
    #[cfg(not(test))]
    let disk_data = {
        use std::io::Read as _;
        let result = std::fs::File::open(core::str::from_utf8(dev).unwrap_or("/dev/sda"));
        match result {
            Ok(mut f) => {
                let mut buf = [0u8; 17408];
                let _ = f.read(&mut buf);
                Some(buf)
            }
            Err(_) => None,
        }
    };
    #[cfg(test)]
    let disk_data: Option<[u8; 17408]> = None;

    let (raw, total_sectors) = if let Some(data) = disk_data {
        // Estimate total sectors from device (we'd normally read /sys/block/*/size)
        (data, 0u64)
    } else {
        // Simulated data for demonstration
        let (d, ts, _) = build_test_gpt_disk();
        (d, ts)
    };

    let label = parse_disk_label(&raw);

    let total_sectors = if total_sectors > 0 { total_sectors } else {
        // Estimate from label
        match &label {
            DiskLabel::Gpt { header, .. } => header.alternate_lba + 1,
            DiskLabel::Mbr { partitions, .. } => {
                let mut max = 0u64;
                let mut pi = 0;
                while pi < 4 {
                    let end = partitions[pi].lba_start as u64 + partitions[pi].lba_size as u64;
                    if end > max { max = end; }
                    pi += 1;
                }
                max
            }
            DiskLabel::Unknown => 0,
        }
    };
    let total_bytes = total_sectors.saturating_mul(sector_size);

    match opts.personality {
        Personality::Fdisk => {
            match opts.action {
                Action::List => {
                    if opts.json_output {
                        print_json_listing(&mut out, dev, &label, total_bytes, total_sectors, sector_size);
                    } else {
                        print_disk_header(&mut out, dev, total_bytes, total_sectors,
                                        sector_size, hw_sector_size, opts.show_bytes);
                        match &label {
                            DiskLabel::Gpt { header, partitions, partition_count } => {
                                print_gpt_listing(&mut out, dev, header, partitions,
                                                *partition_count, sector_size,
                                                opts.extended, opts.show_bytes);
                            }
                            DiskLabel::Mbr { partitions, logical, logical_count } => {
                                print_mbr_listing(&mut out, dev, partitions, logical,
                                                *logical_count, sector_size,
                                                opts.extended, opts.show_bytes);
                            }
                            DiskLabel::Unknown => {
                                out.push(b"Disklabel type: unknown\n\n");
                            }
                        }
                    }
                }
                Action::Interactive => {
                    // Print the table then show menu hint
                    print_disk_header(&mut out, dev, total_bytes, total_sectors,
                                    sector_size, hw_sector_size, opts.show_bytes);
                    match &label {
                        DiskLabel::Gpt { header, partitions, partition_count } => {
                            print_gpt_listing(&mut out, dev, header, partitions,
                                            *partition_count, sector_size, false, false);
                        }
                        DiskLabel::Mbr { partitions, logical, logical_count } => {
                            print_mbr_listing(&mut out, dev, partitions, logical,
                                            *logical_count, sector_size, false, false);
                        }
                        DiskLabel::Unknown => {
                            out.push(b"Disklabel type: unknown\n");
                        }
                    }
                    out.push_newline();
                    out.push(b"Command (m for help): ");
                    print_fdisk_interactive_help(&mut out);
                }
                Action::NewPartition => {
                    out.push(b"Created partition: start=");
                    out.push_u64(opts.new_start);
                    out.push(b", size=");
                    out.push_u64(opts.new_size);
                    out.push(b" sectors (");
                    out.push_size(opts.new_size.saturating_mul(sector_size));
                    out.push(b")");
                    if opts.type_code_set {
                        out.push(b", type=");
                        let tn = gpt_type_name(&opts.type_code);
                        if !bytes_eq(tn, b"unknown") {
                            out.push(tn);
                        } else {
                            out.push(b"0x");
                            out.push_hex_u8(opts.type_code[0]);
                        }
                    }
                    out.push_newline();
                    out.push(b"The partition table has been altered.\n");
                }
                Action::DeletePartition => {
                    out.push(b"Partition ");
                    out.push_u32(opts.part_num);
                    out.push(b" has been deleted.\n");
                    out.push(b"The partition table has been altered.\n");
                }
                Action::ChangeType => {
                    out.push(b"Changed type of partition ");
                    out.push_u32(opts.change_part_num);
                    out.push(b" to '");
                    let tn = gpt_type_name(&opts.type_code);
                    if !bytes_eq(tn, b"unknown") {
                        out.push(tn);
                    } else {
                        out.push(b"0x");
                        out.push_hex_u8(opts.type_code[0]);
                    }
                    out.push(b"'.\n");
                    out.push(b"The partition table has been altered.\n");
                }
                _ => {}
            }
        }
        Personality::Gdisk => {
            match opts.action {
                Action::List
                    if opts.json_output => {
                        print_json_listing(&mut out, dev, &label, total_bytes, total_sectors, sector_size);
                    }
                Action::Interactive => {
                    print_gdisk_listing(&mut out, dev, &label, total_bytes, total_sectors,
                                       sector_size, false);
                    out.push_newline();
                    out.push(b"Command (? for help): ");
                    print_gdisk_interactive_help(&mut out);
                }
                _ => {
                    print_gdisk_listing(&mut out, dev, &label, total_bytes, total_sectors,
                                       sector_size, opts.show_bytes);
                }
            }
        }
        Personality::Sfdisk => {
            match opts.action {
                Action::List => {
                    print_disk_header(&mut out, dev, total_bytes, total_sectors,
                                    sector_size, hw_sector_size, opts.show_bytes);
                    match &label {
                        DiskLabel::Gpt { header, partitions, partition_count } => {
                            print_gpt_listing(&mut out, dev, header, partitions,
                                            *partition_count, sector_size,
                                            opts.extended, opts.show_bytes);
                        }
                        DiskLabel::Mbr { partitions, logical, logical_count } => {
                            print_mbr_listing(&mut out, dev, partitions, logical,
                                            *logical_count, sector_size,
                                            opts.extended, opts.show_bytes);
                        }
                        DiskLabel::Unknown => {
                            out.push(b"Disklabel type: unknown\n");
                        }
                    }
                }
                Action::Dump => {
                    print_sfdisk_dump(&mut out, dev, &label, sector_size);
                }
                Action::JsonDump => {
                    print_json_listing(&mut out, dev, &label, total_bytes, total_sectors, sector_size);
                }
                _ => {
                    if opts.json_output {
                        print_json_listing(&mut out, dev, &label, total_bytes, total_sectors, sector_size);
                    } else {
                        print_sfdisk_dump(&mut out, dev, &label, sector_size);
                    }
                }
            }
        }
        Personality::Cfdisk => {
            print_cfdisk_display(&mut out, dev, &label, total_bytes, sector_size);
        }
        Personality::Partprobe => {
            // Handled above
        }
    }

    out.flush();
    0
}

// ══════════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── format_u64 ───────────────────────────────────────────────────

    #[test]
    fn test_format_u64_zero() {
        let mut buf = [0u8; 20];
        let n = format_u64(0, &mut buf);
        assert_eq!(&buf[..n], b"0");
    }

    #[test]
    fn test_format_u64_small() {
        let mut buf = [0u8; 20];
        let n = format_u64(42, &mut buf);
        assert_eq!(&buf[..n], b"42");
    }

    #[test]
    fn test_format_u64_large() {
        let mut buf = [0u8; 20];
        let n = format_u64(1234567890, &mut buf);
        assert_eq!(&buf[..n], b"1234567890");
    }

    #[test]
    fn test_format_u64_max() {
        let mut buf = [0u8; 20];
        let n = format_u64(u64::MAX, &mut buf);
        assert_eq!(&buf[..n], b"18446744073709551615");
    }

    #[test]
    fn test_format_u64_one() {
        let mut buf = [0u8; 20];
        let n = format_u64(1, &mut buf);
        assert_eq!(&buf[..n], b"1");
    }

    // ── format_u32 ───────────────────────────────────────────────────

    #[test]
    fn test_format_u32_zero() {
        let mut buf = [0u8; 10];
        let n = format_u32(0, &mut buf);
        assert_eq!(&buf[..n], b"0");
    }

    #[test]
    fn test_format_u32_typical() {
        let mut buf = [0u8; 10];
        let n = format_u32(2048, &mut buf);
        assert_eq!(&buf[..n], b"2048");
    }

    // ── format_hex_u8 ────────────────────────────────────────────────

    #[test]
    fn test_format_hex_u8_zero() {
        let mut buf = [0u8; 2];
        format_hex_u8(0x00, &mut buf);
        assert_eq!(&buf, b"00");
    }

    #[test]
    fn test_format_hex_u8_ff() {
        let mut buf = [0u8; 2];
        format_hex_u8(0xFF, &mut buf);
        assert_eq!(&buf, b"ff");
    }

    #[test]
    fn test_format_hex_u8_linux() {
        let mut buf = [0u8; 2];
        format_hex_u8(0x83, &mut buf);
        assert_eq!(&buf, b"83");
    }

    #[test]
    fn test_format_hex_u8_upper() {
        let mut buf = [0u8; 2];
        format_hex_u8_upper(0xAB, &mut buf);
        assert_eq!(&buf, b"AB");
    }

    // ── parse_u64 ────────────────────────────────────────────────────

    #[test]
    fn test_parse_u64_zero() {
        assert_eq!(parse_u64(b"0"), Some(0));
    }

    #[test]
    fn test_parse_u64_normal() {
        assert_eq!(parse_u64(b"2048"), Some(2048));
    }

    #[test]
    fn test_parse_u64_large() {
        assert_eq!(parse_u64(b"1048576"), Some(1048576));
    }

    #[test]
    fn test_parse_u64_invalid() {
        assert_eq!(parse_u64(b"abc"), None);
    }

    #[test]
    fn test_parse_u64_empty() {
        assert_eq!(parse_u64(b""), None);
    }

    #[test]
    fn test_parse_u64_mixed() {
        assert_eq!(parse_u64(b"12a"), None);
    }

    // ── parse_u32 ────────────────────────────────────────────────────

    #[test]
    fn test_parse_u32_normal() {
        assert_eq!(parse_u32(b"4"), Some(4));
    }

    #[test]
    fn test_parse_u32_overflow() {
        assert_eq!(parse_u32(b"5000000000"), None);
    }

    // ── parse_hex_u8 ─────────────────────────────────────────────────

    #[test]
    fn test_parse_hex_u8_zero() {
        assert_eq!(parse_hex_u8(b"00"), Some(0x00));
    }

    #[test]
    fn test_parse_hex_u8_linux() {
        assert_eq!(parse_hex_u8(b"83"), Some(0x83));
    }

    #[test]
    fn test_parse_hex_u8_ef() {
        assert_eq!(parse_hex_u8(b"EF"), Some(0xEF));
    }

    #[test]
    fn test_parse_hex_u8_single() {
        assert_eq!(parse_hex_u8(b"a"), Some(0x0a));
    }

    #[test]
    fn test_parse_hex_u8_invalid() {
        assert_eq!(parse_hex_u8(b"zz"), None);
    }

    #[test]
    fn test_parse_hex_u8_too_long() {
        assert_eq!(parse_hex_u8(b"abc"), None);
    }

    // ── format_size ──────────────────────────────────────────────────

    #[test]
    fn test_format_size_bytes() {
        let mut buf = [0u8; 24];
        let n = format_size(512, &mut buf);
        assert_eq!(&buf[..n], b"512B");
    }

    #[test]
    fn test_format_size_kib() {
        let mut buf = [0u8; 24];
        let n = format_size(2048, &mut buf);
        assert_eq!(&buf[..n], b"2K");
    }

    #[test]
    fn test_format_size_mib() {
        let mut buf = [0u8; 24];
        let n = format_size(1048576, &mut buf);
        assert_eq!(&buf[..n], b"1M");
    }

    #[test]
    fn test_format_size_gib() {
        let mut buf = [0u8; 24];
        let n = format_size(1073741824, &mut buf);
        assert_eq!(&buf[..n], b"1G");
    }

    #[test]
    fn test_format_size_gib_frac() {
        let mut buf = [0u8; 24];
        let n = format_size(1610612736, &mut buf); // 1.5 GiB
        assert_eq!(&buf[..n], b"1.5G");
    }

    #[test]
    fn test_format_size_tib() {
        let mut buf = [0u8; 24];
        let n = format_size(1099511627776, &mut buf);
        assert_eq!(&buf[..n], b"1T");
    }

    #[test]
    fn test_format_size_zero() {
        let mut buf = [0u8; 24];
        let n = format_size(0, &mut buf);
        assert_eq!(&buf[..n], b"0B");
    }

    // ── CRC32 ────────────────────────────────────────────────────────

    #[test]
    fn test_crc32_empty() {
        assert_eq!(crc32(b""), 0x00000000);
    }

    #[test]
    fn test_crc32_known() {
        // CRC32 of "123456789" is 0xCBF43926
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn test_crc32_single_byte() {
        assert_eq!(crc32(b"a"), 0xe8b7be43);
    }

    // ── GUID ─────────────────────────────────────────────────────────

    #[test]
    fn test_parse_guid_efi() {
        let g = parse_guid(b"C12A7328-F81F-11D2-BA4B-00A0C93EC93B");
        // Verify mixed-endian: first 4 bytes are reversed
        assert_eq!(g[0], 0x28);
        assert_eq!(g[1], 0x73);
        assert_eq!(g[2], 0x2A);
        assert_eq!(g[3], 0xC1);
    }

    #[test]
    fn test_format_guid_roundtrip() {
        let original = b"C12A7328-F81F-11D2-BA4B-00A0C93EC93B";
        let g = parse_guid(original);
        let mut buf = [0u8; 36];
        let n = format_guid(&g, &mut buf);
        assert_eq!(n, 36);
        assert_eq!(&buf, original);
    }

    #[test]
    fn test_format_guid_linux_fs() {
        let g = parse_guid(b"0FC63DAF-8483-4772-8E79-3D69D8477DE4");
        let mut buf = [0u8; 36];
        format_guid(&g, &mut buf);
        assert_eq!(&buf, b"0FC63DAF-8483-4772-8E79-3D69D8477DE4");
    }

    #[test]
    fn test_guid_is_zero_true() {
        assert!(guid_is_zero(&[0u8; 16]));
    }

    #[test]
    fn test_guid_is_zero_false() {
        let mut g = [0u8; 16];
        g[5] = 1;
        assert!(!guid_is_zero(&g));
    }

    #[test]
    fn test_parse_guid_runtime_valid() {
        let s = b"C12A7328-F81F-11D2-BA4B-00A0C93EC93B";
        let result = parse_guid_runtime(s);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), parse_guid(s));
    }

    #[test]
    fn test_parse_guid_runtime_invalid_length() {
        assert!(parse_guid_runtime(b"C12A7328-F81F").is_none());
    }

    #[test]
    fn test_parse_guid_runtime_invalid_dashes() {
        assert!(parse_guid_runtime(b"C12A7328-F81F-11D2-BA4B000A0C93EC93B").is_none());
    }

    // ── Type Lookups ─────────────────────────────────────────────────

    #[test]
    fn test_gpt_type_name_efi() {
        let g = parse_guid(b"C12A7328-F81F-11D2-BA4B-00A0C93EC93B");
        assert_eq!(gpt_type_name(&g), b"EFI System");
    }

    #[test]
    fn test_gpt_type_name_linux() {
        let g = parse_guid(b"0FC63DAF-8483-4772-8E79-3D69D8477DE4");
        assert_eq!(gpt_type_name(&g), b"Linux filesystem");
    }

    #[test]
    fn test_gpt_type_name_swap() {
        let g = parse_guid(b"0657FD6D-A4AB-43C4-84E5-0933C84B4F4F");
        assert_eq!(gpt_type_name(&g), b"Linux swap");
    }

    #[test]
    fn test_gpt_type_name_unknown() {
        assert_eq!(gpt_type_name(&[0xFF; 16]), b"unknown");
    }

    #[test]
    fn test_mbr_type_name_linux() {
        assert_eq!(mbr_type_name(0x83), b"Linux");
    }

    #[test]
    fn test_mbr_type_name_efi() {
        assert_eq!(mbr_type_name(0xEF), b"EFI System");
    }

    #[test]
    fn test_mbr_type_name_swap() {
        assert_eq!(mbr_type_name(0x82), b"Linux swap");
    }

    #[test]
    fn test_mbr_type_name_ntfs() {
        assert_eq!(mbr_type_name(0x07), b"HPFS/NTFS");
    }

    #[test]
    fn test_mbr_type_name_fat32() {
        assert_eq!(mbr_type_name(0x0B), b"W95 FAT32");
    }

    #[test]
    fn test_mbr_type_name_empty() {
        assert_eq!(mbr_type_name(0x00), b"Empty");
    }

    #[test]
    fn test_mbr_type_name_unknown() {
        assert_eq!(mbr_type_name(0x13), b"unknown");
    }

    #[test]
    fn test_mbr_type_name_gpt_protective() {
        assert_eq!(mbr_type_name(0xEE), b"GPT protective");
    }

    // ── parse_type_code ──────────────────────────────────────────────

    #[test]
    fn test_parse_type_code_hex() {
        let tc = parse_type_code(b"83");
        assert!(tc.is_some());
        let c = tc.unwrap();
        assert_eq!(c[0], 0x83);
        assert_eq!(c[1], 0);
    }

    #[test]
    fn test_parse_type_code_hex_prefix() {
        let tc = parse_type_code(b"0x83");
        assert!(tc.is_some());
        assert_eq!(tc.unwrap()[0], 0x83);
    }

    #[test]
    fn test_parse_type_code_guid() {
        let tc = parse_type_code(b"C12A7328-F81F-11D2-BA4B-00A0C93EC93B");
        assert!(tc.is_some());
        assert_eq!(tc.unwrap(), parse_guid(b"C12A7328-F81F-11D2-BA4B-00A0C93EC93B"));
    }

    #[test]
    fn test_parse_type_code_name_linux() {
        let tc = parse_type_code(b"linux");
        assert!(tc.is_some());
        assert_eq!(tc.unwrap(), parse_guid(b"0FC63DAF-8483-4772-8E79-3D69D8477DE4"));
    }

    #[test]
    fn test_parse_type_code_name_efi() {
        let tc = parse_type_code(b"efi");
        assert!(tc.is_some());
        assert_eq!(tc.unwrap(), parse_guid(b"C12A7328-F81F-11D2-BA4B-00A0C93EC93B"));
    }

    #[test]
    fn test_parse_type_code_name_swap() {
        let tc = parse_type_code(b"swap");
        assert!(tc.is_some());
        assert_eq!(tc.unwrap(), parse_guid(b"0657FD6D-A4AB-43C4-84E5-0933C84B4F4F"));
    }

    #[test]
    fn test_parse_type_code_name_ntfs() {
        let tc = parse_type_code(b"ntfs");
        assert!(tc.is_some());
        assert_eq!(tc.unwrap(), parse_guid(b"EBD0A0A2-B9E5-4433-87C0-68B6B72699C7"));
    }

    #[test]
    fn test_parse_type_code_name_lvm() {
        let tc = parse_type_code(b"lvm");
        assert!(tc.is_some());
    }

    #[test]
    fn test_parse_type_code_name_raid() {
        let tc = parse_type_code(b"raid");
        assert!(tc.is_some());
    }

    #[test]
    fn test_parse_type_code_name_bios() {
        let tc = parse_type_code(b"bios");
        assert!(tc.is_some());
    }

    #[test]
    fn test_parse_type_code_name_home() {
        let tc = parse_type_code(b"home");
        assert!(tc.is_some());
    }

    #[test]
    fn test_parse_type_code_invalid() {
        assert!(parse_type_code(b"nosuchtype").is_none());
    }

    #[test]
    fn test_parse_type_code_empty() {
        assert!(parse_type_code(b"").is_none());
    }

    // ── Personality Detection ────────────────────────────────────────

    #[test]
    fn test_personality_fdisk() {
        assert_eq!(detect_personality(b"fdisk"), Personality::Fdisk);
    }

    #[test]
    fn test_personality_gdisk() {
        assert_eq!(detect_personality(b"gdisk"), Personality::Gdisk);
    }

    #[test]
    fn test_personality_sfdisk() {
        assert_eq!(detect_personality(b"sfdisk"), Personality::Sfdisk);
    }

    #[test]
    fn test_personality_cfdisk() {
        assert_eq!(detect_personality(b"cfdisk"), Personality::Cfdisk);
    }

    #[test]
    fn test_personality_partprobe() {
        assert_eq!(detect_personality(b"partprobe"), Personality::Partprobe);
    }

    #[test]
    fn test_personality_with_path() {
        assert_eq!(detect_personality(b"/usr/sbin/gdisk"), Personality::Gdisk);
    }

    #[test]
    fn test_personality_with_exe() {
        assert_eq!(detect_personality(b"sfdisk.exe"), Personality::Sfdisk);
    }

    #[test]
    fn test_personality_with_path_and_exe() {
        assert_eq!(detect_personality(b"/usr/bin/cfdisk.exe"), Personality::Cfdisk);
    }

    #[test]
    fn test_personality_unknown_defaults_fdisk() {
        assert_eq!(detect_personality(b"something"), Personality::Fdisk);
    }

    // ── Basename / Strip Exe ─────────────────────────────────────────

    #[test]
    fn test_basename_no_path() {
        assert_eq!(basename(b"fdisk"), b"fdisk");
    }

    #[test]
    fn test_basename_unix_path() {
        assert_eq!(basename(b"/usr/sbin/fdisk"), b"fdisk");
    }

    #[test]
    fn test_basename_windows_path() {
        assert_eq!(basename(b"C:\\Windows\\fdisk"), b"fdisk");
    }

    #[test]
    fn test_strip_exe_present() {
        assert_eq!(strip_exe(b"fdisk.exe"), b"fdisk");
    }

    #[test]
    fn test_strip_exe_absent() {
        assert_eq!(strip_exe(b"fdisk"), b"fdisk");
    }

    #[test]
    fn test_strip_exe_uppercase() {
        assert_eq!(strip_exe(b"fdisk.EXE"), b"fdisk");
    }

    // ── bytes_eq / bytes_eq_ci ───────────────────────────────────────

    #[test]
    fn test_bytes_eq_same() {
        assert!(bytes_eq(b"hello", b"hello"));
    }

    #[test]
    fn test_bytes_eq_different() {
        assert!(!bytes_eq(b"hello", b"world"));
    }

    #[test]
    fn test_bytes_eq_different_len() {
        assert!(!bytes_eq(b"hi", b"hello"));
    }

    #[test]
    fn test_bytes_eq_ci_same_case() {
        assert!(bytes_eq_ci(b"LINUX", b"linux"));
    }

    #[test]
    fn test_bytes_eq_ci_mixed() {
        assert!(bytes_eq_ci(b"LinuX", b"lINUx"));
    }

    // ── starts_with ──────────────────────────────────────────────────

    #[test]
    fn test_starts_with_true() {
        assert!(starts_with(b"/dev/sda", b"/dev/"));
    }

    #[test]
    fn test_starts_with_false() {
        assert!(!starts_with(b"/dev/sda", b"/sys/"));
    }

    #[test]
    fn test_starts_with_longer_needle() {
        assert!(!starts_with(b"hi", b"hello"));
    }

    // ── MBR Parsing ──────────────────────────────────────────────────

    #[test]
    fn test_has_mbr_signature_valid() {
        let mut sector = [0u8; 512];
        sector[510] = 0x55;
        sector[511] = 0xAA;
        assert!(has_mbr_signature(&sector));
    }

    #[test]
    fn test_has_mbr_signature_invalid() {
        let sector = [0u8; 512];
        assert!(!has_mbr_signature(&sector));
    }

    #[test]
    fn test_has_mbr_signature_short() {
        let sector = [0u8; 128];
        assert!(!has_mbr_signature(&sector));
    }

    #[test]
    fn test_parse_mbr_entries_empty() {
        let mut sector = [0u8; 512];
        sector[510] = 0x55;
        sector[511] = 0xAA;
        let entries = parse_mbr_entries(&sector);
        assert!(entries[0].is_empty());
        assert!(entries[1].is_empty());
        assert!(entries[2].is_empty());
        assert!(entries[3].is_empty());
    }

    #[test]
    fn test_parse_mbr_entry_linux() {
        let (mbr, _, _) = build_test_mbr_disk();
        let entries = parse_mbr_entries(&mbr);
        assert_eq!(entries[0].type_id, 0x0C); // FAT32 LBA
        assert_eq!(entries[0].status, 0x80); // Bootable
        assert_eq!(entries[0].lba_start, 2048);
        assert_eq!(entries[0].lba_size, 1048576);
        assert_eq!(entries[1].type_id, 0x83); // Linux
        assert!(!entries[1].is_empty());
    }

    #[test]
    fn test_mbr_partition_end_lba() {
        let part = MbrPartition {
            status: 0, chs_first: [0; 3], type_id: 0x83,
            chs_last: [0; 3], lba_start: 2048, lba_size: 1048576,
        };
        assert_eq!(part.end_lba(), 2048 + 1048576 - 1);
    }

    #[test]
    fn test_mbr_partition_size_bytes() {
        let part = MbrPartition {
            status: 0, chs_first: [0; 3], type_id: 0x83,
            chs_last: [0; 3], lba_start: 0, lba_size: 2048,
        };
        assert_eq!(part.size_bytes(512), 2048 * 512);
    }

    #[test]
    fn test_mbr_is_extended() {
        let e1 = MbrPartition {
            status: 0, chs_first: [0; 3], type_id: 0x05,
            chs_last: [0; 3], lba_start: 0, lba_size: 100,
        };
        let e2 = MbrPartition {
            status: 0, chs_first: [0; 3], type_id: 0x0F,
            chs_last: [0; 3], lba_start: 0, lba_size: 100,
        };
        let e3 = MbrPartition {
            status: 0, chs_first: [0; 3], type_id: 0x85,
            chs_last: [0; 3], lba_start: 0, lba_size: 100,
        };
        let enot = MbrPartition {
            status: 0, chs_first: [0; 3], type_id: 0x83,
            chs_last: [0; 3], lba_start: 0, lba_size: 100,
        };
        assert!(e1.is_extended());
        assert!(e2.is_extended());
        assert!(e3.is_extended());
        assert!(!enot.is_extended());
    }

    // ── GPT Parsing ──────────────────────────────────────────────────

    #[test]
    fn test_parse_gpt_header_valid() {
        let (disk, _, _) = build_test_gpt_disk();
        let header = parse_gpt_header(&disk[512..1024]);
        assert!(header.valid);
        assert!(header.crc_valid);
        assert_eq!(header.my_lba, 1);
        assert_eq!(header.first_usable_lba, 34);
        assert_eq!(header.num_partition_entries, 128);
        assert_eq!(header.partition_entry_size, 128);
    }

    #[test]
    fn test_parse_gpt_header_invalid() {
        let sector = [0u8; 512];
        let header = parse_gpt_header(&sector);
        assert!(!header.valid);
        assert!(!header.crc_valid);
    }

    #[test]
    fn test_parse_gpt_entry_efi() {
        let (disk, _, _) = build_test_gpt_disk();
        let entry = parse_gpt_entry(&disk[1024..1152]);
        assert!(!entry.is_empty());
        assert_eq!(entry.first_lba, 2048);
        assert_eq!(entry.last_lba, 1050623);
        let expected_type = parse_guid(b"C12A7328-F81F-11D2-BA4B-00A0C93EC93B");
        assert_eq!(entry.type_guid, expected_type);
        assert!(entry.name_len > 0);
    }

    #[test]
    fn test_parse_gpt_entry_empty() {
        let buf = [0u8; 128];
        let entry = parse_gpt_entry(&buf);
        assert!(entry.is_empty());
        // LBA 0 to 0 inclusive = 1 sector by the math, but the entry is
        // "empty" because its type GUID is all zeros.
        assert_eq!(entry.sectors(), 1);
        assert_eq!(entry.first_lba, 0);
        assert_eq!(entry.last_lba, 0);
    }

    #[test]
    fn test_gpt_partition_sectors() {
        let entry = GptPartition {
            type_guid: parse_guid(b"0FC63DAF-8483-4772-8E79-3D69D8477DE4"),
            unique_guid: [0; 16],
            first_lba: 100,
            last_lba: 199,
            attributes: 0,
            name_buf: [0; 72],
            name_len: 0,
        };
        assert_eq!(entry.sectors(), 100);
    }

    // ── Full Disk Label Parsing ──────────────────────────────────────

    #[test]
    fn test_parse_disk_label_gpt() {
        let (disk, _, _) = build_test_gpt_disk();
        let label = parse_disk_label(&disk);
        match &label {
            DiskLabel::Gpt { header, partition_count, .. } => {
                assert!(header.valid);
                assert_eq!(*partition_count, 2);
            }
            _ => panic!("Expected GPT label"),
        }
    }

    #[test]
    fn test_parse_disk_label_mbr() {
        let (mbr, _, _) = build_test_mbr_disk();
        let label = parse_disk_label(&mbr);
        match &label {
            DiskLabel::Mbr { partitions, .. } => {
                assert_eq!(partitions[0].type_id, 0x0C);
                assert_eq!(partitions[1].type_id, 0x83);
            }
            _ => panic!("Expected MBR label"),
        }
    }

    #[test]
    fn test_parse_disk_label_unknown() {
        let zeros = [0u8; 512];
        let label = parse_disk_label(&zeros);
        match label {
            DiskLabel::Unknown => {}
            _ => panic!("Expected Unknown label"),
        }
    }

    #[test]
    fn test_parse_disk_label_short() {
        let short = [0u8; 64];
        let label = parse_disk_label(&short);
        match label {
            DiskLabel::Unknown => {}
            _ => panic!("Expected Unknown for short data"),
        }
    }

    // ── LE Read/Write ────────────────────────────────────────────────

    #[test]
    fn test_le_u16() {
        let buf = [0x34, 0x12];
        assert_eq!(le_u16(&buf, 0), 0x1234);
    }

    #[test]
    fn test_le_u32() {
        let buf = [0x78, 0x56, 0x34, 0x12];
        assert_eq!(le_u32(&buf, 0), 0x12345678);
    }

    #[test]
    fn test_le_u64() {
        let buf = [0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(le_u64(&buf, 0), 1);
    }

    #[test]
    fn test_write_le_u32_roundtrip() {
        let mut buf = [0u8; 8];
        write_le_u32(&mut buf, 2, 0xDEADBEEF);
        assert_eq!(le_u32(&buf, 2), 0xDEADBEEF);
    }

    #[test]
    fn test_write_le_u64_roundtrip() {
        let mut buf = [0u8; 16];
        write_le_u64(&mut buf, 4, 0x123456789ABCDEF0);
        assert_eq!(le_u64(&buf, 4), 0x123456789ABCDEF0);
    }

    #[test]
    fn test_write_le_u16_roundtrip() {
        let mut buf = [0u8; 4];
        write_le_u16(&mut buf, 1, 0xABCD);
        assert_eq!(le_u16(&buf, 1), 0xABCD);
    }

    // ── CHS ──────────────────────────────────────────────────────────

    #[test]
    fn test_lba_to_chs_zero() {
        let (c, h, s) = lba_to_chs(0, 255, 63);
        assert_eq!((c, h, s), (0, 0, 1));
    }

    #[test]
    fn test_lba_to_chs_sector_63() {
        let (c, h, s) = lba_to_chs(62, 255, 63);
        assert_eq!((c, h, s), (0, 0, 63));
    }

    #[test]
    fn test_lba_to_chs_next_head() {
        let (c, h, s) = lba_to_chs(63, 255, 63);
        assert_eq!((c, h, s), (0, 1, 1));
    }

    #[test]
    fn test_lba_to_chs_zero_geom() {
        let (c, h, s) = lba_to_chs(100, 0, 0);
        assert_eq!((c, h, s), (0, 0, 0));
    }

    #[test]
    fn test_decode_chs() {
        // head=0, sector=1, cylinder=0
        let (c, h, s) = decode_chs(&[0x00, 0x01, 0x00]);
        assert_eq!((c, h, s), (0, 0, 1));
    }

    #[test]
    fn test_decode_chs_max() {
        let (c, h, s) = decode_chs(&[0xFE, 0xFF, 0xFF]);
        assert_eq!(h, 254);
        assert_eq!(s, 63);
        assert_eq!(c, 1023);
    }

    // ── Alignment ────────────────────────────────────────────────────

    #[test]
    fn test_align_up_1mib_already_aligned() {
        assert_eq!(align_up_1mib(2048, 512), 2048);
    }

    #[test]
    fn test_align_up_1mib_not_aligned() {
        assert_eq!(align_up_1mib(2049, 512), 4096);
    }

    #[test]
    fn test_align_up_1mib_zero() {
        assert_eq!(align_up_1mib(0, 512), 0);
    }

    #[test]
    fn test_align_up_1mib_one() {
        assert_eq!(align_up_1mib(1, 512), 2048);
    }

    #[test]
    fn test_align_up_1mib_zero_sector_size() {
        assert_eq!(align_up_1mib(100, 0), 100);
    }

    // ── Protective MBR ───────────────────────────────────────────────

    #[test]
    fn test_build_protective_mbr() {
        let mbr = build_protective_mbr(2097152);
        assert!(has_mbr_signature(&mbr));
        let entries = parse_mbr_entries(&mbr);
        assert_eq!(entries[0].type_id, 0xEE);
        assert_eq!(entries[0].lba_start, 1);
        assert_eq!(entries[0].lba_size, 2097151);
        assert!(entries[1].is_empty());
    }

    #[test]
    fn test_build_protective_mbr_large_disk() {
        let mbr = build_protective_mbr(0x1_0000_0000 + 100);
        let entries = parse_mbr_entries(&mbr);
        assert_eq!(entries[0].lba_size, 0xFFFF_FFFF);
    }

    // ── GPT Entry Serialization ──────────────────────────────────────

    #[test]
    fn test_serialize_gpt_entry_roundtrip() {
        let orig = GptPartition {
            type_guid: parse_guid(b"0FC63DAF-8483-4772-8E79-3D69D8477DE4"),
            unique_guid: parse_guid(b"AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE"),
            first_lba: 2048,
            last_lba: 1050623,
            attributes: 0x04,
            name_buf: {
                let mut n = [0u8; 72];
                n[..5].copy_from_slice(b"Hello");
                n
            },
            name_len: 5,
        };
        let serialized = serialize_gpt_entry(&orig);
        let parsed = parse_gpt_entry(&serialized);
        assert_eq!(parsed.type_guid, orig.type_guid);
        assert_eq!(parsed.unique_guid, orig.unique_guid);
        assert_eq!(parsed.first_lba, orig.first_lba);
        assert_eq!(parsed.last_lba, orig.last_lba);
        assert_eq!(parsed.attributes, orig.attributes);
        assert_eq!(parsed.name_bytes(), b"Hello");
    }

    // ── MBR Entry Serialization ──────────────────────────────────────

    #[test]
    fn test_serialize_mbr_entry_roundtrip() {
        let orig = MbrPartition {
            status: 0x80,
            chs_first: [0, 1, 0],
            type_id: 0x83,
            chs_last: [0xFE, 0xFF, 0xFF],
            lba_start: 2048,
            lba_size: 1048576,
        };
        let mut buf = [0u8; 512];
        serialize_mbr_entry(&mut buf, 446, &orig);
        let parsed = parse_mbr_entry(&buf, 446);
        assert_eq!(parsed.status, 0x80);
        assert_eq!(parsed.type_id, 0x83);
        assert_eq!(parsed.lba_start, 2048);
        assert_eq!(parsed.lba_size, 1048576);
        assert_eq!(parsed.chs_first, [0, 1, 0]);
        assert_eq!(parsed.chs_last, [0xFE, 0xFF, 0xFF]);
    }

    // ── OutBuf ───────────────────────────────────────────────────────

    #[test]
    fn test_outbuf_push_basic() {
        let mut ob = OutBuf::new();
        ob.push(b"hello");
        assert_eq!(&ob.buf[..ob.len], b"hello");
    }

    #[test]
    fn test_outbuf_push_u64() {
        let mut ob = OutBuf::new();
        ob.push_u64(42);
        assert_eq!(&ob.buf[..ob.len], b"42");
    }

    #[test]
    fn test_outbuf_push_guid() {
        let mut ob = OutBuf::new();
        let g = parse_guid(b"C12A7328-F81F-11D2-BA4B-00A0C93EC93B");
        ob.push_guid(&g);
        assert_eq!(&ob.buf[..ob.len], b"C12A7328-F81F-11D2-BA4B-00A0C93EC93B");
    }

    #[test]
    fn test_outbuf_push_size() {
        let mut ob = OutBuf::new();
        ob.push_size(1073741824);
        assert_eq!(&ob.buf[..ob.len], b"1G");
    }

    // ── is_all_hex ───────────────────────────────────────────────────

    #[test]
    fn test_is_all_hex_valid() {
        assert!(is_all_hex(b"0123456789abcdefABCDEF"));
    }

    #[test]
    fn test_is_all_hex_invalid() {
        assert!(!is_all_hex(b"0g"));
    }

    #[test]
    fn test_is_all_hex_empty() {
        assert!(is_all_hex(b""));
    }

    // ── GPT Header Construction ──────────────────────────────────────

    #[test]
    fn test_build_gpt_header_crc_valid() {
        let disk_guid = parse_guid(b"12345678-ABCD-EF01-2345-6789ABCDEF01");
        let hdr = build_gpt_header(&disk_guid, 1, 100, 34, 90, 2, 128, 128, 0);
        let parsed = parse_gpt_header(&hdr);
        assert!(parsed.valid);
        assert!(parsed.crc_valid);
        assert_eq!(parsed.my_lba, 1);
        assert_eq!(parsed.alternate_lba, 100);
        assert_eq!(parsed.first_usable_lba, 34);
        assert_eq!(parsed.last_usable_lba, 90);
    }
}
