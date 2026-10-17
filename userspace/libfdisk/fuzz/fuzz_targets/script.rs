//! An sfdisk script a stranger wrote -- `sfdisk < file`, `fdisk`'s `I`,
//! `cfdisk`'s `L`: parsing it, making the table it describes on a blank
//! two-mebibyte disk, in memory, and dumping that table back as a script
//! must end, never panic or hang.
#![no_main]
use std::cell::RefCell;
use std::rc::Rc;

use libfdisk::Context;
use libfdisk::script::{FileInput, Script};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut cxt = Context::new();
    let mut dp = Script::new();
    if dp.read_file(&mut cxt, &mut FileInput::new(data)) != 0 {
        return;
    }
    let path = std::env::temp_dir().join(format!("libfdisk-fuzz-script-{}.img", std::process::id()));
    let Ok(f) = std::fs::File::create(&path) else {
        return;
    };
    if f.set_len(2 * 1024 * 1024).is_err() {
        return;
    }
    drop(f);
    let Ok(f) = std::fs::File::open(&path) else {
        return;
    };
    let name = path.as_os_str().as_encoded_bytes().to_vec();
    if cxt.assign_device_by_file(Rc::new(f), &name, true) != 0 {
        return;
    }
    if cxt.apply_script(&Rc::new(RefCell::new(dp))) != 0 {
        return;
    }
    let mut back = Script::new();
    if back.read_context(&mut cxt) == 0 {
        let mut out = Vec::new();
        let _ = back.write_file(&cxt, &mut out);
    }
});
