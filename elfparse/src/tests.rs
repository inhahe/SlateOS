//! Host tests for the parser: every check `parse` and `loadable_segments`
//! make, the note walk, the ABI markers, the interpreter path and
//! `image_end`'s arithmetic.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use super::*;

/// A program header to write into a test file.
#[derive(Clone, Copy)]
struct Ph {
    p_type: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    filesz: u64,
    memsz: u64,
    align: u64,
}

/// A `PT_LOAD` of `len` bytes of the file at `offset`, loaded at `vaddr`.
fn load(offset: u64, vaddr: u64, len: u64, flags: u32) -> Ph {
    Ph {
        p_type: PT_LOAD,
        flags,
        offset,
        vaddr,
        filesz: len,
        memsz: len,
        align: 0x1000,
    }
}

/// A segment of type `p_type` over `len` bytes of the file at `offset`.
fn segment(p_type: u32, offset: u64, len: u64, align: u64) -> Ph {
    Ph {
        p_type,
        flags: PF_R,
        offset,
        vaddr: 0,
        filesz: len,
        memsz: len,
        align,
    }
}

/// Where the payload of a file made by [`file`] with `n` program headers
/// begins.
fn payload_at(n: usize) -> u64 {
    (ELF64_EHDR_SIZE + n * ELF64_PHDR_SIZE) as u64
}

/// An ELF64 x86-64 file of type `e_type` entered at `entry`, with `EI_OSABI`
/// `osabi`, program headers `phdrs` right after the header, and `payload`
/// after them.
fn file(e_type: u16, entry: u64, osabi: u8, phdrs: &[Ph], payload: &[u8]) -> Vec<u8> {
    let mut b = vec![0u8; ELF64_EHDR_SIZE];
    b[..4].copy_from_slice(&ELF_MAGIC);
    b[EI_CLASS] = ELFCLASS64;
    b[EI_DATA] = ELFDATA2LSB;
    b[EI_VERSION] = EV_CURRENT;
    b[EI_OSABI] = osabi;
    b[16..18].copy_from_slice(&e_type.to_le_bytes());
    b[18..20].copy_from_slice(&EM_X86_64.to_le_bytes());
    b[20..24].copy_from_slice(&1u32.to_le_bytes());
    b[24..32].copy_from_slice(&entry.to_le_bytes());
    let phoff: u64 = if phdrs.is_empty() { 0 } else { 64 };
    b[32..40].copy_from_slice(&phoff.to_le_bytes());
    b[52..54].copy_from_slice(&64u16.to_le_bytes());
    b[54..56].copy_from_slice(&(ELF64_PHDR_SIZE as u16).to_le_bytes());
    b[56..58].copy_from_slice(&(phdrs.len() as u16).to_le_bytes());
    for ph in phdrs {
        b.extend_from_slice(&ph.p_type.to_le_bytes());
        b.extend_from_slice(&ph.flags.to_le_bytes());
        b.extend_from_slice(&ph.offset.to_le_bytes());
        b.extend_from_slice(&ph.vaddr.to_le_bytes());
        b.extend_from_slice(&ph.vaddr.to_le_bytes());
        b.extend_from_slice(&ph.filesz.to_le_bytes());
        b.extend_from_slice(&ph.memsz.to_le_bytes());
        b.extend_from_slice(&ph.align.to_le_bytes());
    }
    b.extend_from_slice(payload);
    b
}

/// A static executable: one read-execute segment of 16 bytes at `0x40_0000`.
fn static_exec() -> Vec<u8> {
    let at = payload_at(1);
    file(
        ET_EXEC,
        0x40_0000,
        ELFOSABI_SYSV,
        &[load(at, 0x40_0000, 16, PF_R | PF_X)],
        &[0x90; 16],
    )
}

/// One note: owner `name` (NUL included), type `n_type`, descriptor `desc`,
/// each part padded to `align`.
fn note(name: &[u8], n_type: u32, desc: &[u8], align: usize) -> Vec<u8> {
    let pad = |v: &mut Vec<u8>| {
        while !v.len().is_multiple_of(align) {
            v.push(0);
        }
    };
    let mut n = Vec::new();
    n.extend_from_slice(&(name.len() as u32).to_le_bytes());
    n.extend_from_slice(&(desc.len() as u32).to_le_bytes());
    n.extend_from_slice(&n_type.to_le_bytes());
    n.extend_from_slice(name);
    pad(&mut n);
    n.extend_from_slice(desc);
    pad(&mut n);
    n
}

/// A position-independent executable whose `PT_INTERP` holds `interp`.
fn dynamic_exec(interp: &[u8]) -> Vec<u8> {
    let at = payload_at(2);
    file(
        ET_DYN,
        0x1000,
        ELFOSABI_SYSV,
        &[
            segment(PT_INTERP, at, interp.len() as u64, 1),
            load(0, 0, at + interp.len() as u64, PF_R | PF_X),
        ],
        interp,
    )
}

#[test]
fn a_static_executable_parses_and_yields_its_segment() {
    let data = static_exec();
    let elf = ElfFile::parse(&data).unwrap();
    assert_eq!(elf.entry_point(), 0x40_0000);
    assert!(!elf.is_pie());
    assert_eq!(elf.program_header_count(), 1);
    assert_eq!(elf.data().len(), data.len());
    let segs: Vec<_> = elf.loadable_segments().unwrap().collect();
    assert_eq!(segs.len(), 1);
    let s = &segs[0];
    assert_eq!((s.vaddr, s.file_size, s.mem_size), (0x40_0000, 16, 16));
    assert_eq!(s.file_offset, payload_at(1));
    assert!(s.readable && s.executable && !s.writable);
    assert_eq!(elf.phdr_table_bytes().unwrap().len(), ELF64_PHDR_SIZE);
    assert_eq!(elf.interp_path(), None);
    assert!(!elf.detect_linux_abi());
}

#[test]
fn every_header_check_refuses() {
    let good = static_exec();
    let refused = |edit: &dyn Fn(&mut Vec<u8>)| {
        let mut data = good.clone();
        edit(&mut data);
        ElfFile::parse(&data).err()
    };
    let m = Some(ElfError::Malformed);
    assert_eq!(refused(&|d| d.truncate(63)), m, "shorter than a header");
    assert_eq!(refused(&|d| d[0] = 0x7E), m, "magic");
    assert_eq!(refused(&|d| d[EI_CLASS] = 1), m, "32-bit");
    assert_eq!(refused(&|d| d[EI_DATA] = 2), m, "big-endian");
    assert_eq!(refused(&|d| d[EI_VERSION] = 0), m, "ident version");
    assert_eq!(refused(&|d| d[16] = 1), m, "relocatable object");
    assert_eq!(refused(&|d| d[18] = 3), m, "i386");
    assert_eq!(refused(&|d| d[20] = 2), m, "e_version");
    assert_eq!(refused(&|d| d[54] = 55), m, "phentsize below 56");
    assert_eq!(refused(&|d| d[54] = 0), m, "phentsize zero");
    assert_eq!(refused(&|d| d[56] = 200), m, "table past the file");
    assert_eq!(
        refused(&|d| d[32..40].copy_from_slice(&u64::MAX.to_le_bytes())),
        m,
        "table offset that overflows"
    );
    assert_eq!(
        refused(&|d| d[24..32].copy_from_slice(&0u64.to_le_bytes())),
        m,
        "ET_EXEC without an entry point"
    );
    assert!(ElfFile::parse(&[]).is_err());
}

#[test]
fn a_pie_may_start_at_zero_and_has_no_entry_check() {
    let mut data = dynamic_exec(b"/lib/ld-musl-x86_64.so.1\0");
    data[24..32].copy_from_slice(&0u64.to_le_bytes());
    let elf = ElfFile::parse(&data).unwrap();
    assert!(elf.is_pie());
    assert_eq!(elf.loadable_segments().unwrap().count(), 1);
}

#[test]
fn no_program_headers_is_a_valid_file_with_nothing_to_load() {
    let data = file(ET_DYN, 0, ELFOSABI_SYSV, &[], &[]);
    let elf = ElfFile::parse(&data).unwrap();
    assert_eq!(elf.program_header_count(), 0);
    assert_eq!(elf.loadable_segments().unwrap().count(), 0);
    assert_eq!(elf.image_end(0, 0x4000), Ok(0));
    assert_eq!(elf.phdr_table_bytes(), Some(&[][..]));
}

#[test]
fn no_program_headers_needs_no_program_header_size() {
    // With no program headers, their entry size is not looked at: linkers
    // write 0 there.
    let mut data = file(ET_DYN, 0, ELFOSABI_SYSV, &[], &[]);
    data[54..56].copy_from_slice(&0u16.to_le_bytes());
    assert!(ElfFile::parse(&data).is_ok());
}

#[test]
fn a_program_header_table_may_end_the_file() {
    let data = file(ET_DYN, 0, ELFOSABI_SYSV, &[segment(PT_NOTE, 0, 0, 4)], &[]);
    assert_eq!(data.len(), payload_at(1) as usize);
    assert!(ElfFile::parse(&data).is_ok());
}

#[test]
fn each_permission_bit_is_read_alone() {
    let at = payload_at(1);
    let one = |flags: u32| {
        let data = file(
            ET_EXEC,
            0x40_0000,
            0,
            &[load(at, 0x40_0000, 16, flags)],
            &[0; 16],
        );
        let s = ElfFile::parse(&data)
            .unwrap()
            .loadable_segments()
            .unwrap()
            .next()
            .unwrap();
        (s.readable, s.writable, s.executable)
    };
    assert_eq!(one(PF_R), (true, false, false));
    assert_eq!(one(PF_W), (false, true, false));
    assert_eq!(one(PF_X), (false, false, true));
    assert_eq!(one(0), (false, false, false));
}

#[test]
fn every_error_says_what_it_is() {
    assert_eq!(
        format!("{}", ElfError::Malformed),
        "malformed or unsupported ELF file"
    );
    assert_eq!(
        format!("{}", ElfError::AddressOverflow),
        "load bias overflows the address space"
    );
    assert_eq!(
        format!("{}", ElfError::BadAlignment),
        "alignment is not a power of two"
    );
}

#[test]
fn every_segment_check_refuses() {
    let at = payload_at(1);
    let with = |ph: Ph| {
        let data = file(ET_EXEC, 0x40_0000, 0, &[ph], &[0; 16]);
        ElfFile::parse(&data).unwrap().loadable_segments().err()
    };
    let m = Some(ElfError::Malformed);
    let base = load(at, 0x40_0000, 16, PF_R);
    assert_eq!(with(Ph { memsz: 8, ..base }), m, "memsz below filesz");
    assert_eq!(
        with(Ph {
            filesz: 17,
            memsz: 17,
            ..base
        }),
        m,
        "bytes past the file"
    );
    assert_eq!(
        with(Ph {
            offset: u64::MAX,
            ..base
        }),
        m,
        "offset that overflows"
    );
    assert_eq!(
        with(Ph {
            vaddr: USER_SPACE_END - 8,
            ..base
        }),
        m,
        "past the user address space"
    );
    assert_eq!(
        with(Ph {
            vaddr: u64::MAX - 4,
            ..base
        }),
        m,
        "address wrap"
    );
    assert_eq!(
        with(Ph {
            vaddr: USER_SPACE_END - 16,
            ..base
        }),
        None,
        "ends exactly at the top"
    );
    // A segment of another type is not checked as a loadable one.
    assert_eq!(
        with(Ph {
            p_type: PT_NOTE,
            filesz: 99,
            memsz: 1,
            ..base
        }),
        None
    );
}

#[test]
fn empty_and_other_segments_are_not_yielded() {
    let at = payload_at(3);
    let data = file(
        ET_EXEC,
        0x40_0000,
        0,
        &[
            load(at, 0x40_0000, 0, PF_R),
            segment(PT_NOTE, at, 0, 4),
            Ph {
                memsz: 32,
                ..load(at, 0x50_0000, 8, PF_R | PF_W)
            },
        ],
        &[0; 8],
    );
    let elf = ElfFile::parse(&data).unwrap();
    let segs: Vec<_> = elf.loadable_segments().unwrap().collect();
    assert_eq!(segs.len(), 1);
    assert_eq!(
        (segs[0].vaddr, segs[0].file_size, segs[0].mem_size),
        (0x50_0000, 8, 32)
    );
    assert!(segs[0].writable && !segs[0].executable);
}

#[test]
fn a_program_header_past_the_count_is_none() {
    let data = static_exec();
    let elf = ElfFile::parse(&data).unwrap();
    assert!(elf.program_header(0).is_some());
    assert!(elf.program_header(1).is_none());
    assert!(elf.program_header(usize::MAX).is_none());
}

#[test]
fn the_interpreter_path_is_cut_at_its_first_nul() {
    let data = dynamic_exec(b"/lib/ld-musl-x86_64.so.1\0junk");
    let elf = ElfFile::parse(&data).unwrap();
    assert_eq!(elf.interp_path(), Some(&b"/lib/ld-musl-x86_64.so.1"[..]));
    // No NUL: taken whole.
    let data = dynamic_exec(b"/x");
    assert_eq!(
        ElfFile::parse(&data).unwrap().interp_path(),
        Some(&b"/x"[..])
    );
    // Empty: refused.
    let data = dynamic_exec(b"\0/x");
    assert_eq!(ElfFile::parse(&data).unwrap().interp_path(), None);
    // Past the file: refused.
    let mut data = dynamic_exec(b"/x\0");
    let filesz_at = ELF64_EHDR_SIZE + 32;
    data[filesz_at..filesz_at + 8].copy_from_slice(&1000u64.to_le_bytes());
    assert_eq!(ElfFile::parse(&data).unwrap().interp_path(), None);
}

#[test]
fn linux_is_detected_by_osabi_by_loader_and_by_gnu_property() {
    let mut data = static_exec();
    data[EI_OSABI] = ELFOSABI_GNU;
    assert!(ElfFile::parse(&data).unwrap().detect_linux_abi(), "OSABI 3");

    let glibc = dynamic_exec(b"/lib64/ld-linux-x86-64.so.2\0");
    assert!(
        ElfFile::parse(&glibc).unwrap().detect_linux_abi(),
        "glibc's loader"
    );
    let musl = dynamic_exec(b"/lib/ld-musl-x86_64.so.1\0");
    assert!(
        ElfFile::parse(&musl).unwrap().detect_linux_abi(),
        "musl's loader"
    );
    let other = dynamic_exec(b"/system/lib/ld.so\0");
    assert!(
        !ElfFile::parse(&other).unwrap().detect_linux_abi(),
        "another loader"
    );

    let at = payload_at(2);
    let property = file(
        ET_EXEC,
        0x40_0000,
        0,
        &[
            segment(PT_GNU_PROPERTY, at, 0, 8),
            load(at, 0x40_0000, 4, PF_R | PF_X),
        ],
        &[0x90; 4],
    );
    assert!(
        ElfFile::parse(&property).unwrap().detect_linux_abi(),
        "PT_GNU_PROPERTY"
    );
}

#[test]
fn the_slateos_marker_outranks_every_linux_signal() {
    let mut data = static_exec();
    data[EI_OSABI] = ELFOSABI_SLATEOS;
    let elf = ElfFile::parse(&data).unwrap();
    assert!(elf.has_slateos_marker());
    assert!(!elf.detect_linux_abi());

    // The note form, beside OSABI 3 and a Linux loader.
    for align in [4usize, 8] {
        let interp = b"/lib/ld-musl-x86_64.so.1\0";
        let notes = note(
            SLATEOS_NOTE_NAME,
            NT_SLATEOS_ABI,
            &1u32.to_le_bytes(),
            align,
        );
        let at = payload_at(3);
        let mut payload = interp.to_vec();
        payload.extend_from_slice(&notes);
        let mut data = file(
            ET_DYN,
            0,
            ELFOSABI_GNU,
            &[
                segment(PT_INTERP, at, interp.len() as u64, 1),
                segment(
                    PT_NOTE,
                    at + interp.len() as u64,
                    notes.len() as u64,
                    align as u64,
                ),
                load(0, 0, at + payload.len() as u64, PF_R | PF_X),
            ],
            &payload,
        );
        let elf = ElfFile::parse(&data).unwrap();
        assert!(elf.has_slateos_marker(), "{align}-aligned note");
        assert!(!elf.detect_linux_abi(), "{align}-aligned note");
        // The same note of another type is no marker.
        let type_at = data.len() - notes.len() + 8;
        data[type_at] = 2;
        let elf = ElfFile::parse(&data).unwrap();
        assert!(!elf.has_slateos_marker());
        assert!(elf.detect_linux_abi());
    }
}

#[test]
fn the_note_walk_finds_a_later_note_and_stops_on_a_bad_one() {
    let mut notes = note(b"GNU\0", 3, &[0; 16], 4);
    notes.extend_from_slice(&note(SLATEOS_NOTE_NAME, NT_SLATEOS_ABI, &[1, 0, 0, 0], 4));
    assert!(notes_contain(&notes, 4, SLATEOS_NOTE_NAME, NT_SLATEOS_ABI));
    assert!(
        !notes_contain(&notes, 4, b"SlateOS", NT_SLATEOS_ABI),
        "the NUL is part of it"
    );

    // A first note whose descriptor runs past the segment hides the rest.
    let mut bad = notes.clone();
    bad[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(!notes_contain(&bad, 4, SLATEOS_NOTE_NAME, NT_SLATEOS_ABI));
    let mut bad = notes.clone();
    bad[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(!notes_contain(&bad, 4, SLATEOS_NOTE_NAME, NT_SLATEOS_ABI));

    // Empty notes advance by their header; a truncated header ends the walk.
    let empty = vec![0u8; 12 * 100];
    assert!(!notes_contain(&empty, 4, SLATEOS_NOTE_NAME, NT_SLATEOS_ABI));
    assert!(!notes_contain(&[0u8; 11], 4, b"", 0));
    assert!(
        notes_contain(&[0u8; 12], 4, b"", 0),
        "an empty name of type 0"
    );
    assert!(!notes_contain(&[], 4, b"", 0));
}

#[test]
fn a_linux_loader_is_named_before_the_first_nul() {
    assert!(is_linux_interp(b"/lib64/ld-linux-x86-64.so.2\0"));
    assert!(is_linux_interp(b"/lib/ld-musl-x86_64.so.1"));
    assert!(!is_linux_interp(b"/x\0/lib64/ld-linux-x86-64.so.2"));
    assert!(!is_linux_interp(b""));
    assert!(!is_linux_interp(b"ld-linux"));
}

#[test]
fn image_end_rounds_up_and_refuses_overflow() {
    let at = payload_at(2);
    let data = file(
        ET_DYN,
        0,
        0,
        &[
            load(at, 0x1000, 8, PF_R),
            Ph {
                memsz: 0x5001,
                ..load(at, 0x10_0000, 8, PF_R | PF_W)
            },
        ],
        &[0; 8],
    );
    let elf = ElfFile::parse(&data).unwrap();
    assert_eq!(elf.image_end(0, 0x4000), Ok(0x10_8000));
    assert_eq!(elf.image_end(0x7000_0000, 0x4000), Ok(0x7010_8000));
    assert_eq!(elf.image_end(0, 1), Ok(0x10_5001));
    assert_eq!(
        elf.image_end(u64::MAX - 0x1000, 0x4000),
        Err(ElfError::AddressOverflow)
    );
    assert_eq!(
        elf.image_end(u64::MAX - 0x10_8000, 0x4000),
        Err(ElfError::AddressOverflow),
        "the rounding passes the top"
    );
    assert_eq!(elf.image_end(0, 0x3000), Err(ElfError::BadAlignment));
    assert_eq!(elf.image_end(0, 0), Err(ElfError::BadAlignment));
}

#[test]
fn a_truncated_file_never_panics() {
    // Every prefix of every kind of file: parsed, and every answer asked.
    let files = [
        static_exec(),
        dynamic_exec(b"/lib64/ld-linux-x86-64.so.2\0"),
        file(ET_DYN, 0, 0, &[], &[]),
    ];
    for f in &files {
        for len in 0..=f.len() {
            if let Ok(elf) = ElfFile::parse(&f[..len]) {
                let _ = elf.loadable_segments().map(Iterator::count);
                let _ = (
                    elf.interp_path(),
                    elf.detect_linux_abi(),
                    elf.phdr_table_bytes(),
                );
                let _ = elf.image_end(u64::MAX / 2, 0x4000);
            }
        }
    }
}
