//! `bc` -- an arbitrary-precision calculator language: GNU bc 1.07.1,
//! ported. The scanner, parser, code generator and machine are the `gnubc`
//! library; this is upstream's `main.c`: the options (`BC_ENV_ARGS`'s first,
//! then the command line's), the environment, and the interrupt handler.

// Before `main`, so that `stdfd::restore` still sees the descriptors `bc` was
// given: a closed standard output is a write error, as it is upstream -- not
// the `/dev/null` Rust's runtime would put there.
coreutils::guard_std_fds!();

#[cfg(unix)]
fn main() {
    // Before anything else: a standard descriptor bc was started without is
    // closed again, and SIGPIPE given back the disposition it came with.
    coreutils::stdfd::restore();
    run::main(&std::env::args_os().collect::<Vec<_>>());
}

#[cfg(not(unix))]
fn main() {
    eprintln!("bc: runs only on a Unix host");
    std::process::exit(1);
}

#[cfg(unix)]
mod run {
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    use getoptlong::{Opt, Program, Takes};
    use gnubc::Bc;

    const SHORTOPTS: &str = "chilqswv";
    const LONGOPTS: &[(&str, Takes)] = &[
        ("compile", Takes::Nothing),
        ("help", Takes::Nothing),
        ("interactive", Takes::Nothing),
        ("mathlib", Takes::Nothing),
        ("quiet", Takes::Nothing),
        ("standard", Takes::Nothing),
        ("version", Takes::Nothing),
        ("warn", Takes::Nothing),
    ];

    /// `usage (progname)`'s text after its first line.
    const USAGE: &str = "  -h  --help         print this usage and exit\n\
\x20 -i  --interactive  force interactive mode\n\
\x20 -l  --mathlib      use the predefined math routines\n\
\x20 -q  --quiet        don't print initial banner\n\
\x20 -s  --standard     non-standard bc constructs are errors\n\
\x20 -w  --warn         warn about non-standard bc constructs\n\
\x20 -v  --version      print version information and exit\n";

    /// `usage (progname)`: to standard output.
    fn usage(bc: &mut Bc, progname: &[u8]) {
        let mut text = b"usage: ".to_vec();
        text.extend_from_slice(progname);
        text.extend_from_slice(b" [options] [file ...]\n");
        text.extend_from_slice(USAGE.as_bytes());
        bc.out.write(&text);
    }

    /// `parse_args (argc, argv)`: the options of `argv` set, its operands
    /// added to the files to read. Help, the version, and a bad option end
    /// the program.
    fn parse_args(bc: &mut Bc, argv: &[OsString]) {
        const BC: Program = Program::new("bc", 1);
        let progname = argv
            .first()
            .map(|a| a.as_bytes().to_vec())
            .unwrap_or_default();
        let words = argv.get(1..).unwrap_or_default();
        let mut operands = Vec::new();
        for item in BC.parse(words, SHORTOPTS, LONGOPTS) {
            let code = match item {
                Err(e) => {
                    // getopt names the program as `argv[0]` has it.
                    let mut text = progname.clone();
                    text.extend_from_slice(b": ");
                    text.extend_from_slice(e.sentence.as_bytes());
                    text.push(b'\n');
                    bc.err.write(&text);
                    usage(bc, &progname);
                    bc.exit(1);
                }
                Ok(Opt::Operand(w)) => {
                    operands.push(w.as_bytes().to_vec());
                    continue;
                }
                Ok(Opt::Short(c, _)) => c,
                // Each long option is its short one by its first letter --
                // `--compile`, which upstream's table gives as a flag, among
                // them -- but for `--mathlib`, which is `-l`.
                Ok(Opt::Long(name, _)) => match name {
                    "mathlib" => b'l',
                    _ => name.as_bytes().first().copied().unwrap_or(0),
                },
            };
            match code {
                b'c' => bc.compile_only = true,
                b'h' => {
                    usage(bc, &progname);
                    bc.exit(0);
                }
                b'i' => bc.interactive = true,
                b'l' => bc.use_math = true,
                b'q' => bc.quiet = true,
                b's' => bc.std_only = true,
                b'v' => {
                    bc.show_bc_version();
                    bc.exit(0);
                }
                _ => bc.warn_not_std = true,
            }
        }
        bc.file_names.extend(operands);
    }

    /// `BC_ENV_ARGS`'s words, split at spaces (only), after an `argv[0]`
    /// of `BC_ENV_ARGS`.
    fn env_argv(value: &[u8]) -> Vec<OsString> {
        let mut argv = vec![OsString::from("BC_ENV_ARGS")];
        argv.extend(
            value
                .split(|&b| b == b' ')
                .filter(|w| !w.is_empty())
                .map(|w| OsString::from_vec(w.to_vec())),
        );
        argv
    }

    /// `main`: the program run; it does not return.
    pub fn main(args: &[OsString]) -> ! {
        let mut bc = Bc::default();
        if libcall::fd::is_terminal(0) && libcall::fd::is_terminal(1) {
            bc.interactive = true;
        }
        if let Some(value) = std::env::var_os("BC_ENV_ARGS") {
            parse_args(&mut bc, &env_argv(value.as_bytes()));
        }
        parse_args(&mut bc, args);
        if std::env::var_os("POSIXLY_CORRECT").is_some() {
            bc.std_only = true;
        }
        bc.line_size = line_size(
            std::env::var_os("BC_LINE_LENGTH")
                .as_deref()
                .map(OsStrExt::as_bytes),
        );
        if bc.interactive {
            gnubc::on_interrupt(gnubc::use_quit);
        }
        bc.run()
    }

    /// The line length `BC_LINE_LENGTH`'s value (`None` when it is not set)
    /// asks for: its `atoi`, unless that is below 3 and not 0, which is 70,
    /// the length when it is not set. 0 means no breaks.
    fn line_size(value: Option<&[u8]>) -> i32 {
        match value {
            Some(value) => {
                // `atoi`: strtol's value, cut to an int.
                let size = cstrtol::low_i32(cstrtol::strtol(value, 10).0);
                if size < 3 && size != 0 { 70 } else { size }
            }
            None => 70,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn argv(words: &[&str]) -> Vec<OsString> {
            words.iter().map(OsString::from).collect()
        }

        #[test]
        fn each_option_sets_its_own_flag() {
            let mut bc = Bc::default();
            parse_args(&mut bc, &argv(&["bc", "-i", "-l", "-c", "-q", "-s", "f"]));
            assert!(bc.interactive && bc.use_math && bc.compile_only);
            assert!(bc.quiet && bc.std_only);
            assert!(!bc.warn_not_std, "only -w warns");
            assert_eq!(bc.file_names, [b"f".to_vec()]);

            let mut bc = Bc::default();
            parse_args(
                &mut bc,
                &argv(&["bc", "--mathlib", "--compile", "--interactive", "--warn"]),
            );
            assert!(bc.use_math && bc.compile_only && bc.interactive);
            assert!(bc.warn_not_std);
            assert!(!bc.quiet && !bc.std_only);
        }

        #[test]
        fn usage_names_the_program_as_it_was_called() {
            let mut bc = Bc::default();
            bc.out = cstdio::Output::new(cstdio::Sink::Memory(Vec::new()), false, false);
            usage(&mut bc, b"./bc");
            let text = bc.out.take_written();
            assert!(text.starts_with(b"usage: ./bc [options] [file ...]\n  -h  --help "));
            assert!(text.ends_with(b"print version information and exit\n"));
        }

        #[test]
        fn a_line_length_below_three_but_zero_is_the_default() {
            assert_eq!(line_size(None), 70);
            assert_eq!(line_size(Some(b"3")), 3);
            assert_eq!(line_size(Some(b"2")), 70);
            assert_eq!(line_size(Some(b"-5")), 70);
            assert_eq!(line_size(Some(b"0")), 0, "no breaks");
            assert_eq!(line_size(Some(b"100x")), 100, "atoi stops at the x");
            assert_eq!(line_size(Some(b"x")), 0, "atoi of no number");
        }

        #[test]
        fn environment_arguments_are_split_at_spaces_only() {
            let argv = env_argv(b" -q  -l\tx  f ");
            let words: Vec<&[u8]> = argv.iter().map(|a| a.as_bytes()).collect();
            assert_eq!(words, [&b"BC_ENV_ARGS"[..], b"-q", b"-l\tx", b"f"]);
        }
    }
}
