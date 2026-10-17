//! `misc/lsattr.c`: list a file's attributes.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;

use cstdio::Output;
use getoptlong::{Opt, Program};

use crate::e2p;
use crate::{E2FSPROGS_DATE, E2FSPROGS_VERSION};

/// The run's options: upstream's globals.
#[derive(Default)]
struct Options {
    all: bool,
    dirs_opt: bool,
    long: bool,
    recursive: bool,
    verbose: bool,
    generation_opt: bool,
    project_opt: bool,
}

/// The program: its name, its options, its two streams.
struct Lsattr {
    program_name: Vec<u8>,
    opt: Options,
    out: Output,
    err: Output,
}

impl Lsattr {
    /// `exit (code)`: standard output flushed first.
    fn exit(&mut self, code: i32) -> ! {
        self.out.flush();
        std::process::exit(code)
    }

    /// `usage ()`.
    fn usage(&mut self) -> ! {
        let mut text = b"Usage: ".to_vec();
        text.extend_from_slice(&self.program_name);
        text.extend_from_slice(b" [-RVadlpv] [files...]\n");
        self.err.write(&text);
        self.exit(1)
    }

    /// `com_err (program_name, code, what name)`.
    fn complain(&mut self, code: i32, what: &[u8], name: &[u8]) {
        let mut msg = what.to_vec();
        msg.extend_from_slice(name);
        e2p::com_err(&mut self.err, &self.program_name, code, &msg);
    }

    /// `list_attributes (name)`: 0, or -1 for a failure.
    fn list_attributes(&mut self, name: &[u8]) -> i32 {
        let flags = match e2p::fgetflags(name) {
            Ok(f) => f,
            Err(e) => {
                self.complain(e, b"While reading flags on ", name);
                return -1;
            }
        };
        if self.opt.project_opt {
            match e2p::fgetproject(name) {
                Ok(project) => self.out.write(format!("{project:5} ").as_bytes()),
                Err(e) => {
                    self.complain(e, b"While reading project on ", name);
                    return -1;
                }
            }
        }
        if self.opt.generation_opt {
            match e2p::fgetversion(name) {
                Ok(generation) => self.out.write(format!("{generation:<10} ").as_bytes()),
                Err(e) => {
                    self.complain(e, b"While reading version on ", name);
                    return -1;
                }
            }
        }
        if self.opt.long {
            // `%-28s`: padded to 28 bytes, as printf counts them.
            let mut padded = name.to_vec();
            while padded.len() < 28 {
                padded.push(b' ');
            }
            padded.push(b' ');
            self.out.write(&padded);
            crate::flags::print_flags(&mut self.out, flags, true);
            self.out.write(b"\n");
        } else {
            crate::flags::print_flags(&mut self.out, flags, false);
            self.out.write(b" ");
            self.out.write(name);
            self.out.write(b"\n");
        }
        0
    }

    /// `lsattr_args (name)`: a directory's entries, unless `-d`; else the
    /// file itself.
    fn lsattr_args(&mut self, name: &[u8]) -> i32 {
        match e2p::lstat(name) {
            Err(e) => {
                self.complain(e, b"while trying to stat ", name);
                -1
            }
            Ok(md) if md.is_dir() && !self.opt.dirs_opt => {
                e2p::iterate_on_dir(name, &mut |dir, entry| self.lsattr_dir_proc(dir, entry))
            }
            Ok(_) => self.list_attributes(name),
        }
    }

    /// `lsattr_dir_proc`: an entry -- hidden ones only under `-a` -- and,
    /// under `-R`, the directory below it.
    fn lsattr_dir_proc(&mut self, dir: &[u8], entry: &[u8]) -> i32 {
        let mut path = dir.to_vec();
        if dir.last() != Some(&b'/') {
            path.push(b'/');
        }
        path.extend_from_slice(entry);
        match e2p::lstat(&path) {
            Err(e) => cstdio::perror(&mut self.err, &path, e),
            Ok(md) => {
                if entry.first() != Some(&b'.') || self.opt.all {
                    self.list_attributes(&path);
                    if md.is_dir() && self.opt.recursive && entry != b"." && entry != b".." {
                        self.out.write(b"\n");
                        self.out.write(&path);
                        self.out.write(b":\n");
                        e2p::iterate_on_dir(&path, &mut |d, e| self.lsattr_dir_proc(d, e));
                        self.out.write(b"\n");
                    }
                }
            }
        }
        0
    }
}

/// `main (argc, argv)`: the exit status.
pub fn main(argv: &[Vec<u8>]) -> i32 {
    let mut l = Lsattr {
        // `static const char *program_name = "lsattr"`, kept when argv is
        // empty -- whose `usage ()` then names it.
        program_name: argv.first().cloned().unwrap_or_else(|| b"lsattr".to_vec()),
        opt: Options::default(),
        out: Output::stdout(),
        err: Output::stderr(),
    };
    if argv.is_empty() {
        l.usage();
    }
    const LSATTR: Program = Program::new("lsattr", 1);
    let words: Vec<OsString> = argv
        .iter()
        .skip(1)
        .map(|w| OsString::from_vec(w.clone()))
        .collect();
    let mut operands: Vec<Vec<u8>> = Vec::new();
    for item in LSATTR.parse(&words, "RVadlvp", &[]).short_only(true) {
        match item {
            Err(e) => {
                // getopt's complaint names the program as `argv[0]` has it.
                let mut text = l.program_name.clone();
                text.extend_from_slice(b": ");
                text.extend_from_slice(e.sentence.as_bytes());
                text.push(b'\n');
                l.err.write(&text);
                l.usage();
            }
            Ok(Opt::Operand(w)) => operands.push(w.clone().into_vec()),
            Ok(Opt::Short(c, _)) => match c {
                b'R' => l.opt.recursive = true,
                b'V' => l.opt.verbose = true,
                b'a' => l.opt.all = true,
                b'd' => l.opt.dirs_opt = true,
                b'l' => l.opt.long = true,
                b'v' => l.opt.generation_opt = true,
                b'p' => l.opt.project_opt = true,
                _ => l.usage(),
            },
            Ok(Opt::Long(..)) => l.usage(),
        }
    }
    if l.opt.verbose {
        l.err
            .write(format!("lsattr {E2FSPROGS_VERSION} ({E2FSPROGS_DATE})\n").as_bytes());
    }
    let mut retval = 0;
    if operands.is_empty() {
        if l.lsattr_args(b".") == -1 {
            retval = 1;
        }
    } else {
        for name in &operands {
            if l.lsattr_args(name) != 0 {
                retval = 1;
            }
        }
    }
    l.out.flush();
    retval
}
