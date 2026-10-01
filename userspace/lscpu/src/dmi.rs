//! `lscpu-dmi.c`, and the half of `lscpu-virt.c` that reads the firmware's
//! DMI (SMBIOS) tables: the BIOS's name for the processor, the number of
//! sockets, and a hypervisor vendor that names itself in them.
//!
//! All of it reads the live machine -- `/sys/firmware/dmi/tables/DMI`,
//! `/sys/firmware/efi/systab`, `/dev/mem` -- never `--sysroot`'s copy, and
//! every file involved is readable only by root; as anyone else it finds
//! nothing, as upstream does.
//!
//! The table walk is upstream's, bounds and all: the table's length and
//! entry count are `uint16_t`, so a table past 64 KiB is walked only as far
//! as its length modulo 65536; and a structure's fields are read at fixed
//! offsets whether or not the structure is that long -- bytes past the end
//! of what was read count as zero here, where upstream reads past its
//! buffer.

use crate::path::read_all;
use crate::types::{
    CpuType, VIRT_VENDOR_HITACHI, VIRT_VENDOR_INNOTEK, VIRT_VENDOR_NONE, VIRT_VENDOR_PARALLELS,
};
use std::fs::File;
use std::io::{BufReader, Seek, SeekFrom};

/// `_PATH_SYS_DMI`.
pub const PATH_SYS_DMI: &str = "/sys/firmware/dmi/tables/DMI";
/// `_PATH_DEV_MEM`.
const PATH_DEV_MEM: &str = "/dev/mem";

/// `struct dmi_info`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DmiInfo {
    pub vendor: Option<Vec<u8>>,
    pub product: Option<Vec<u8>>,
    pub manufacturer: Option<Vec<u8>>,
    pub sockets: i32,
    pub processor_family: u16,
    pub processor_manufacturer: Option<Vec<u8>>,
    pub processor_version: Option<Vec<u8>>,
    pub current_speed: u16,
    pub part_num: Option<Vec<u8>>,
}

/// A byte of the table; 0 past what was read.
fn byte(data: &[u8], at: usize) -> u8 {
    data.get(at).copied().unwrap_or(0)
}

/// `WORD(x)`: a little-endian `uint16_t`.
fn word(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([byte(data, at), byte(data, at.saturating_add(1))])
}

/// `DWORD(x)`: a little-endian `uint32_t`.
fn dword(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([
        byte(data, at),
        byte(data, at.saturating_add(1)),
        byte(data, at.saturating_add(2)),
        byte(data, at.saturating_add(3)),
    ])
}

/// `dmi_string(dm, s)`: the structure's `s`th string, counting from 1,
/// which follow its formatted area; `None` for string 0 or past the last.
fn dmi_string(data: &[u8], start: usize, length: u8, s: u8) -> Option<Vec<u8>> {
    if s == 0 {
        return None;
    }
    let mut bp = start.saturating_add(usize::from(length));
    let mut s = s;
    while s > 1 && byte(data, bp) != 0 {
        let len = data
            .get(bp..)
            .map_or(0, |r| r.iter().position(|&b| b == 0).unwrap_or(r.len()));
        bp = bp.saturating_add(len).saturating_add(1);
        s = s.saturating_sub(1);
    }
    if byte(data, bp) == 0 {
        return None;
    }
    let rest = data.get(bp..).unwrap_or_default();
    Some(crate::cstr::c_str(rest).to_vec())
}

/// `parse_dmi_table(len, num, data, &di)`: the BIOS, system and first
/// processor structures, and the processor structures counted. -1 when a
/// structure is shorter than its own header, after which nothing can be
/// found.
pub fn parse_dmi_table(len: u16, num: u16, data: &[u8], di: &mut DmiInfo) -> i32 {
    let len = usize::from(len);
    let mut at = 0usize;
    let mut i = 0u16;
    while i < num && at.saturating_add(4) <= len {
        let kind = byte(data, at);
        let length = byte(data, at.saturating_add(1));
        if length < 4 {
            return -1;
        }
        // The next structure starts after the double NUL ending this one's
        // strings.
        let mut next = at.saturating_add(usize::from(length));
        while next.saturating_add(1) < len
            && (byte(data, next) != 0 || byte(data, next.saturating_add(1)) != 0)
        {
            next = next.saturating_add(1);
        }
        next = next.saturating_add(2);
        let field = |off: usize| byte(data, at.saturating_add(off));
        match kind {
            0 => di.vendor = dmi_string(data, at, length, field(0x04)),
            1 => {
                di.manufacturer = dmi_string(data, at, length, field(0x04));
                di.product = dmi_string(data, at, length, field(0x05));
            }
            4 => {
                if di.sockets == 0 {
                    di.processor_manufacturer = dmi_string(data, at, length, field(0x07));
                    di.processor_version = dmi_string(data, at, length, field(0x10));
                    di.current_speed = word(data, at.saturating_add(0x16));
                    di.part_num = dmi_string(data, at, length, field(0x22));
                    di.processor_family = if field(0x06) == 0xfe {
                        word(data, at.saturating_add(0x28))
                    } else {
                        u16::from(field(0x06))
                    };
                }
                di.sockets = di.sockets.saturating_add(1);
            }
            _ => {}
        }
        at = next;
        i = i.saturating_add(1);
    }
    0
}

/// `get_mem_chunk(base, len, devmem)`: `len` bytes from `base`, zero past
/// the end of what could be read; `None` when the file will not open, seek
/// or read at all.
fn get_mem_chunk(base: u64, len: usize, devmem: &str) -> Option<Vec<u8>> {
    let mut file = File::open(devmem).ok()?;
    file.seek(SeekFrom::Start(base)).ok()?;
    let mut data = read_all(&mut file, len).ok()?;
    data.resize(len, 0);
    Some(data)
}

/// `access(_PATH_SYS_DMI, R_OK) == 0`, as opening it for reading answers.
#[must_use]
pub fn readable() -> bool {
    File::open(PATH_SYS_DMI).is_ok()
}

/// The table file's size, `stat`'s `st_size`.
fn dmi_file_size() -> Option<u64> {
    std::fs::metadata(PATH_SYS_DMI).ok().map(|m| m.len())
}

/// `dmi_decode_cputype(ct)`: the BIOS's vendor, model name and family of
/// the first processor.
pub fn decode_cputype(ct: &mut CpuType) {
    let Some(size) = dmi_file_size() else {
        return;
    };
    let Some(data) = get_mem_chunk(0, usize::try_from(size).unwrap_or(0), PATH_SYS_DMI) else {
        return;
    };
    let mut di = DmiInfo::default();
    // `st_size` and `st_size / 4` into `uint16_t`s.
    if parse_dmi_table(size as u16, (size / 4) as u16, &data, &mut di) < 0 {
        return;
    }
    if let Some(m) = di.processor_manufacturer.clone() {
        ct.bios_vendor = Some(m);
    }
    ct.bios_modelname = Some(bios_modelname(&di));
    ct.bios_family = Some(di.processor_family.to_string().into_bytes());
}

/// `"%s %s CPU @ %d.%dGHz"` in a 100-byte buffer.
#[must_use]
pub fn bios_modelname(di: &DmiInfo) -> Vec<u8> {
    let mut text = di.processor_version.clone().unwrap_or_default();
    text.push(b' ');
    text.extend_from_slice(di.part_num.as_deref().unwrap_or_default());
    let speed = di.current_speed;
    text.extend_from_slice(
        format!(" CPU @ {}.{}GHz", speed / 1000, (speed % 1000) / 100).as_bytes(),
    );
    text.truncate(99);
    text
}

/// `get_number_of_physical_sockets_from_dmi()`.
#[must_use]
pub fn physical_sockets() -> usize {
    let Some(size) = dmi_file_size() else {
        return 0;
    };
    let Some(data) = get_mem_chunk(0, usize::try_from(size).unwrap_or(0), PATH_SYS_DMI) else {
        return 0;
    };
    let mut di = DmiInfo::default();
    if parse_dmi_table(size as u16, (size / 4) as u16, &data, &mut di) < 0 || di.sockets == 0 {
        return 0;
    }
    usize::try_from(di.sockets).unwrap_or(0)
}

/// `hypervisor_from_dmi_table(base, len, num, devmem)`: a vendor the
/// system structure names -- VirtualBox's `innotek GmbH`, a Hitachi LPAR,
/// Parallels -- or none; -1 for a table that will not parse.
fn hypervisor_from_dmi_table(base: u32, len: u16, num: u16, devmem: &str) -> i64 {
    let Some(data) = get_mem_chunk(u64::from(base), usize::from(len), devmem) else {
        return VIRT_VENDOR_NONE as i64;
    };
    let mut di = DmiInfo::default();
    if parse_dmi_table(len, num, &data, &mut di) < 0 {
        return -1;
    }
    vendor_of(&di) as i64
}

/// The hypervisor a parsed table names.
#[must_use]
pub fn vendor_of(di: &DmiInfo) -> usize {
    let manufacturer = di.manufacturer.as_deref();
    let has = |s: Option<&[u8]>, needle: &[u8]| {
        s.is_some_and(|s| crate::cstr::strstr(s, needle).is_some())
    };
    if manufacturer == Some(b"innotek GmbH") {
        VIRT_VENDOR_INNOTEK
    } else if has(manufacturer, b"HITACHI") && has(di.product.as_deref(), b"LPAR") {
        VIRT_VENDOR_HITACHI
    } else if di.vendor.as_deref() == Some(b"Parallels") {
        VIRT_VENDOR_PARALLELS
    } else {
        VIRT_VENDOR_NONE
    }
}

/// `checksum(buf, len)`: the bytes sum to 0 modulo 256.
fn checksum(buf: &[u8], at: usize, len: usize) -> bool {
    (0..len).fold(0u8, |sum, i| {
        sum.wrapping_add(byte(buf, at.saturating_add(i)))
    }) == 0
}

/// `hypervisor_decode_legacy(buf, devmem)`: a bare `_DMI_` anchor.
fn decode_legacy(buf: &[u8], at: usize) -> i64 {
    if !checksum(buf, at, 0x0f) {
        return -1;
    }
    hypervisor_from_dmi_table(
        dword(buf, at.saturating_add(0x08)),
        word(buf, at.saturating_add(0x06)),
        word(buf, at.saturating_add(0x0c)),
        PATH_DEV_MEM,
    )
}

/// `hypervisor_decode_smbios(buf, devmem)`: an `_SM_` anchor, with the
/// `_DMI_` one inside it.
fn decode_smbios(buf: &[u8], at: usize) -> i64 {
    if !checksum(buf, at, usize::from(byte(buf, at.saturating_add(0x05))))
        || buf.get(at.saturating_add(0x10)..at.saturating_add(0x15)) != Some(b"_DMI_")
        || !checksum(buf, at.saturating_add(0x10), 0x0f)
    {
        return -1;
    }
    hypervisor_from_dmi_table(
        dword(buf, at.saturating_add(0x18)),
        word(buf, at.saturating_add(0x16)),
        word(buf, at.saturating_add(0x1c)),
        PATH_DEV_MEM,
    )
}

/// `address_from_efi(&address)`: `Ok(address)` of the SMBIOS table the EFI
/// system table names; `Err(-1)` without EFI, `Err(-2)` when it names none.
fn address_from_efi() -> Result<u64, i32> {
    let file = File::open("/sys/firmware/efi/systab")
        .or_else(|_| File::open("/proc/efi/systab"))
        .map_err(|_| -1)?;
    let mut reader = BufReader::new(file);
    while let Some(line) = crate::cstr::fgets(&mut reader, 63) {
        let line = crate::cstr::c_str(&line);
        let Some(eq) = line.iter().position(|&b| b == b'=') else {
            continue;
        };
        if line.get(..eq) != Some(b"SMBIOS") {
            continue;
        }
        // `strtoul(addrp, NULL, 0)`, refused only on ERANGE.
        let addr = line.get(eq.saturating_add(1)..).unwrap_or_default();
        match ulstrutils::scan_integer(addr, 0) {
            // ERANGE: on to the next line.
            Some(sc) if sc.saturated || sc.magnitude > u128::from(u64::MAX) => {}
            Some(sc) => {
                let m = u64::try_from(sc.magnitude).unwrap_or(0);
                return Ok(if sc.negative { m.wrapping_neg() } else { m });
            }
            None => return Ok(0),
        }
    }
    Err(-2)
}

/// `read_hypervisor_dmi_from_devmem()`.
fn from_devmem() -> i64 {
    let mut rc = VIRT_VENDOR_NONE as i64;
    match address_from_efi() {
        Err(-2) => return rc,
        Ok(fp) => {
            let Some(buf) = get_mem_chunk(fp, 0x20, PATH_DEV_MEM) else {
                return rc;
            };
            rc = decode_smbios(&buf, 0);
            if rc >= VIRT_VENDOR_NONE as i64 {
                return rc;
            }
        }
        Err(_) => {}
    }
    if cfg!(any(target_arch = "x86_64", target_arch = "x86")) {
        let Some(buf) = get_mem_chunk(0xF0000, 0x10000, PATH_DEV_MEM) else {
            return rc;
        };
        // Upstream's scan stops as soon as `rc` is not negative -- which it
        // is not to begin with, unless the EFI anchor failed -- so it looks
        // at the first 16 bytes and no further.
        let mut fp = 0usize;
        while fp <= 0xFFF0 {
            if buf.get(fp..fp.saturating_add(4)) == Some(b"_SM_") && fp <= 0xFFE0 {
                rc = decode_smbios(&buf, fp);
                if rc < 0 {
                    fp = fp.saturating_add(16);
                }
            } else if buf.get(fp..fp.saturating_add(5)) == Some(b"_DMI_") {
                rc = decode_legacy(&buf, fp);
            }
            if rc >= VIRT_VENDOR_NONE as i64 {
                break;
            }
            fp = fp.saturating_add(16);
        }
    }
    rc
}

/// `read_hypervisor_dmi()`: the sysfs table -- or `/dev/mem`, when that is
/// missing or does not parse. (One that will not open, as it will not for
/// anyone but root, is an answer: none.)
#[must_use]
pub fn read_hypervisor_dmi() -> usize {
    let from_sysfw = match dmi_file_size() {
        // `st_size` and `st_size / 4` into `uint16_t`s.
        Some(size) => hypervisor_from_dmi_table(0, size as u16, (size / 4) as u16, PATH_SYS_DMI),
        None => -1,
    };
    let rc = if from_sysfw < 0 {
        from_devmem()
    } else {
        from_sysfw
    };
    usize::try_from(rc).unwrap_or(VIRT_VENDOR_NONE)
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    reason = "the tables are built at fixed offsets the test just sized"
)]
mod tests {
    use super::*;

    /// A table: BIOS (vendor "Acme"), system ("innotek GmbH", "VirtualBox"),
    /// and two processors.
    fn table() -> Vec<u8> {
        let mut t = Vec::new();
        // Type 0, length 0x18, vendor string 1.
        let mut s0 = vec![0u8; 0x18];
        s0[0] = 0;
        s0[1] = 0x18;
        s0[4] = 1;
        t.extend_from_slice(&s0);
        t.extend_from_slice(b"Acme\0\0");
        // Type 1, length 0x1b, manufacturer 1, product 2.
        let mut s1 = vec![0u8; 0x1b];
        s1[0] = 1;
        s1[1] = 0x1b;
        s1[4] = 1;
        s1[5] = 2;
        t.extend_from_slice(&s1);
        t.extend_from_slice(b"innotek GmbH\0VirtualBox\0\0");
        for n in 0..2u8 {
            let mut s4 = vec![0u8; 0x30];
            s4[0] = 4;
            s4[1] = 0x30;
            s4[0x06] = 0xfe;
            s4[0x07] = 1;
            s4[0x10] = 2;
            s4[0x16] = 0x98; // 2200 MHz, little-endian
            s4[0x17] = 0x08;
            s4[0x22] = 3;
            s4[0x28] = 0x19;
            s4[0x29] = 0x01;
            t.extend_from_slice(&s4);
            t.extend_from_slice(format!("Maker{n}\0Version{n}\0Part{n}\0\0").as_bytes());
        }
        t
    }

    #[test]
    fn tables_parse_as_upstream() {
        let t = table();
        let mut di = DmiInfo::default();
        let len = u16::try_from(t.len()).unwrap_or(0);
        assert_eq!(parse_dmi_table(len, len / 4, &t, &mut di), 0);
        assert_eq!(di.vendor.as_deref(), Some(&b"Acme"[..]));
        assert_eq!(di.manufacturer.as_deref(), Some(&b"innotek GmbH"[..]));
        assert_eq!(di.product.as_deref(), Some(&b"VirtualBox"[..]));
        assert_eq!(di.sockets, 2);
        assert_eq!(di.processor_manufacturer.as_deref(), Some(&b"Maker0"[..]));
        assert_eq!(di.processor_family, 0x119);
        assert_eq!(di.current_speed, 2200);
        assert_eq!(bios_modelname(&di), b"Version0 Part0 CPU @ 2.2GHz".to_vec());
        assert_eq!(vendor_of(&di), VIRT_VENDOR_INNOTEK);
    }

    #[test]
    fn a_short_structure_stops_the_walk() {
        let mut t = table();
        t[1] = 3;
        let mut di = DmiInfo::default();
        assert_eq!(parse_dmi_table(200, 50, &t, &mut di), -1);
    }

    #[test]
    fn strings_past_the_last_are_none() {
        let t = table();
        let mut di = DmiInfo::default();
        let mut t2 = t.clone();
        // The BIOS vendor is string 5 of 1.
        t2[4] = 5;
        let len = u16::try_from(t2.len()).unwrap_or(0);
        parse_dmi_table(len, len / 4, &t2, &mut di);
        assert!(di.vendor.is_none());
        let _ = t;
    }

    #[test]
    fn checksums_wrap() {
        assert!(checksum(&[0x80, 0x80], 0, 2));
        assert!(!checksum(&[1], 0, 1));
        assert!(checksum(&[], 0, 0));
    }
}
