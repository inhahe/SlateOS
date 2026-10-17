//! A program file, as the kernel reads one before running it: parsed, every
//! program header and its bytes read, the loadable segments walked, the
//! interpreter path and the ABI markers looked for, and the image's end
//! computed at load biases up to the top of the address space.
//!
//! Beyond not panicking, the parser's promises are checked, since the loader
//! maps whatever it accepts: every loadable segment lies within the file and
//! below `USER_SPACE_END` with its memory size at least its file size, and the
//! image's end is aligned and covers every segment.
#![no_main]

use elfparse::{ElfError, ElfFile, USER_SPACE_END, is_linux_interp, notes_contain};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(elf) = ElfFile::parse(data) else {
        return;
    };
    let _ = (elf.entry_point(), elf.is_pie(), elf.phdr_table_bytes());
    for i in 0..=elf.program_header_count() {
        let Some(ph) = elf.program_header(i) else {
            continue;
        };
        if let Some(bytes) = elf.raw_segment_bytes(&ph) {
            let _ = is_linux_interp(bytes);
            let _ = notes_contain(bytes, ph.p_align, b"GNU\0", 1);
        }
    }
    let _ = (
        elf.interp_path(),
        elf.detect_linux_abi(),
        elf.has_slateos_marker(),
    );

    let Ok(segments) = elf.loadable_segments() else {
        return;
    };
    let mut ends = Vec::new();
    for s in segments {
        assert!(
            s.mem_size >= s.file_size,
            "memory smaller than the file's bytes"
        );
        let file_end = s
            .file_offset
            .checked_add(s.file_size)
            .expect("file range wraps");
        assert!(file_end <= data.len() as u64, "segment bytes past the file");
        let end = s
            .vaddr
            .checked_add(s.mem_size)
            .expect("address range wraps");
        assert!(end <= USER_SPACE_END, "segment past user space");
        ends.push(end);
    }
    for bias in [
        0,
        0x5555_5555_4000,
        0x7fff_ffff_0000,
        u64::MAX - 0xffff,
        u64::MAX,
    ] {
        for align in [1u64, 0x1000, 0x4000] {
            match elf.image_end(bias, align) {
                Ok(top) => {
                    assert_eq!(top % align, 0, "image end not aligned");
                    for &end in &ends {
                        assert!(end + bias <= top, "image end below a segment");
                    }
                }
                Err(ElfError::AddressOverflow) => {}
                Err(e) => panic!("image_end refused an accepted image: {e}"),
            }
        }
    }
});
