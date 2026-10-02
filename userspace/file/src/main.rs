//! `file` -- determine the type of a file. A port of file 5.45 (Ian F.
//! Darwin, Christos Zoulas and others), libmagic and its magic database
//! included.
//!
//! This is `src/file.c`: the command line. Everything it reports comes from
//! libmagic's modules, each a port of the libmagic source file of the same
//! name -- `apprentice` reads the magic database, `softmagic` runs its rules,
//! `funcs::file_buffer` decides which tests run in which order, `encoding` and
//! `ascmagic` describe text. They are ports function by function, measured
//! against file 5.45 by `scripts/file-diff.sh`, and keep upstream's behaviour
//! where it is odd, because what `file` prints is read by scripts.
//!
//! The library is `userspace/libmagic`. The database is file 5.45's own
//! (`magic/`, vendored by `scripts/file-magic-vendor.py`), compiled by
//! `build.rs` with that library exactly as `file -C` compiles it, and carried
//! inside the program ([`database`]): used in place, as upstream uses its
//! installed `magic.mgc`, when nothing is installed at the default path.
//!
//! Where this deliberately differs from upstream:
//!
//! | Upstream | Here | Why |
//! |---|---|---|
//! | the database is `magic.mgc`, installed | the same `magic.mgc`, built into the program | the image need not carry a file only this program reads |
//! | `-S` turns off the seccomp sandbox | accepted, and does nothing | SlateOS has no seccomp; there is no sandbox to turn off |

// The workspace's lint policy, less two of its defensive lints, as for
// libmagic: this is file.c's arithmetic and indexing -- counts of arguments
// and errors, columns of a name, offsets into the option documentation's
// fixed strings -- each bounded where it is made.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]

mod database {
    //! The database this program carries: file 5.45's magic (`magic/`),
    //! compiled by the build script exactly as `file -C` compiles it. It is
    //! used in place of an installed `magic.mgc` when nothing is installed at
    //! the default path -- in place meaning as it lies, which is why it is
    //! aligned for a rule (`libmagic::magic::Magic`, 8 bytes).

    /// Bytes at the alignment of `A`.
    #[repr(C)]
    struct AlignedAs<A, B: ?Sized> {
        _align: [A; 0],
        bytes: B,
    }

    static DATABASE: &AlignedAs<u64, [u8]> = &AlignedAs {
        _align: [],
        bytes: *include_bytes!(concat!(env!("OUT_DIR"), "/magic.mgc")),
    };

    /// The compiled database.
    pub fn builtin() -> &'static [u8] {
        &DATABASE.bytes
    }
}

use std::ffi::OsString;
use std::io::{BufRead, Write};

use getoptlong::{Opt, Program, Takes};

use libmagic::apprentice::{self, Action, os_bytes};
use libmagic::funcs::{self, Ms, decode_utf8, iswprint};
use libmagic::magic::{self, *};
use libmagic::magicapi::{self, Param};
use libmagic::{cstd, out};

/// The option string: upstream's `OPTSTRING`.
const OPTSTRING: &str = "bcCde:Ef:F:hiklLm:nNpP:rsSvzZ0";

/// `file_opts.h`'s long options, in its order -- which is part of the
/// interface: an abbreviation is resolved against it.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
    ("magic-file", Takes::Required),
    ("uncompress", Takes::Nothing),
    ("uncompress-noreport", Takes::Nothing),
    ("brief", Takes::Nothing),
    ("checking-printout", Takes::Nothing),
    ("exclude", Takes::Required),
    ("exclude-quiet", Takes::Required),
    ("files-from", Takes::Required),
    ("separator", Takes::Required),
    ("mime", Takes::Nothing),
    ("apple", Takes::Nothing),
    ("extension", Takes::Nothing),
    ("mime-type", Takes::Nothing),
    ("mime-encoding", Takes::Nothing),
    ("keep-going", Takes::Nothing),
    ("list", Takes::Nothing),
    ("dereference", Takes::Nothing),
    ("no-dereference", Takes::Nothing),
    ("no-buffer", Takes::Nothing),
    ("no-pad", Takes::Nothing),
    ("print0", Takes::Nothing),
    ("preserve-date", Takes::Nothing),
    ("parameter", Takes::Required),
    ("raw", Takes::Nothing),
    ("special-files", Takes::Nothing),
    ("no-sandbox", Takes::Nothing),
    ("compile", Takes::Nothing),
    ("debug", Takes::Nothing),
];

/// The short option a long one is (`long_options[].val`), or the long-only
/// ones by name.
fn long_to_short(name: &str) -> Option<u8> {
    Some(match name {
        "version" => b'v',
        "magic-file" => b'm',
        "uncompress" => b'z',
        "uncompress-noreport" => b'Z',
        "brief" => b'b',
        "checking-printout" => b'c',
        "exclude" => b'e',
        "files-from" => b'f',
        "separator" => b'F',
        "mime" => b'i',
        "keep-going" => b'k',
        "list" => b'l',
        "dereference" => b'L',
        "no-dereference" => b'h',
        "no-buffer" => b'n',
        "no-pad" => b'N',
        "print0" => b'0',
        "preserve-date" => b'p',
        "parameter" => b'P',
        "raw" => b'r',
        "special-files" => b's',
        "no-sandbox" => b'S',
        "compile" => b'C',
        "debug" => b'd',
        _ => return None,
    })
}

/// `nv`: the tests `-e` can exclude.
const NV: &[(&str, u32)] = &[
    ("apptype", MAGIC_NO_CHECK_APPTYPE),
    ("ascii", MAGIC_NO_CHECK_ASCII),
    ("cdf", MAGIC_NO_CHECK_CDF),
    ("compress", MAGIC_NO_CHECK_COMPRESS),
    ("csv", MAGIC_NO_CHECK_CSV),
    ("elf", MAGIC_NO_CHECK_ELF),
    ("encoding", MAGIC_NO_CHECK_ENCODING),
    ("soft", MAGIC_NO_CHECK_SOFT),
    ("tar", MAGIC_NO_CHECK_TAR),
    ("json", MAGIC_NO_CHECK_JSON),
    ("simh", MAGIC_NO_CHECK_SIMH),
    // A synonym for `ascii`.
    ("text", MAGIC_NO_CHECK_TEXT),
    // Obsolete: accepted and ignored.
    ("tokens", MAGIC_NO_CHECK_TOKENS),
];

/// `pm`: the limits `-P` can set.
struct ParamDef {
    name: &'static str,
    def: usize,
    desc: &'static str,
    tag: Param,
}

const PM: &[ParamDef] = &[
    ParamDef { name: "bytes", def: funcs::FILE_BYTES_MAX, desc: "max bytes to look inside file", tag: Param::Bytes },
    ParamDef { name: "elf_notes", def: funcs::FILE_ELF_NOTES_MAX as usize, desc: "max ELF notes processed", tag: Param::ElfNotes },
    ParamDef { name: "elf_phnum", def: funcs::FILE_ELF_PHNUM_MAX as usize, desc: "max ELF prog sections processed", tag: Param::ElfPhnum },
    ParamDef { name: "elf_shnum", def: funcs::FILE_ELF_SHNUM_MAX as usize, desc: "max ELF sections processed", tag: Param::ElfShnum },
    ParamDef { name: "elf_shsize", def: funcs::FILE_ELF_SHSIZE_MAX, desc: "max ELF section size", tag: Param::ElfShsize },
    ParamDef { name: "encoding", def: funcs::FILE_ENCODING_MAX, desc: "max bytes to scan for encoding", tag: Param::Encoding },
    ParamDef { name: "indir", def: funcs::FILE_INDIR_MAX as usize, desc: "recursion limit for indirection", tag: Param::Indir },
    ParamDef { name: "name", def: funcs::FILE_NAME_MAX as usize, desc: "use limit for name/use magic", tag: Param::Name },
    ParamDef { name: "regex", def: magic::FILE_REGEX_MAX, desc: "length limit for REGEX searches", tag: Param::Regex },
];

/// The command line's state: upstream's file-scope globals.
struct Cli {
    progname: Vec<u8>,
    bflag: u32,
    nopad: bool,
    nobuffer: bool,
    nulsep: u32,
    separator: Vec<u8>,
    posixly: bool,
    /// `pm[].value` and `pm[].set`.
    params: Vec<Option<usize>>,
}

/// The `Usage:` text, with the program name thrice.
fn usage(cli: &Cli) -> ! {
    out::flush();
    let pn = &cli.progname;
    let mut w = b"Usage: ".to_vec();
    w.extend_from_slice(pn);
    w.extend_from_slice(b" [-bcCdEhikLlNnprsSvzZ0] [--apple] [--extension] [--mime-encoding]\n");
    w.extend_from_slice(b"            [--mime-type] [-e <testname>] [-F <separator>]  [-f <namefile>]\n");
    w.extend_from_slice(b"            [-m <magicfiles>] [-P <parameter=value>] [--exclude-quiet]\n");
    w.extend_from_slice(b"            <file> ...\n       ");
    w.extend_from_slice(pn);
    w.extend_from_slice(b" -C [-m <magicfiles>]\n       ");
    w.extend_from_slice(pn);
    w.extend_from_slice(b" [--help]\n");
    let _written = std::io::stderr().write_all(&w);
    std::process::exit(1);
}

/// `defprint`: ` (default)` after the option that is the default, and the
/// line's end.
fn defprint(cli: &Cli, def: u32, w: &mut Vec<u8>) {
    if def == 0 {
        return;
    }
    if (def & 1 != 0 && cli.posixly) || (def & 2 != 0 && !cli.posixly) {
        w.extend_from_slice(b" (default)");
    }
    w.push(b'\n');
}

/// `docprint`: an option's documentation, with `%e` and `%P` expanded.
fn docprint(cli: &Cli, opts: &str, def: u32, w: &mut Vec<u8>) {
    let ob = opts.as_bytes();
    let Some(p) = ob.iter().position(|&c| c == b'%') else {
        w.extend_from_slice(ob);
        defprint(cli, def, w);
        return;
    };
    let mut sp = p.saturating_sub(1);
    while sp > 0 && ob[sp] == b' ' {
        sp -= 1;
    }
    w.extend_from_slice(&ob[..p]);
    let pad = p - sp - 1;
    match ob.get(p + 1) {
        Some(b'e') => {
            let mut comma = false;
            for (i, (name, _)) in NV.iter().enumerate() {
                if comma {
                    w.extend_from_slice(b", ");
                }
                comma = true;
                w.extend_from_slice(name.as_bytes());
                if i != 0 && i % 5 == 0 && i != NV.len() - 1 {
                    w.extend_from_slice(b",\n");
                    w.extend(std::iter::repeat_n(b' ', pad));
                    comma = false;
                }
            }
        }
        Some(b'P') => {
            for (i, p) in PM.iter().enumerate() {
                w.extend_from_slice(format!("{:>9} {:>7} {}", p.name, p.def, p.desc).as_bytes());
                if i != PM.len() - 1 {
                    w.push(b'\n');
                    w.extend(std::iter::repeat_n(b' ', pad));
                }
            }
        }
        _ => {}
    }
    w.extend_from_slice(ob.get(p + 2..).unwrap_or_default());
}

/// `help`: `--help`, from `file_opts.h`.
fn help(cli: &Cli) -> ! {
    // (short, long, def, doc); a short of 0 is a long-only option.
    let opts: &[(u8, &str, u32, &str)] = &[
        (0, "help", 0, "                 display this help and exit\n"),
        (b'v', "version", 0, "              output version information and exit\n"),
        (b'm', "magic-file", 0, " LIST      use LIST as a colon-separated list of magic\n                               number files\n"),
        (b'z', "uncompress", 0, "           try to look inside compressed files\n"),
        (b'Z', "uncompress-noreport", 0, "  only print the contents of compressed files\n"),
        (b'b', "brief", 0, "                do not prepend filenames to output lines\n"),
        (b'c', "checking-printout", 0, "    print the parsed form of the magic file, use in\n                               conjunction with -m to debug a new magic file\n                               before installing it\n"),
        (b'e', "exclude", 0, " TEST         exclude TEST from the list of test to be\n                               performed for file. Valid tests are:\n                               %e\n"),
        (0, "exclude-quiet", 0, " TEST   like exclude, but ignore unknown tests\n"),
        (b'f', "files-from", 0, " FILE      read the filenames to be examined from FILE\n"),
        (b'F', "separator", 0, " STRING     use string as separator instead of `:'\n"),
        (b'i', "mime", 0, "                 output MIME type strings (--mime-type and\n                               --mime-encoding)\n"),
        (0, "apple", 0, "                output the Apple CREATOR/TYPE\n"),
        (0, "extension", 0, "            output a slash-separated list of extensions\n"),
        (0, "mime-type", 0, "            output the MIME type\n"),
        (0, "mime-encoding", 0, "        output the MIME encoding\n"),
        (b'k', "keep-going", 0, "           don't stop at the first match\n"),
        (b'l', "list", 0, "                 list magic strength\n"),
        (b'L', "dereference", 1, "          follow symlinks (default if POSIXLY_CORRECT is set)"),
        (b'h', "no-dereference", 2, "       don't follow symlinks (default if POSIXLY_CORRECT is not set)"),
        (b'n', "no-buffer", 0, "            do not buffer output\n"),
        (b'N', "no-pad", 0, "               do not pad output\n"),
        (b'0', "print0", 0, "               terminate filenames with ASCII NUL\n"),
        (b'p', "preserve-date", 0, "        preserve access times on files\n"),
        (b'P', "parameter", 0, "            set file engine parameter limits\n                               %P\n"),
        (b'r', "raw", 0, "                  don't translate unprintable chars to \\ooo\n"),
        (b's', "special-files", 0, "        treat special (block/char devices) files as\n                             ordinary ones\n"),
        (b'S', "no-sandbox", 0, "           disable system call sandboxing\n"),
        (b'C', "compile", 0, "              compile file specified by -m\n"),
        (b'd', "debug", 0, "                print debugging messages\n"),
    ];
    let mut w = b"Usage: file [OPTION...] [FILE...]\nDetermine type of FILEs.\n\n".to_vec();
    for &(short, long, def, doc) in opts {
        if short == 0 {
            w.extend_from_slice(format!("      --{long}").as_bytes());
        } else {
            w.extend_from_slice(format!("  -{}, --{long}", char::from(short)).as_bytes());
        }
        docprint(cli, doc, def, &mut w);
    }
    w.extend_from_slice(b"\nReport bugs to https://bugs.astron.com/\n");
    out::write(&w);
    out::flush();
    std::process::exit(0);
}

/// `file_warn`: `file: message`, and the error `errno` would hold.
fn file_warn(cli: &Cli, msg: &[u8], errno: Option<funcs::Errno>) {
    let mut w = cli.progname.clone();
    w.extend_from_slice(b": ");
    w.extend_from_slice(msg);
    if let Some(k) = errno {
        let e = k.to_error();
        w.extend_from_slice(format!(" ({})", errmsg::strerror(&e)).as_bytes());
    }
    w.push(b'\n');
    let _written = std::io::stderr().write_all(&w);
}

/// `file_errx`: `file: message`, and exit 1.
fn file_errx(cli: &Cli, msg: &[u8]) -> ! {
    out::flush();
    let mut w = cli.progname.clone();
    w.extend_from_slice(b": ");
    w.extend_from_slice(msg);
    w.push(b'\n');
    let _written = std::io::stderr().write_all(&w);
    std::process::exit(1);
}

/// C's `atoi`: white space, a sign, digits, as an `int`.
fn atoi(s: &[u8]) -> i32 {
    let mut i = 0usize;
    while s.get(i).copied().is_some_and(cstd::isspace) {
        i += 1;
    }
    let neg = s.get(i) == Some(&b'-');
    if matches!(s.get(i), Some(b'+' | b'-')) {
        i += 1;
    }
    let mut v: i32 = 0;
    while let Some(&d) = s.get(i).filter(|d| d.is_ascii_digit()) {
        v = v.wrapping_mul(10).wrapping_add(i32::from(d - b'0'));
        i += 1;
    }
    if neg { v.wrapping_neg() } else { v }
}

/// `setparam`: `-P name=value`, the name matched as a prefix of a parameter's.
fn setparam(cli: &mut Cli, p: &[u8]) {
    if let Some(eq) = p.iter().position(|&c| c == b'=') {
        let key = &p[..eq];
        for (i, pm) in PM.iter().enumerate() {
            // `strncmp(p, pm[i].name, s - p)`: the given name, as a prefix.
            let name = pm.name.as_bytes();
            let n = key.len();
            let a = |j: usize| key.get(j).copied().unwrap_or(0);
            let b = |j: usize| name.get(j).copied().unwrap_or(0);
            let mut equal = true;
            for j in 0..n {
                if a(j) != b(j) {
                    equal = false;
                    break;
                }
                if a(j) == 0 {
                    break;
                }
            }
            if !equal {
                continue;
            }
            #[allow(clippy::cast_sign_loss)]
            let v = i64::from(atoi(&p[eq + 1..])) as usize;
            cli.params[i] = Some(v);
            return;
        }
    }
    let mut msg = b"Unknown param ".to_vec();
    msg.extend_from_slice(p);
    file_errx(cli, &msg);
}

/// `applyparam`.
fn applyparam(cli: &Cli, ms: &mut Ms) {
    for (i, pm) in PM.iter().enumerate() {
        if let Some(v) = cli.params[i] {
            magicapi::magic_setparam(ms, pm.tag, v);
        }
    }
}

/// `load`: a magic set with its database, or `None` after saying why.
fn load(cli: &Cli, magicfile: Option<&[u8]>, flags: u32) -> Option<Ms> {
    let mut ms = magicapi::magic_open(flags);
    ms.utf8 = codeset_is_utf8();
    ms.builtin = Some(database::builtin());
    if magicapi::magic_load(&mut ms, magicfile) == -1 {
        let e = magicapi::magic_error(&ms).unwrap_or_default();
        file_warn(cli, &e, ms.errno);
        return None;
    }
    if let Some(e) = magicapi::magic_error(&ms) {
        file_warn(cli, &e, ms.errno);
    }
    Some(ms)
}

/// Whether `setlocale(LC_CTYPE, "")` leaves a UTF-8 codeset: the first of
/// `LC_ALL`, `LC_CTYPE` and `LANG` that is set and not empty names the
/// locale, and its codeset -- after the `.`, before any `@` -- is UTF-8 in any
/// spelling. (`smartcols::tty::codeset_is_utf8`'s rule.)
fn codeset_is_utf8() -> bool {
    let name = ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .filter_map(std::env::var_os)
        .find(|v| !v.is_empty());
    let Some(name) = name else {
        return false;
    };
    let name = os_bytes(&name);
    let Some(dot) = name.iter().position(|&b| b == b'.') else {
        return false;
    };
    let codeset: Vec<u8> = name[dot + 1..]
        .iter()
        .take_while(|&&b| b != b'@')
        .filter(|&&b| b != b'-')
        .map(u8::to_ascii_lowercase)
        .collect();
    codeset == b"utf8"
}

/// `file_mbswidth`: the columns a name takes as it is printed -- an escaped
/// byte four.
fn file_mbswidth(ms: &Ms, s: &[u8]) -> usize {
    let raw = ms.flags & MAGIC_RAW != 0;
    let s = cstd::cstr(s);
    let mut width = 0usize;
    let mut i = 0usize;
    while i < s.len() {
        let rest = &s[i..];
        let decoded = if ms.utf8 {
            decode_utf8(rest)
        } else {
            rest.first().filter(|c| c.is_ascii()).map(|&c| (char::from(c), 1))
        };
        match decoded {
            None => {
                width += 4;
                i += 1;
            }
            Some((c, n)) => {
                let w = charwidth::char_width(c).unwrap_or(0);
                width += if raw || iswprint(c) { if w > 0 { w } else { 1 } } else { 4 };
                i += n;
            }
        }
    }
    width
}

/// `file_octal`.
fn file_octal(w: &mut Vec<u8>, c: u8) {
    w.push(b'\\');
    w.push(((c >> 6) & 7) + b'0');
    w.push(((c >> 3) & 7) + b'0');
    w.push((c & 7) + b'0');
}

/// `fname_print`: a name with what the terminal should not be sent as
/// `\ooo` -- a character that is not printable as its low byte only, as
/// upstream does.
fn fname_print(ms: &Ms, name: &[u8], w: &mut Vec<u8>) {
    let s = cstd::cstr(name);
    let mut i = 0usize;
    while i < s.len() {
        let rest = &s[i..];
        let decoded = if ms.utf8 {
            decode_utf8(rest)
        } else {
            rest.first().filter(|c| c.is_ascii()).map(|&c| (char::from(c), 1))
        };
        match decoded {
            None => {
                file_octal(w, s[i]);
                i += 1;
            }
            Some((c, n)) => {
                if iswprint(c) {
                    w.extend_from_slice(&rest[..n]);
                } else {
                    #[allow(clippy::cast_possible_truncation)]
                    file_octal(w, u32::from(c) as u8);
                }
                i += n;
            }
        }
    }
}

/// `process`: one name, and its type. 1 when it failed.
fn process(cli: &Cli, ms: &mut Ms, inname: &[u8], wid: usize) -> i32 {
    let c = if cli.nulsep > 1 { 0u8 } else { b'\n' };
    let std_in = inname == b"-";
    let mut w = Vec::new();
    if wid > 0 && cli.bflag == 0 {
        let pname: &[u8] = if std_in { b"/dev/stdin" } else { inname };
        if ms.flags & MAGIC_RAW == 0 {
            fname_print(ms, pname, &mut w);
        } else {
            w.extend_from_slice(cstd::cstr(pname));
        }
        if cli.nulsep != 0 {
            w.push(0);
        }
        if cli.nulsep < 2 {
            w.extend_from_slice(&cli.separator);
            let pad = if cli.nopad { 0 } else { wid.saturating_sub(file_mbswidth(ms, inname)) };
            w.extend(std::iter::repeat_n(b' ', pad));
            w.push(b' ');
        }
    }
    out::write(&w);
    let typ = magicapi::magic_file(ms, if std_in { None } else { Some(inname) });
    let mut w = Vec::new();
    let failed = match &typ {
        None => {
            w.extend_from_slice(b"ERROR: ");
            match magicapi::magic_error(ms) {
                Some(e) => w.extend_from_slice(&e),
                None => w.extend_from_slice(b"(null)"),
            }
            w.push(c);
            true
        }
        Some(t) => {
            w.extend_from_slice(t);
            w.push(c);
            false
        }
    };
    out::write(&w);
    if cli.nobuffer {
        out::flush();
    }
    i32::from(failed || out::failed())
}

/// `unwrap`: the names in a file (or standard input for `-`), one a line.
fn unwrap(cli: &Cli, ms: &mut Ms, fname: &[u8]) -> i32 {
    let reader: Box<dyn BufRead> = if fname == b"-" {
        Box::new(std::io::BufReader::new(std::io::stdin()))
    } else {
        match std::fs::File::open(apprentice::os_path(fname)) {
            Ok(f) => Box::new(std::io::BufReader::new(f)),
            Err(e) => {
                let mut msg = b"Cannot open `".to_vec();
                msg.extend_from_slice(fname);
                msg.push(b'\'');
                file_warn(cli, &msg, Some(funcs::Errno::of(&e)));
                return 1;
            }
        }
    };
    let mut e = 0;
    let mut wid = 0usize;
    let mut names: Vec<Vec<u8>> = Vec::new();
    for line in reader.split(b'\n').map_while(Result::ok) {
        let mut line = line;
        // `getline` keeps the newline; upstream strips it, and nothing else.
        // `split` already took it; a NUL ends the name as C reads it.
        if let Some(nul) = line.iter().position(|&c| c == 0) {
            line.truncate(nul);
        }
        let cwid = file_mbswidth(ms, &line);
        if cli.nobuffer {
            e |= process(cli, ms, &line, cwid);
            continue;
        }
        wid = wid.max(cwid);
        names.push(line);
    }
    if !cli.nobuffer {
        for n in &names {
            e |= process(cli, ms, n, wid);
        }
    }
    e
}

#[allow(clippy::too_many_lines)]
fn main() {
    let argv: Vec<OsString> = std::env::args_os().collect();
    let arg0 = argv.first().map(|a| os_bytes(a)).unwrap_or_default();
    let progname = match arg0.iter().rposition(|&c| c == b'/') {
        Some(p) => arg0[p + 1..].to_vec(),
        None => arg0.clone(),
    };
    let mut cli = Cli {
        progname,
        bflag: 0,
        nopad: false,
        nobuffer: false,
        nulsep: 0,
        separator: b":".to_vec(),
        posixly: std::env::var_os("POSIXLY_CORRECT").is_some(),
        params: vec![None; PM.len()],
    };
    out::init();
    let mut flags: u32 = if cli.posixly { MAGIC_SYMLINK } else { 0 };
    let mut action: Option<Action> = None;
    let mut didsomefiles = 0;
    let mut errflg = 0;
    let mut e = 0;
    let mut magic: Option<Ms> = None;
    let mut magicfile: Option<Vec<u8>> = None;
    let mut operands: Vec<Vec<u8>> = Vec::new();

    let prog = Program::new("file", 1);
    let args = argv.get(1..).unwrap_or_default();
    for item in prog.parse(args, OPTSTRING, LONG_OPTIONS).keep_going(true) {
        let (c, optarg): (u8, Option<Vec<u8>>) = match item {
            Err(err) => {
                // glibc's getopt prints its complaint as it goes, named by
                // `argv[0]` as given -- not the basename `file` names itself
                // by -- and `file` counts it and shows the usage at the end.
                out::flush();
                let mut w = arg0.clone();
                w.extend_from_slice(b": ");
                w.extend_from_slice(err.sentence.as_bytes());
                w.push(b'\n');
                let _written = std::io::stderr().write_all(&w);
                errflg += 1;
                continue;
            }
            Ok(Opt::Operand(o)) => {
                operands.push(os_bytes(o));
                continue;
            }
            Ok(Opt::Short(c, v)) => (c, v.map(|v| os_bytes(&v))),
            Ok(Opt::Long(name, v)) => {
                let v = v.map(|v| os_bytes(&v));
                match name {
                    "help" => help(&cli),
                    "apple" => {
                        flags |= MAGIC_APPLE;
                        continue;
                    }
                    "extension" => {
                        flags |= MAGIC_EXTENSION;
                        continue;
                    }
                    "mime-type" => {
                        flags |= MAGIC_MIME_TYPE;
                        continue;
                    }
                    "mime-encoding" => {
                        flags |= MAGIC_MIME_ENCODING;
                        continue;
                    }
                    "exclude-quiet" => {
                        let arg = v.unwrap_or_default();
                        if let Some((_, bit)) = NV.iter().find(|(n, _)| n.as_bytes() == arg.as_slice()) {
                            flags |= bit;
                        }
                        continue;
                    }
                    other => match long_to_short(other) {
                        Some(c) => (c, v),
                        None => continue,
                    },
                }
            }
        };
        let arg = optarg.unwrap_or_default();
        match c {
            b'0' => cli.nulsep += 1,
            b'b' => cli.bflag += 1,
            b'c' => action = Some(Action::Check),
            b'C' => action = Some(Action::Compile),
            b'd' => flags |= MAGIC_DEBUG | MAGIC_CHECK,
            b'E' => flags |= MAGIC_ERROR,
            b'e' => {
                if let Some((_, bit)) = NV.iter().find(|(n, _)| n.as_bytes() == arg.as_slice()) {
                    flags |= bit;
                } else {
                    errflg += 1;
                }
            }
            b'f' => {
                if action.is_some() {
                    usage(&cli);
                }
                if magic.is_none() {
                    match load(&cli, magicfile.as_deref(), flags) {
                        Some(m) => magic = Some(m),
                        None => {
                            out::flush();
                            std::process::exit(1);
                        }
                    }
                }
                if let Some(ms) = magic.as_mut() {
                    applyparam(&cli, ms);
                    e |= unwrap(&cli, ms, &arg);
                }
                didsomefiles += 1;
            }
            b'F' => cli.separator = arg,
            b'i' => flags |= MAGIC_MIME,
            b'k' => flags |= MAGIC_CONTINUE,
            b'l' => action = Some(Action::List),
            b'm' => magicfile = Some(arg),
            b'n' => cli.nobuffer = true,
            b'N' => cli.nopad = true,
            b'p' => flags |= MAGIC_PRESERVE_ATIME,
            b'P' => setparam(&mut cli, &arg),
            b'r' => flags |= MAGIC_RAW,
            b's' => flags |= MAGIC_DEVICES,
            // There is no sandbox to turn off.
            b'S' => {}
            b'v' => {
                let path = magicfile
                    .clone()
                    .unwrap_or_else(|| apprentice::magic_getpath(None, action.unwrap_or(Action::Load)));
                let mut w = cli.progname.clone();
                w.extend_from_slice(b"-5.45\nmagic file from ");
                w.extend_from_slice(&path);
                w.push(b'\n');
                out::write(&w);
                out::flush();
                std::process::exit(0);
            }
            b'z' => flags |= MAGIC_COMPRESS,
            b'Z' => flags |= MAGIC_COMPRESS | MAGIC_COMPRESS_TRANSP,
            b'L' => flags |= MAGIC_SYMLINK,
            b'h' => flags &= !MAGIC_SYMLINK,
            _ => errflg += 1,
        }
    }

    if errflg != 0 {
        usage(&cli);
    }
    if e != 0 {
        out::flush();
        std::process::exit(e);
    }

    let rc = 'run: {
        if let Some(act) = action {
            // Do not check or compile ~/.magic unless asked to.
            let mut ms = magicapi::magic_open(flags | MAGIC_CHECK);
            ms.utf8 = codeset_is_utf8();
            ms.builtin = Some(database::builtin());
            let c = match act {
                Action::Check => magicapi::magic_check(&mut ms, magicfile.as_deref()),
                Action::Compile => magicapi::magic_compile(&mut ms, magicfile.as_deref()),
                Action::List => magicapi::magic_list(&mut ms, magicfile.as_deref()),
                Action::Load => 0,
            };
            if c == -1 {
                out::flush();
                let msg = magicapi::magic_error(&ms).unwrap_or_default();
                let mut w = cli.progname.clone();
                w.extend_from_slice(b": ");
                w.extend_from_slice(&msg);
                w.push(b'\n');
                let _written = std::io::stderr().write_all(&w);
                break 'run 1;
            }
            break 'run 0;
        }
        if magic.is_none() {
            match load(&cli, magicfile.as_deref(), flags) {
                Some(m) => magic = Some(m),
                None => {
                    out::flush();
                    std::process::exit(1);
                }
            }
        }
        let Some(ms) = magic.as_mut() else {
            break 'run 1;
        };
        applyparam(&cli, ms);

        if operands.is_empty() {
            if didsomefiles == 0 {
                usage(&cli);
            }
            break 'run e;
        }
        let wid = operands.iter().map(|o| file_mbswidth(ms, o)).max().unwrap_or(0);
        // `-bb`: brief only for a single file (undocumented upstream).
        if cli.bflag == 2 {
            cli.bflag = u32::from(operands.len() <= 1);
        }
        for o in &operands {
            e |= process(&cli, ms, o, wid);
        }
        e
    };
    let mut rc = rc;
    if !cli.nobuffer && !out::flush() {
        rc |= 1;
    }
    std::process::exit(rc);
}
