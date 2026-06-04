//! id — print real and effective user and group IDs.
//!
//! Usage: id [-u] [-g] [-n]
//!   (no flags)  print full uid/gid info
//!   -u          print only effective UID
//!   -g          print only effective GID
//!   -n          print name instead of number (not yet supported — prints number)

use std::env;

unsafe extern "C" {
    fn getuid() -> u32;
    fn geteuid() -> u32;
    fn getgid() -> u32;
    fn getegid() -> u32;
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut show_uid_only = false;
    let mut show_gid_only = false;

    for arg in &args {
        if let Some(flags) = arg.strip_prefix('-') {
            for c in flags.chars() {
                match c {
                    'u' => show_uid_only = true,
                    'g' => show_gid_only = true,
                    'n' => {} // name mode — ignored (no name db yet)
                    _ => {
                        eprintln!("id: unknown option: -{c}");
                    }
                }
            }
        }
    }

    // SAFETY: these are simple POSIX getters with no pointer arguments.
    let uid = unsafe { getuid() };
    let euid = unsafe { geteuid() };
    let gid = unsafe { getgid() };
    let egid = unsafe { getegid() };

    if show_uid_only {
        println!("{euid}");
    } else if show_gid_only {
        println!("{egid}");
    } else {
        // Full output: uid=1000 gid=1000 euid=1000 egid=1000
        print!("uid={uid}");
        if euid != uid {
            print!(" euid={euid}");
        }
        print!(" gid={gid}");
        if egid != gid {
            print!(" egid={egid}");
        }
        println!();
    }
}
