//! A disk a stranger made: whatever its first two mebibytes hold -- an MBR
//! and its chain of extended boot records, a GPT and its entries, an SGI or
//! a Sun label, a BSD label nested in an MBR partition -- probing it,
//! listing it, verifying it, reading its partitions and free space, showing
//! every field of each, and adding a partition with every default must all
//! end, never panic or hang. That is what `fdisk -l`, `fdisk`'s `n` and
//! `cfdisk` do to a disk plugged in from elsewhere.
#![no_main]
use std::io::Write;
use std::rc::Rc;

use libfdisk::{Ask, AskType, Context, Partition, Table, fieldtype};
use libfuzzer_sys::fuzz_target;

/// The image's size: 4096 sectors of 512 bytes.
const SIZE: u64 = 2 * 1024 * 1024;

/// The dialogs answered as a user pressing Enter would: every default, and
/// "no".
fn defaults(_: &mut Context, ask: &mut Ask) -> i32 {
    match ask.get_type() {
        AskType::Number | AskType::Offset => {
            let d = ask.number_get_default();
            ask.number_set_result(d)
        }
        AskType::Menu => {
            let d = ask.menu_get_default();
            ask.menu_set_result(d)
        }
        AskType::YesNo => {
            ask.yesno_set_result(0);
            0
        }
        AskType::String => {
            ask.string_set_result(Vec::new());
            0
        }
        _ => 0,
    }
}

/// Everything a listing and `n` do to the label `cxt` found.
fn exercise(cxt: &mut Context) {
    let _ = cxt.list_disklabel();
    let _ = cxt.verify_disklabel();
    let mut tb = Table::new();
    let _ = cxt.get_partitions(&mut tb);
    let _ = cxt.get_freespaces(&mut tb);
    for pa in tb.iter_mut() {
        for id in 1..fieldtype::NFIELDS {
            let _ = cxt.partition_to_string(pa, id);
        }
    }
    let mut pa = Partition::new();
    pa.set_start_follow_default(true);
    pa.set_end_follow_default(true);
    pa.set_partno_follow_default(true);
    let _ = cxt.add_partition(Some(&mut pa), None);
}

fuzz_target!(|data: &[u8]| {
    let path = std::env::temp_dir().join(format!("libfdisk-fuzz-{}.img", std::process::id()));
    let Ok(mut f) = std::fs::File::create(&path) else {
        return;
    };
    let n = data.len().min(usize::try_from(SIZE).unwrap_or(0));
    if f.write_all(&data[..n]).is_err() || f.set_len(SIZE).is_err() {
        return;
    }
    drop(f);
    let Ok(f) = std::fs::File::open(&path) else {
        return;
    };
    let name = path.as_os_str().as_encoded_bytes().to_vec();
    let mut cxt = Context::new();
    cxt.set_ask(Some(Rc::new(defaults)));
    if cxt.assign_device_by_file(Rc::new(f), &name, true) != 0 {
        return;
    }
    let dos = cxt.is_labeltype(libfdisk::LabelType::Dos);
    exercise(&mut cxt);
    // `fdisk`'s `b`: a BSD label inside the DOS one.
    if dos {
        let mut nested = Context::new_nested(cxt, Some(b"bsd"));
        if nested.has_label() {
            exercise(&mut nested);
        }
    }
});
