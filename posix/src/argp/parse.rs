//! argp's parse, glibc's argp-parse: the argp tree made into getopt's
//! option tables and a list of parsers ("groups", one for each argp with
//! options or a parser, in the tree's order), then each argument given to
//! getopt, and what it finds to the parser it belongs to.
//!
//! The order the parsers hear things in is glibc's, and is observable:
//! `ARGP_KEY_INIT` to each in the tree's order, a parent before its
//! children, so that it can give them their inputs; each option to the
//! parser whose table it came from; each argument to each parser in turn
//! until one takes it -- as `ARGP_KEY_ARG`, then as `ARGP_KEY_ARGS` with
//! the rest; then `ARGP_KEY_NO_ARGS` to each that had no argument,
//! `ARGP_KEY_END` to each in reverse, and `ARGP_KEY_SUCCESS` or
//! `ARGP_KEY_ERROR`, and `ARGP_KEY_FINI` last.
//!
//! A parser may call back into argp while it is being called -- for help,
//! which reads the parse's groups through `state->pstate` -- so the parse
//! is reached through a raw pointer, never a reference held across a call.

use super::{
    ARGP_ERR_UNKNOWN, ARGP_HELP_EXIT_OK, ARGP_HELP_STD_ERR, ARGP_HELP_STD_HELP, ARGP_HELP_USAGE,
    ARGP_IN_ORDER, ARGP_KEY_ARG, ARGP_KEY_ARGS, ARGP_KEY_END, ARGP_KEY_ERROR, ARGP_KEY_FINI,
    ARGP_KEY_INIT, ARGP_KEY_NO_ARGS, ARGP_KEY_SUCCESS, ARGP_LONG_ONLY, ARGP_NO_ARGS, ARGP_NO_ERRS,
    ARGP_NO_EXIT, ARGP_NO_HELP, ARGP_PARSE_ARGV0, Argp, ArgpChild, ArgpOption, ArgpParserFn,
    ArgpState, OPTION_ALIAS, OPTION_ARG_OPTIONAL, OPTION_DOC, OPTION_HIDDEN, is_end, is_short,
};
use crate::getopt;
use crate::list::List;
use core::ffi::c_void;

/// The built-in options' keys, glibc's: long options only.
const OPT_PROGNAME: i32 = -2;
const OPT_USAGE: i32 = -3;
const OPT_HANG: i32 = -4;

/// getopt's answers: the options' end; an argument (with `-` first in the
/// option string); an error, or the option `-?` -- `'?'`, which is also
/// `--help`'s key, and `'V'` `--version`'s.
const KEY_END: i32 = -1;
const KEY_ARG: i32 = 1;
const KEY_ERR: i32 = 0x3F;
const KEY_VERSION: i32 = 0x56;

/// A long option's `val`: its key in the low 24 bits, sign kept, and its
/// group's number plus one above them.
const USER_BITS: u32 = 24;
const USER_MASK: i32 = (1 << USER_BITS) - 1;

/// An option entry, for the library's own tables.
const fn opt(
    name: &'static core::ffi::CStr,
    key: i32,
    arg: *const u8,
    flags: i32,
    doc: &'static core::ffi::CStr,
    group: i32,
) -> ArgpOption {
    ArgpOption {
        name: name.as_ptr().cast(),
        key,
        arg,
        flags,
        doc: doc.as_ptr().cast(),
        group,
    }
}

/// The entry that ends a table.
const END: ArgpOption = ArgpOption {
    name: core::ptr::null(),
    key: 0,
    arg: core::ptr::null(),
    flags: 0,
    doc: core::ptr::null(),
    group: 0,
};

/// `--help`, `--usage`, and the two hidden ones, glibc's, with its texts.
static DEFAULT_OPTIONS: [ArgpOption; 5] = [
    opt(
        c"help",
        KEY_ERR,
        core::ptr::null(),
        0,
        c"Give this help list",
        -1,
    ),
    opt(
        c"usage",
        OPT_USAGE,
        core::ptr::null(),
        0,
        c"Give a short usage message",
        0,
    ),
    opt(
        c"program-name",
        OPT_PROGNAME,
        c"NAME".as_ptr().cast(),
        OPTION_HIDDEN,
        c"Set the program name",
        0,
    ),
    opt(
        c"HANG",
        OPT_HANG,
        c"SECS".as_ptr().cast(),
        OPTION_ARG_OPTIONAL | OPTION_HIDDEN,
        c"Hang for SECS seconds (default 3600)",
        0,
    ),
    END,
];

static DEFAULT_ARGP: Argp = Argp {
    options: DEFAULT_OPTIONS.as_ptr(),
    parser: Some(default_parser),
    args_doc: core::ptr::null(),
    doc: core::ptr::null(),
    children: core::ptr::null(),
    help_filter: None,
    argp_domain: c"libc".as_ptr().cast(),
};

/// `--version`, there when the program has a version or a hook.
static VERSION_OPTIONS: [ArgpOption; 2] = [
    opt(
        c"version",
        KEY_VERSION,
        core::ptr::null(),
        0,
        c"Print program version",
        -1,
    ),
    END,
];

static VERSION_ARGP: Argp = Argp {
    options: VERSION_OPTIONS.as_ptr(),
    parser: Some(version_parser),
    args_doc: core::ptr::null(),
    doc: core::ptr::null(),
    children: core::ptr::null(),
    help_filter: None,
    argp_domain: c"libc".as_ptr().cast(),
};

callback! {
    /// The built-in options' parser.
    ///
    /// # Safety
    ///
    /// `state` is the parse's state.
    unsafe fn default_parser(key: i32, arg: *mut u8, state: *mut ArgpState) -> i32 {
        match key {
            KEY_ERR => {
                // SAFETY: the parse's state, and its stream.
                unsafe { super::state_help(state, (*state).out_stream, ARGP_HELP_STD_HELP) };
            }
            OPT_USAGE => {
                // SAFETY: as above.
                unsafe {
                    super::state_help(
                        state,
                        (*state).out_stream,
                        ARGP_HELP_USAGE | ARGP_HELP_EXIT_OK,
                    )
                };
            }
            OPT_PROGNAME => {
                // SAFETY: the parse's state; `arg` is the option's argument, a C
                // string in the program's argv; the two names are plain words.
                unsafe {
                    (&raw mut crate::crt::__progname_full).write(arg.cast_const());
                    let short = base_name(arg);
                    (*state).name = short;
                    (&raw mut crate::crt::__progname).write(short.cast_const());
                    if (*state).flags & (ARGP_PARSE_ARGV0 | ARGP_NO_ERRS) == ARGP_PARSE_ARGV0 {
                        // What getopt's messages say too.
                        *(*state).argv = arg;
                    }
                }
            }
            OPT_HANG => {
                // SAFETY: NULL or the option's argument, a C string.
                let secs = if arg.is_null() {
                    3600
                } else {
                    unsafe { crate::stdlib::atoi(arg) }
                };
                for _ in 0..secs.max(0) {
                    crate::time::sleep(1);
                }
            }
            _ => return ARGP_ERR_UNKNOWN,
        }
        0
    }
}

callback! {
    /// `--version`'s parser.
    ///
    /// # Safety
    ///
    /// As [`default_parser`].
    unsafe fn version_parser(key: i32, _arg: *mut u8, state: *mut ArgpState) -> i32 {
        if key != KEY_VERSION {
            return ARGP_ERR_UNKNOWN;
        }
        // SAFETY: the parse's state.
        let out = unsafe { (*state).out_stream };
        if let Some(hook) = super::version_hook() {
            // SAFETY: the program's hook, with its stream and the state.
            unsafe { hook(out, state) };
        } else if !super::version().is_null() {
            // SAFETY: the program's version, a C string.
            super::put(out, unsafe { super::text(super::version()) });
            super::put(out, b"\n");
        } else {
            // SAFETY: the parse's state.
            unsafe { super::error_message(state, &[b"(PROGRAM ERROR) No version known!?"]) };
        }
        // SAFETY: as above.
        if unsafe { (*state).flags } & ARGP_NO_EXIT == 0 {
            super::exit_now(0);
        }
        0
    }
}

/// After the last `/`, as glibc's `__argp_base_name`.
///
/// # Safety
///
/// `name` is a C string.
unsafe fn base_name(name: *mut u8) -> *mut u8 {
    // SAFETY: the caller's C string.
    let bytes = unsafe { super::text(name) };
    match bytes.iter().rposition(|&c| c == b'/') {
        // SAFETY: within the string.
        Some(i) => unsafe { name.add(i.saturating_add(1)) },
        None => name,
    }
}

/// One parser of the parse: glibc's `struct group`.
struct Group {
    parser: Option<ArgpParserFn>,
    argp: *const Argp,
    /// Its short options end here in the option string.
    short_end: usize,
    /// The arguments it has taken.
    args_processed: u32,
    /// Its parent's group, if it has one, and its place among the parent's
    /// children.
    parent: Option<usize>,
    parent_index: usize,
    input: *mut c_void,
    /// Its children's inputs: where they start in the parse's list.
    child_inputs: Option<usize>,
    hook: *mut c_void,
}

/// A parse: glibc's `struct parser`.
pub(crate) struct Parser {
    /// getopt's option string: `-` or `+` first, as the flags ask, then
    /// each group's short options in turn. NUL-ended.
    short_opts: List<u8>,
    /// getopt's long options, ended by an entry of zeros.
    long_opts: List<getopt::Option>,
    groups: List<Group>,
    /// Every group's children's inputs; never grown once the parse begins,
    /// since the parsers hold pointers into it.
    child_inputs: List<*mut c_void>,
    /// Whether getopt is still to be asked: not after the options end.
    try_getopt: bool,
    /// Whether getopt writes its complaints: not under `ARGP_NO_ERRS`.
    opterr: bool,
    state: ArgpState,
    getopt: getopt::engine::State,
    /// `ARGP_PARSE_ARGV0` under `ARGP_NO_ERRS`: the program's argv behind
    /// a placeholder, which getopt skips as it skips `argv[0]` (glibc moves
    /// the pointer back one, before the array); the order getopt leaves is
    /// copied back to the program's at the end.
    shifted: Option<(List<*mut u8>, *mut *mut u8)>,
}

/// The parse's lists could not be had.
struct NoMem;

impl From<crate::list::NoMem> for NoMem {
    fn from(_: crate::list::NoMem) -> Self {
        Self
    }
}

/// `argp_parse`.
///
/// # Safety
///
/// As [`super::argp_parse`].
pub(crate) unsafe fn parse(
    argp: *const Argp,
    argc: i32,
    argv: *mut *mut u8,
    flags: u32,
    end_index: *mut i32,
    input: *mut c_void,
) -> i32 {
    // The program's argp, the built-in options and maybe `--version`, as
    // the children of one of argp's own -- unless ARGP_NO_HELP.
    let mut kids = [const {
        ArgpChild {
            argp: core::ptr::null(),
            flags: 0,
            header: core::ptr::null(),
            group: 0,
        }
    }; 4];
    let mut top = Argp {
        options: core::ptr::null(),
        parser: None,
        args_doc: core::ptr::null(),
        doc: core::ptr::null(),
        children: core::ptr::null(),
        help_filter: None,
        argp_domain: core::ptr::null(),
    };
    let root = if flags & ARGP_NO_HELP == 0 {
        let mut n = 0;
        let mut add = |a: *const Argp| {
            if let Some(k) = kids.get_mut(n) {
                k.argp = a;
                n = n.saturating_add(1);
            }
        };
        if !argp.is_null() {
            add(argp);
        }
        add(&raw const DEFAULT_ARGP);
        if !super::version().is_null() || super::version_hook().is_some() {
            add(&raw const VERSION_ARGP);
        }
        top.children = kids.as_ptr();
        &raw const top
    } else {
        argp
    };

    let mut parser = Parser {
        short_opts: List::new(),
        long_opts: List::new(),
        groups: List::new(),
        child_inputs: List::new(),
        try_getopt: true,
        opterr: true,
        state: ArgpState {
            root_argp: root,
            argc,
            argv,
            next: 0,
            flags,
            arg_num: 0,
            quoted: 0,
            input: core::ptr::null_mut(),
            child_inputs: core::ptr::null_mut(),
            hook: core::ptr::null_mut(),
            name: core::ptr::null_mut(),
            err_stream: crate::stdio::stderr_stream(),
            out_stream: crate::stdio::stdout_stream(),
            pstate: core::ptr::null_mut(),
        },
        getopt: getopt::engine::State::new(),
        shifted: None,
    };
    let p = &raw mut parser;
    // SAFETY: `p` is the parse, alive to the end of this function; the
    // argp tree and argv are the caller's.
    unsafe {
        if convert(p, root, flags).is_err() {
            return crate::errno::ENOMEM;
        }
        (*p).state.pstate = p.cast();
        let mut err = init(p, input, argc, argv);
        if err != 0 {
            return err;
        }
        let mut arg_ebadkey = false;
        while err == 0 {
            err = parse_next(p, &mut arg_ebadkey);
        }
        let err = finalize(p, err, arg_ebadkey, end_index);
        if let Some((copy, orig)) = (*p).shifted.take() {
            // The order getopt left, and any argument a parser replaced.
            for (i, &a) in copy
                .iter()
                .skip(1)
                .take(usize::try_from(argc).unwrap_or(0))
                .enumerate()
            {
                orig.add(i).write(a);
            }
        }
        err
    }
}

/// The tree made into option tables and groups: glibc's `parser_convert`.
///
/// # Safety
///
/// `p` is the parse; `root` NULL or a valid argp tree.
unsafe fn convert(p: *mut Parser, root: *const Argp, flags: u32) -> Result<(), NoMem> {
    // SAFETY: the caller's parse.
    let parser = unsafe { &mut *p };
    if flags & ARGP_IN_ORDER != 0 {
        parser.short_opts.push(b'-')?;
    } else if flags & ARGP_NO_ARGS != 0 {
        parser.short_opts.push(b'+')?;
    }
    if !root.is_null() {
        // SAFETY: the caller's tree.
        unsafe { convert_options(parser, root, None, 0)? };
    }
    parser.short_opts.push(0)?;
    parser.long_opts.push(getopt::Option {
        name: core::ptr::null(),
        has_arg: 0,
        flag: core::ptr::null_mut(),
        val: 0,
    })?;
    Ok(())
}

/// One argp's options, then its children's: glibc's `convert_options`.
///
/// # Safety
///
/// `argp` is a valid argp tree.
unsafe fn convert_options(
    parser: &mut Parser,
    argp: *const Argp,
    parent: Option<usize>,
    parent_index: usize,
) -> Result<(), NoMem> {
    // SAFETY: the caller's tree.
    let a = unsafe { &*argp };
    let mut this = None;
    if !a.options.is_null() || a.parser.is_some() {
        let gi = parser.groups.len();
        if !a.options.is_null() {
            let mut o = a.options;
            let mut real = o;
            // SAFETY: a table ended by its end entry.
            unsafe {
                while !is_end(&*o) {
                    if (*o).flags & OPTION_ALIAS == 0 {
                        real = o;
                    }
                    if (*real).flags & OPTION_DOC == 0 {
                        if is_short(&*o) {
                            parser
                                .short_opts
                                .push(u8::try_from((*o).key).unwrap_or(0))?;
                            if !(*real).arg.is_null() {
                                parser.short_opts.push(b':')?;
                                if (*real).flags & OPTION_ARG_OPTIONAL != 0 {
                                    parser.short_opts.push(b':')?;
                                }
                            }
                        }
                        if !(*o).name.is_null() && find_long(&parser.long_opts, (*o).name).is_none()
                        {
                            let has_arg = if (*real).arg.is_null() {
                                getopt::NO_ARGUMENT
                            } else if (*real).flags & OPTION_ARG_OPTIONAL != 0 {
                                getopt::OPTIONAL_ARGUMENT
                            } else {
                                getopt::REQUIRED_ARGUMENT
                            };
                            let key = if (*o).key != 0 { (*o).key } else { (*real).key };
                            let group = i32::try_from(gi.saturating_add(1)).unwrap_or(0);
                            parser.long_opts.push(getopt::Option {
                                name: (*o).name,
                                has_arg,
                                flag: core::ptr::null_mut(),
                                val: (key & USER_MASK).wrapping_add(group.wrapping_shl(USER_BITS)),
                            })?;
                        }
                    }
                    o = o.add(1);
                }
            }
        }
        let child_inputs = if count_children(a) > 0 {
            let at = parser.child_inputs.len();
            for _ in 0..count_children(a) {
                parser.child_inputs.push(core::ptr::null_mut())?;
            }
            Some(at)
        } else {
            None
        };
        parser.groups.push(Group {
            parser: a.parser,
            argp,
            short_end: parser.short_opts.len(),
            args_processed: 0,
            parent,
            parent_index,
            input: core::ptr::null_mut(),
            child_inputs,
            hook: core::ptr::null_mut(),
        })?;
        this = Some(gi);
    }
    if !a.children.is_null() {
        let mut c = a.children;
        let mut i = 0usize;
        // SAFETY: a list ended by an entry whose argp is NULL.
        unsafe {
            while !(*c).argp.is_null() {
                convert_options(parser, (*c).argp, this, i)?;
                i = i.saturating_add(1);
                c = c.add(1);
            }
        }
    }
    Ok(())
}

/// How many children an argp has.
fn count_children(a: &Argp) -> usize {
    if a.children.is_null() {
        return 0;
    }
    let mut n = 0usize;
    // SAFETY: a list ended by an entry whose argp is NULL.
    unsafe {
        while !(*a.children.add(n)).argp.is_null() {
            n = n.saturating_add(1);
        }
    }
    n
}

/// The long option of that name already in the table.
fn find_long(table: &List<getopt::Option>, name: *const u8) -> Option<usize> {
    table.iter().position(|o| {
        // SAFETY: both C strings.
        !o.name.is_null() && unsafe { crate::string::strcmp(o.name, name) } == 0
    })
}

/// The parse's groups, for a look -- never held across a parser's call.
///
/// # Safety
///
/// `p` is the parse.
unsafe fn groups<'a>(p: *const Parser) -> &'a [Group] {
    // SAFETY: the caller's parse; the reference is the caller's to drop
    // before a parser runs.
    unsafe { (*p).groups.as_slice() }
}

/// The pointer to a group's children's inputs.
///
/// # Safety
///
/// `p` is the parse.
unsafe fn child_inputs_of(p: *mut Parser, gi: usize) -> *mut *mut c_void {
    // SAFETY: the parse; the list is never grown once the parse begins.
    unsafe {
        match groups(p).get(gi).and_then(|g| g.child_inputs) {
            Some(at) => (*p).child_inputs.as_mut_slice().as_mut_ptr().add(at),
            None => core::ptr::null_mut(),
        }
    }
}

/// A group's parser called with `key`: glibc's `group_parse`.
///
/// # Safety
///
/// `p` is the parse.
unsafe fn group_parse(p: *mut Parser, gi: usize, key: i32, arg: *mut u8) -> i32 {
    // SAFETY: the parse; no reference into it is held across the call.
    unsafe {
        let Some(g) = groups(p).get(gi) else {
            return ARGP_ERR_UNKNOWN;
        };
        let Some(parser) = g.parser else {
            return ARGP_ERR_UNKNOWN;
        };
        let (hook, input, args) = (g.hook, g.input, g.args_processed);
        let ci = child_inputs_of(p, gi);
        let st = &raw mut (*p).state;
        (*st).hook = hook;
        (*st).input = input;
        (*st).child_inputs = ci;
        (*st).arg_num = args;
        let err = parser(key, arg, st);
        if let Some(g) = (*p).groups.as_mut_slice().get_mut(gi) {
            g.hook = (*st).hook;
        }
        err
    }
}

/// `ARGP_KEY_INIT` to each, and the state made ready: glibc's
/// `parser_init`.
///
/// # Safety
///
/// `p` is the parse; `argv` holds `argc` pointers (or is NULL with 0).
unsafe fn init(p: *mut Parser, input: *mut c_void, argc: i32, argv: *mut *mut u8) -> i32 {
    // SAFETY: the parse.
    unsafe {
        let n = groups(p).len();
        if let Some(first) = (*p).groups.as_mut_slice().first_mut() {
            first.input = input;
        }
        let mut err = 0;
        let mut gi = 0;
        while gi < n && (err == 0 || err == ARGP_ERR_UNKNOWN) {
            let (parent, parent_index, has_parser, argp) = match groups(p).get(gi) {
                Some(g) => (g.parent, g.parent_index, g.parser.is_some(), g.argp),
                None => break,
            };
            if let Some(pi) = parent {
                let from = child_inputs_of(p, pi);
                if !from.is_null() {
                    let v = from.add(parent_index).read();
                    if let Some(g) = (*p).groups.as_mut_slice().get_mut(gi) {
                        g.input = v;
                    }
                }
            }
            // A wrapper with no parser of its own passes its input to its
            // first child.
            if !has_parser && count_children(&*argp) > 0 {
                let to = child_inputs_of(p, gi);
                if !to.is_null() {
                    to.write(groups(p).get(gi).map_or(core::ptr::null_mut(), |g| g.input));
                }
            }
            err = group_parse(p, gi, ARGP_KEY_INIT, core::ptr::null_mut());
            gi = gi.saturating_add(1);
        }
        if err == ARGP_ERR_UNKNOWN {
            err = 0;
        }
        if err != 0 {
            return err;
        }
        let flags = (*p).state.flags;
        let print_errors = flags & ARGP_NO_ERRS == 0;
        (*p).opterr = print_errors;
        if !print_errors && flags & ARGP_PARSE_ARGV0 != 0 {
            // getopt skips argv[0]: give it a placeholder to skip.
            let count = usize::try_from(argc).unwrap_or(0);
            let mut copy: List<*mut u8> = List::new();
            if copy.push(c"".as_ptr().cast_mut().cast()).is_err() {
                return crate::errno::ENOMEM;
            }
            for i in 0..count {
                if copy.push(argv.add(i).read()).is_err() {
                    return crate::errno::ENOMEM;
                }
            }
            if copy.push(core::ptr::null_mut()).is_err() {
                return crate::errno::ENOMEM;
            }
            (*p).state.argv = copy.as_mut_slice().as_mut_ptr();
            (*p).state.argc = argc.saturating_add(1);
            (*p).shifted = Some((copy, argv));
        }
        (*p).state.name =
            if (*p).shifted.is_none() && !argv.is_null() && argc > 0 && !argv.read().is_null() {
                base_name(argv.read())
            } else {
                super::short_program_name()
            };
        0
    }
}

/// The next option or argument parsed: glibc's `parser_parse_next`.
///
/// # Safety
///
/// `p` is the parse.
unsafe fn parse_next(p: *mut Parser, arg_ebadkey: &mut bool) -> i32 {
    // SAFETY: the parse.
    unsafe {
        let st = &raw mut (*p).state;
        if (*st).quoted != 0 && (*st).next < (*st).quoted {
            // A parser moved back before the `--`: give getopt another look.
            (*st).quoted = 0;
        }
        let mut opt;
        if (*p).try_getopt && (*st).quoted == 0 {
            (*p).getopt.optind = (*st).next;
            (*p).getopt.optopt = KEY_END;
            let long_only = (*st).flags & ARGP_LONG_ONLY != 0;
            opt = getopt::engine::parse_one(
                &mut (*p).getopt,
                (*st).argc,
                (*st).argv.cast(),
                (*p).short_opts.as_slice().as_ptr(),
                (*p).long_opts.as_slice().as_ptr(),
                core::ptr::null_mut(),
                long_only,
                (*p).opterr,
                &getopt::engine::Io::process(),
            );
            (*st).next = (*p).getopt.optind;
            if opt == KEY_END {
                (*p).try_getopt = false;
                let next = (*st).next;
                if next > 1
                    && usize::try_from(next.saturating_sub(1))
                        .is_ok_and(|i| super::text((*st).argv.add(i).read()) == b"--")
                {
                    // A quoted region: whatever looks like an option there
                    // is an argument.
                    (*st).quoted = next;
                }
            } else if opt == KEY_ERR && (*p).getopt.optopt != KEY_END {
                *arg_ebadkey = false;
                return ARGP_ERR_UNKNOWN;
            }
        } else {
            opt = KEY_END;
        }
        let mut optarg = (*p).getopt.optarg.cast_mut();
        if opt == KEY_END {
            if (*st).next >= (*st).argc || (*st).flags & ARGP_NO_ARGS != 0 {
                *arg_ebadkey = true;
                return ARGP_ERR_UNKNOWN;
            }
            opt = KEY_ARG;
            let i = usize::try_from((*st).next).unwrap_or(0);
            optarg = (*st).argv.add(i).read();
            (*st).next = (*st).next.saturating_add(1);
        }
        let err = if opt == KEY_ARG {
            parse_arg(p, optarg)
        } else {
            parse_opt(p, opt, optarg)
        };
        if err == ARGP_ERR_UNKNOWN {
            *arg_ebadkey = opt == KEY_END || opt == KEY_ARG;
        }
        err
    }
}

/// An argument offered to each parser in turn: glibc's `parser_parse_arg`.
///
/// # Safety
///
/// `p` is the parse.
unsafe fn parse_arg(p: *mut Parser, val: *mut u8) -> i32 {
    // SAFETY: the parse.
    unsafe {
        let st = &raw mut (*p).state;
        (*st).next = (*st).next.saturating_sub(1);
        let index = (*st).next;
        let n = groups(p).len();
        let mut err = ARGP_ERR_UNKNOWN;
        let mut key = ARGP_KEY_ARG;
        let mut gi = 0;
        while gi < n && err == ARGP_ERR_UNKNOWN {
            (*st).next = (*st).next.saturating_add(1);
            key = ARGP_KEY_ARG;
            err = group_parse(p, gi, key, val);
            if err == ARGP_ERR_UNKNOWN {
                // Not as one argument: as the rest of them.
                (*st).next = (*st).next.saturating_sub(1);
                key = ARGP_KEY_ARGS;
                err = group_parse(p, gi, key, core::ptr::null_mut());
            }
            gi = gi.saturating_add(1);
        }
        if err == 0 {
            if key == ARGP_KEY_ARGS {
                // Unless the parser moved `next`, it took them all.
                (*st).next = (*st).argc;
            }
            if (*st).next > index {
                let took = u32::try_from((*st).next.saturating_sub(index)).unwrap_or(0);
                if let Some(g) = (*p).groups.as_mut_slice().get_mut(gi.saturating_sub(1)) {
                    g.args_processed = g.args_processed.saturating_add(took);
                }
            } else {
                // The parser moved back: give getopt another try.
                (*p).try_getopt = true;
            }
        }
        err
    }
}

/// An option to the parser it is from: glibc's `parser_parse_opt`.
///
/// # Safety
///
/// `p` is the parse.
unsafe fn parse_opt(p: *mut Parser, opt: i32, val: *mut u8) -> i32 {
    // SAFETY: the parse.
    unsafe {
        let group_key = opt >> USER_BITS;
        let mut err = ARGP_ERR_UNKNOWN;
        if group_key == 0 {
            // A short option: the group whose part of the option string holds it.
            let shorts = (*p).short_opts.as_slice();
            let at = u8::try_from(opt)
                .ok()
                .and_then(|c| shorts.iter().position(|&s| s == c && c != 0));
            if let Some(at) = at {
                let n = groups(p).len();
                for gi in 0..n {
                    if groups(p).get(gi).is_some_and(|g| g.short_end > at) {
                        err = group_parse(p, gi, opt, val);
                        break;
                    }
                }
            }
        } else {
            // A long option: its group is in its `val`, its key below,
            // sign and all.
            let user_key = (if opt & (1 << (USER_BITS - 1)) != 0 {
                !USER_MASK
            } else {
                0
            }) | (opt & USER_MASK);
            let gi = usize::try_from(group_key.saturating_sub(1)).unwrap_or(usize::MAX);
            err = group_parse(p, gi, user_key, val);
        }
        if err == ARGP_ERR_UNKNOWN {
            // Every option is meant for the parser it came from.
            const BAD_KEY: &[u8] = b"(PROGRAM ERROR) Option should have been recognized!?";
            let st = &raw const (*p).state;
            if group_key == 0 {
                let c = [u8::try_from(opt).unwrap_or(b'?')];
                super::error_message(st, &[b"-", &c, b": ", BAD_KEY]);
            } else {
                let name = (*p)
                    .long_opts
                    .iter()
                    .find(|o| o.val == opt && !o.name.is_null())
                    .map_or(&b"???"[..], |o| super::text(o.name));
                super::error_message(st, &[b"--", name, b": ", BAD_KEY]);
            }
        }
        err
    }
}

/// The parsers told how the parse ended: glibc's `parser_finalize`.
///
/// # Safety
///
/// `p` is the parse; `end_index` NULL or writable.
unsafe fn finalize(p: *mut Parser, mut err: i32, arg_ebadkey: bool, end_index: *mut i32) -> i32 {
    // SAFETY: the parse.
    unsafe {
        let st = &raw mut (*p).state;
        let n = groups(p).len();
        if err == ARGP_ERR_UNKNOWN && arg_ebadkey {
            // Arguments no parser took are no error.
            err = 0;
        }
        if err == 0 {
            if (*st).next == (*st).argc {
                // Every argument parsed: the parsers called a few more times.
                let mut gi = 0;
                while gi < n && (err == 0 || err == ARGP_ERR_UNKNOWN) {
                    if groups(p).get(gi).is_some_and(|g| g.args_processed == 0) {
                        err = group_parse(p, gi, ARGP_KEY_NO_ARGS, core::ptr::null_mut());
                    }
                    gi = gi.saturating_add(1);
                }
                let mut gi = n;
                while gi > 0 && (err == 0 || err == ARGP_ERR_UNKNOWN) {
                    gi = gi.saturating_sub(1);
                    err = group_parse(p, gi, ARGP_KEY_END, core::ptr::null_mut());
                }
                if err == ARGP_ERR_UNKNOWN {
                    err = 0;
                }
                if !end_index.is_null() {
                    end_index.write((*st).next);
                }
            } else if !end_index.is_null() {
                // The rest are the program's.
                end_index.write((*st).next);
            } else {
                // No way to give the rest back: they must be wrong.
                if (*st).flags & ARGP_NO_ERRS == 0 && !(*st).err_stream.is_null() {
                    let stream = (*st).err_stream;
                    let _l = super::Locked::new(stream);
                    super::put(stream, super::text((*st).name));
                    super::put(stream, b": Too many arguments\n");
                }
                err = ARGP_ERR_UNKNOWN;
            }
        }
        if err != 0 {
            if err == ARGP_ERR_UNKNOWN {
                // What was wrong was said already; now how to see more.
                super::state_help(st, (*st).err_stream, ARGP_HELP_STD_ERR);
            }
            for gi in 0..n {
                group_parse(p, gi, ARGP_KEY_ERROR, core::ptr::null_mut());
            }
        } else {
            // Children before their parents, so that they can pass values up.
            let mut gi = n;
            while gi > 0 && (err == 0 || err == ARGP_ERR_UNKNOWN) {
                gi = gi.saturating_sub(1);
                err = group_parse(p, gi, ARGP_KEY_SUCCESS, core::ptr::null_mut());
            }
            if err == ARGP_ERR_UNKNOWN {
                err = 0;
            }
        }
        // Last, whatever happened; their answers do not matter.
        let mut gi = n;
        while gi > 0 {
            gi = gi.saturating_sub(1);
            group_parse(p, gi, ARGP_KEY_FINI, core::ptr::null_mut());
        }
        if err == ARGP_ERR_UNKNOWN {
            crate::errno::EINVAL
        } else {
            err
        }
    }
}

/// The input `argp`'s parser has in `state`'s parse, or NULL: glibc's
/// `__argp_input`.
///
/// # Safety
///
/// `state` is NULL or a parse's state.
pub(crate) unsafe fn input_of(argp: *const Argp, state: *const ArgpState) -> *mut c_void {
    if state.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: a parse's state, whose `pstate` is its parse.
    unsafe {
        let p = (*state).pstate.cast::<Parser>();
        if p.is_null() {
            return core::ptr::null_mut();
        }
        (*p).groups
            .iter()
            .find(|g| g.argp == argp)
            .map_or(core::ptr::null_mut(), |g| g.input)
    }
}
