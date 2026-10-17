//! `misc/chattr.c`: change a file's attributes.

use cstdio::Output;

use crate::e2p;
use crate::flags::{EXT2_DIRSYNC_FL, get_flag, print_flags};
use crate::{E2FSPROGS_DATE, E2FSPROGS_VERSION};

/// The run's state: upstream's globals.
#[derive(Default)]
struct State {
    add: bool,
    rem: bool,
    set: bool,
    set_version: bool,
    version: u64,
    set_project: bool,
    project: u64,
    recursive: bool,
    verbose: bool,
    silent: bool,
    af: u64,
    rf: u64,
    sf: u64,
}

/// The program: its name, its state, its two streams.
struct Chattr {
    program_name: Vec<u8>,
    st: State,
    out: Output,
    err: Output,
}

/// `decode_arg`'s answer: an option argument used, or the first operand.
enum Arg {
    Option,
    Operand,
}

impl Chattr {
    /// `exit (code)`: standard output flushed first.
    fn exit(&mut self, code: i32) -> ! {
        self.out.flush();
        std::process::exit(code)
    }

    /// `usage ()`.
    fn usage(&mut self) -> ! {
        let mut text = b"Usage: ".to_vec();
        text.extend_from_slice(&self.program_name);
        text.extend_from_slice(
            b" [-RVf] [-+=aAcCdDeijPsStTuFx] [-p project] [-v version] files...\n",
        );
        self.err.write(&text);
        self.exit(1)
    }

    /// `strtol (arg, &tmp, 0)` for `-p` and `-v`: the value as an
    /// `unsigned long` takes a `long`, or the usage after `bad what - arg`
    /// when anything follows the number.
    fn number(&mut self, what: &[u8], arg: &[u8]) -> u64 {
        let (value, used) = cstrtol::strtol(arg, 0);
        if arg.get(used).is_some() {
            let mut msg = b"bad ".to_vec();
            msg.extend_from_slice(what);
            msg.extend_from_slice(b" - ");
            msg.extend_from_slice(arg);
            msg.push(b'\n');
            e2p::com_err(&mut self.err, &self.program_name, 0, &msg);
            self.usage();
        }
        value.cast_unsigned()
    }

    /// `decode_arg (&i, argc, argv)`.
    fn decode_arg(&mut self, i: &mut usize, argv: &[Vec<u8>]) -> Arg {
        let arg = argv.get(*i).map(Vec::as_slice).unwrap_or_default();
        let rest = arg.get(1..).unwrap_or_default();
        match arg.first() {
            Some(b'-') => {
                for &c in rest {
                    match c {
                        b'R' => self.st.recursive = true,
                        b'V' => self.st.verbose = true,
                        b'f' => self.st.silent = true,
                        b'p' | b'v' => {
                            *i = i.saturating_add(1);
                            let Some(value) = argv.get(*i) else {
                                self.usage();
                            };
                            if c == b'p' {
                                self.st.project = self.number(b"project", value);
                                self.st.set_project = true;
                            } else {
                                self.st.version = self.number(b"version", value);
                                self.st.set_version = true;
                            }
                        }
                        _ => {
                            let fl = get_flag(c);
                            if fl == 0 {
                                self.usage();
                            }
                            self.st.rf |= fl;
                            self.st.rem = true;
                        }
                    }
                }
                Arg::Option
            }
            Some(b'+') => {
                self.st.add = true;
                for &c in rest {
                    let fl = get_flag(c);
                    if fl == 0 {
                        self.usage();
                    }
                    self.st.af |= fl;
                }
                Arg::Option
            }
            Some(b'=') => {
                self.st.set = true;
                for &c in rest {
                    let fl = get_flag(c);
                    if fl == 0 {
                        self.usage();
                    }
                    self.st.sf |= fl;
                }
                Arg::Option
            }
            _ => Arg::Operand,
        }
    }

    /// `com_err (program_name, code, what name)`, unless `-f`.
    fn complain(&mut self, code: i32, what: &[u8], name: &[u8]) {
        if self.st.silent {
            return;
        }
        let mut msg = what.to_vec();
        msg.extend_from_slice(name);
        e2p::com_err(&mut self.err, &self.program_name, code, &msg);
    }

    /// `printf ("Flags of %s set as "); print_flags; printf ("\n")`.
    fn say_flags(&mut self, name: &[u8], flags: u64) {
        self.out.write(b"Flags of ");
        self.out.write(name);
        self.out.write(b" set as ");
        print_flags(&mut self.out, flags, false);
        self.out.write(b"\n");
    }

    /// `change_attributes (name)`: 0, or non-zero for a failure.
    fn change_attributes(&mut self, name: &[u8]) -> i32 {
        let md = match e2p::lstat(name) {
            Ok(md) => md,
            Err(e) => {
                self.complain(e, b"while trying to stat ", name);
                return -1;
            }
        };
        let mut flags = match e2p::fgetflags(name) {
            Ok(f) => f,
            Err(e) => {
                self.complain(e, b"while reading flags on ", name);
                return -1;
            }
        };
        if self.st.set {
            if self.st.verbose {
                self.say_flags(name, self.st.sf);
            }
            if let Err(e) = e2p::fsetflags(name, self.st.sf) {
                // `perror`, which `-f` does not silence, and the run goes
                // on to the version and the project.
                cstdio::perror(&mut self.err, name, e);
            }
        } else {
            if self.st.rem {
                flags &= !self.st.rf;
            }
            if self.st.add {
                flags |= self.st.af;
            }
            if self.st.verbose {
                self.say_flags(name, flags);
            }
            if !md.is_dir() {
                flags &= !EXT2_DIRSYNC_FL;
            }
            if let Err(e) = e2p::fsetflags(name, flags) {
                self.complain(e, b"while setting flags on ", name);
                return -1;
            }
        }
        if self.st.set_version {
            if self.st.verbose {
                let line = format!(" set as {}\n", self.st.version);
                self.out.write(b"Version of ");
                self.out.write(name);
                self.out.write(line.as_bytes());
            }
            if let Err(e) = e2p::fsetversion(name, self.st.version) {
                self.complain(e, b"while setting version on ", name);
                return -1;
            }
        }
        if self.st.set_project {
            if self.st.verbose {
                let line = format!(" set as {}\n", self.st.project);
                self.out.write(b"Project of ");
                self.out.write(name);
                self.out.write(line.as_bytes());
            }
            if let Err(e) = e2p::fsetproject(name, self.st.project) {
                self.complain(e, b"while setting project on ", name);
                return -1;
            }
        }
        if md.is_dir() && self.st.recursive {
            return e2p::iterate_on_dir(name, &mut |dir, entry| self.chattr_dir_proc(dir, entry));
        }
        0
    }

    /// `chattr_dir_proc`: each entry but `.` and `..`, as `dir/entry`.
    fn chattr_dir_proc(&mut self, dir: &[u8], entry: &[u8]) -> i32 {
        if entry == b"." || entry == b".." {
            return 0;
        }
        let mut path = dir.to_vec();
        path.push(b'/');
        path.extend_from_slice(entry);
        self.change_attributes(&path)
    }
}

/// `main (argc, argv)`: the exit status.
pub fn main(argv: &[Vec<u8>]) -> i32 {
    let mut c = Chattr {
        program_name: argv.first().cloned().unwrap_or_else(|| b"chattr".to_vec()),
        st: State::default(),
        out: Output::stdout(),
        err: Output::stderr(),
    };
    let mut i = 1usize;
    let mut end_arg = false;
    while i < argv.len() && !end_arg {
        if argv.get(i).is_some_and(|a| a == b"--") {
            i = i.saturating_add(1);
            end_arg = true;
        } else {
            match c.decode_arg(&mut i, argv) {
                Arg::Operand => end_arg = true,
                Arg::Option => i = i.saturating_add(1),
            }
        }
    }
    if i >= argv.len() {
        c.usage();
    }
    if c.st.set && (c.st.add || c.st.rem) {
        c.err.write(b"= is incompatible with - and +\n");
        c.exit(1);
    }
    if c.st.rf & c.st.af != 0 {
        c.err.write(b"Can't both set and unset same flag.\n");
        c.exit(1);
    }
    if !(c.st.add || c.st.rem || c.st.set || c.st.set_version || c.st.set_project) {
        c.err.write(b"Must use '-v', =, - or +\n");
        c.exit(1);
    }
    if c.st.verbose {
        c.err
            .write(format!("chattr {E2FSPROGS_VERSION} ({E2FSPROGS_DATE})\n").as_bytes());
    }
    let mut retval = 0;
    for name in argv.iter().skip(i) {
        if c.change_attributes(name) != 0 {
            retval = 1;
        }
    }
    c.out.flush();
    retval
}
