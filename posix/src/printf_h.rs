//! `<printf.h>`: glibc's window onto its printf engine.
//!
//! [`parse_printf_format`] says how many arguments a format takes and the
//! type of each, the way glibc's own engine types them when it reads a
//! format by position: `%d` a `PA_INT`, `%lu` a `PA_INT | PA_FLAG_LONG`,
//! `%Lf` a `PA_DOUBLE | PA_FLAG_LONG_DOUBLE`, `%s` a `PA_STRING`, each `*`
//! a `PA_INT`. It is glibc's to the letter (`printf_parse_oracle.txt`, every
//! conversion under every length modifier), quirks included: `%lc` and
//! `%ls` are a `PA_CHAR` and a `PA_STRING`, as glibc's parser calls them --
//! only `%C` and `%S` are the wide types -- and `%qd` is a plain `PA_INT`
//! where `%lld` is a `long` ([`Length::Quad`]).
//!
//! The specification is read by the engine's own parser
//! ([`crate::printf::parse_spec`]), so the two cannot disagree about what a
//! format says.
//!
//! ## Registration
//!
//! A program can add conversions of its own ([`register_printf_specifier`]
//! and the older [`register_printf_function`]), length modifiers
//! ([`register_printf_modifier`]) and argument types
//! ([`register_printf_type`]), and register [`printf_size`] as one. All of
//! it is glibc's (`printf_reg_oracle.txt`), down to what a handler is
//! given: a `struct printf_info` -- [`PrintfInfo`], 20 bytes, its bitfields
//! where glibc's are -- a stream writing into the call's output, wide for
//! the wide family, and pointers to its arguments, a registered type's
//! being a pointer to its memory.
//!
//! As in glibc, a registration lasts for the process, and from the first
//! one on every format -- registered conversions or none -- is read as
//! glibc's positional pass reads one ([`crate::printf::format_registered`]):
//! every argument typed and read before anything is written. So, as
//! glibc's does then, a format cut short is written back (`abc%` is `abc%`)
//! rather than failing. Two differences, on purpose: a number past
//! `INT_MAX` still fails the call, `EOVERFLOW` (design-decisions 1173), and
//! a numbered conversion whose arginfo asks for several arguments gets room
//! for them all, where glibc's array is sized without them and would be
//! written past.

use crate::printf::{Count, INT_MAX, Length, Star};
use crate::wchar::WcharT;

/// `int`.
pub const PA_INT: i32 = 0;
/// `int`, cast to `char`.
pub const PA_CHAR: i32 = 1;
/// A wide character, `wint_t`.
pub const PA_WCHAR: i32 = 2;
/// `const char *`, a NUL-terminated string.
pub const PA_STRING: i32 = 3;
/// `const wchar_t *`, a NUL-terminated wide string.
pub const PA_WSTRING: i32 = 4;
/// `void *`.
pub const PA_POINTER: i32 = 5;
/// `float`.
pub const PA_FLOAT: i32 = 6;
/// `double`.
pub const PA_DOUBLE: i32 = 7;
/// The first value after the built-in types.
pub const PA_LAST: i32 = 8;
/// The flag bits of a type.
pub const PA_FLAG_MASK: i32 = 0xff00;
/// `long long`.
pub const PA_FLAG_LONG_LONG: i32 = 1 << 8;
/// `long double`: the same bit as `long long`.
pub const PA_FLAG_LONG_DOUBLE: i32 = PA_FLAG_LONG_LONG;
/// `long`.
pub const PA_FLAG_LONG: i32 = 1 << 9;
/// `short`.
pub const PA_FLAG_SHORT: i32 = 1 << 10;
/// A pointer to the type.
pub const PA_FLAG_PTR: i32 = 1 << 11;

/// glibc's `struct printf_info`: a conversion specification as glibc's
/// parser leaves it. 20 bytes, measured against glibc 2.39's: the
/// precision (-1 for none), the width, the conversion character, then
/// thirteen one-bit fields packed into [`PrintfInfo::bits`] from its lowest
/// bit, the bits of user-registered modifiers, and the padding character.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrintfInfo {
    /// The precision; -1 for none.
    pub prec: i32,
    /// The width.
    pub width: i32,
    /// The conversion character.
    pub spec: WcharT,
    /// glibc's bitfields, the `IS_*` and flag constants below.
    pub bits: u16,
    /// Bits for user-registered modifiers.
    pub user: u16,
    /// The padding character: `'0'` for the `0` flag without `-`, else `' '`.
    pub pad: WcharT,
}

impl PrintfInfo {
    /// `L` (and `ll`, `q`, `w64`): `long double`, or `long long`.
    pub const IS_LONG_DOUBLE: u16 = 1 << 0;
    /// `h`.
    pub const IS_SHORT: u16 = 1 << 1;
    /// `l` (and `ll`, `j`, `z`, `Z`, `t`, `w64`).
    pub const IS_LONG: u16 = 1 << 2;
    /// `#`.
    pub const ALT: u16 = 1 << 3;
    /// A space.
    pub const SPACE: u16 = 1 << 4;
    /// `-`.
    pub const LEFT: u16 = 1 << 5;
    /// `+`.
    pub const SHOWSIGN: u16 = 1 << 6;
    /// `'`.
    pub const GROUP: u16 = 1 << 7;
    /// For a handler's own use.
    pub const EXTRA: u16 = 1 << 8;
    /// `hh`.
    pub const IS_CHAR: u16 = 1 << 9;
    /// The output is a wide stream.
    pub const WIDE: u16 = 1 << 10;
    /// `I`.
    pub const I18N: u16 = 1 << 11;
    /// The floating argument is a `binary128`.
    pub const IS_BINARY128: u16 = 1 << 12;
}

/// The bits glibc's parser sets in a [`PrintfInfo`] for a length modifier,
/// on x86-64 (`printf-parsemb.c`): `ll` is `is_long` and `is_long_double`,
/// `L` and `q` only `is_long_double`, and `j`, `z`, `Z`, `t` -- 64 bits
/// wide -- `is_long`. C23's `w8` and `wf8` are `is_char`, `w16` `is_short`,
/// `w32` nothing, and `w64` and the other fast widths, which are 64 bits
/// here, `is_long` and `is_long_double`, as `ll`.
pub(crate) const fn length_bits(length: Length) -> u16 {
    match length {
        Length::Char | Length::Bits(8) => PrintfInfo::IS_CHAR,
        Length::Short | Length::Bits(16) => PrintfInfo::IS_SHORT,
        Length::Long | Length::Word => PrintfInfo::IS_LONG,
        Length::LongLong | Length::Bits(64) => PrintfInfo::IS_LONG | PrintfInfo::IS_LONG_DOUBLE,
        Length::Quad | Length::LongDouble => PrintfInfo::IS_LONG_DOUBLE,
        Length::Int | Length::Bits(_) | Length::Invalid => 0,
    }
}

/// The type of the argument the built-in conversion `conv` takes, by the
/// length bits glibc's parser set ([`length_bits`]) -- glibc's own table,
/// `printf-parsemb.c` -- or `None` for one that takes none (`%m`, `%%`, a
/// conversion glibc does not know, a format that ends first).
pub(crate) const fn builtin_arg_type(conv: u8, bits: u16) -> Option<i32> {
    Some(match conv {
        b'i' | b'd' | b'u' | b'o' | b'X' | b'x' | b'B' | b'b' => {
            if bits & PrintfInfo::IS_LONG != 0 {
                PA_INT | PA_FLAG_LONG
            } else if bits & PrintfInfo::IS_SHORT != 0 {
                PA_INT | PA_FLAG_SHORT
            } else if bits & PrintfInfo::IS_CHAR != 0 {
                PA_CHAR
            } else {
                PA_INT
            }
        }
        b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'a' | b'A' => {
            if bits & PrintfInfo::IS_LONG_DOUBLE != 0 {
                PA_DOUBLE | PA_FLAG_LONG_DOUBLE
            } else {
                PA_DOUBLE
            }
        }
        b'c' => PA_CHAR,
        b'C' => PA_WCHAR,
        b's' => PA_STRING,
        b'S' => PA_WSTRING,
        b'p' => PA_POINTER,
        b'n' => PA_INT | PA_FLAG_PTR,
        _ => return None,
    })
}

/// A `*`'s argument, as glibc's parser numbers it: `*m$`'s `m - 1`, or the
/// next unnumbered argument, `*posn`, taken. `m` past `INT_MAX` is no
/// position to glibc (its `read_int`'s -1), so the next argument is taken.
fn star_arg(count: Count, posn: &mut usize, max_ref: &mut usize) -> Option<usize> {
    match count {
        Count::Given(_) => None,
        Count::Star(Star::At(m)) if m <= INT_MAX => {
            *max_ref = (*max_ref).max(m);
            Some(m.saturating_sub(1))
        }
        Count::Star(_) => {
            let i = *posn;
            *posn = posn.saturating_add(1);
            Some(i)
        }
    }
}

/// `parse_printf_format(fmt, n, argtypes)`: how many arguments `fmt` takes,
/// and in `argtypes[0..n]` the type of each the format names -- a `PA_*`
/// value, with `PA_FLAG_*` bits -- as glibc's `parse_printf_format` says.
///
/// Each specification's arguments are typed in the order glibc types them:
/// a `*` width, then a `*` precision, each a `PA_INT`, then the argument
/// its conversion takes. An argument named twice keeps the type the later
/// specification gives it (`"%d %1$s"`: a `PA_STRING`), and one no
/// specification names is left as it was. Unnumbered specifications and
/// `*`s take arguments in order; the count is the larger of how many those
/// took and the highest position the format names. An entry past `n` is
/// counted and not written. A NULL `fmt` takes nothing (glibc's would
/// fault).
///
/// # Safety
///
/// `fmt` must be NULL or a NUL-terminated string, and `argtypes` writable
/// for `n` entries (or NULL with `n` 0).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn parse_printf_format(
    fmt: *const u8,
    n: usize,
    argtypes: *mut i32,
) -> usize {
    if fmt.is_null() {
        return 0;
    }
    let put = |index: usize, ty: i32| {
        if index < n && !argtypes.is_null() {
            // SAFETY: the caller's contract: `argtypes` has `n` entries.
            unsafe { argtypes.add(index).write(ty) };
        }
    };
    // glibc's `nargs`: how many arguments the unnumbered specifications and
    // `*`s have taken.
    let mut posn = 0usize;
    let mut max_ref = 0usize;
    let mut fpos = 0usize;
    loop {
        // SAFETY: `fmt` is NUL-terminated; this walk stops at the NUL.
        let c = unsafe { *fmt.add(fpos) };
        if c == 0 {
            break;
        }
        fpos = fpos.wrapping_add(1);
        if c != b'%' {
            continue;
        }
        let raw = crate::printf::parse_spec(fmt, &mut fpos);
        // SAFETY: `parse_spec` stopped at the conversion, or at the NUL.
        let conv = unsafe { *fmt.add(fpos) };
        let a = spec_args(&raw, conv, false, &mut posn, &mut max_ref);
        if let Some(i) = a.width_arg {
            put(i, PA_INT);
        }
        if let Some(i) = a.prec_arg {
            put(i, PA_INT);
        }
        match a.ndata {
            0 => {}
            1 => put(a.data_arg, a.data_type),
            // More than one: the arginfo function types them all, asked
            // again with the room `argtypes` has from the first, as glibc
            // asks it.
            ndata => {
                if a.data_arg < n && !argtypes.is_null() {
                    let mut size = -1;
                    // SAFETY: `argtypes` has `n` entries, so `n - data_arg`
                    // from `data_arg`.
                    let at = unsafe { argtypes.add(a.data_arg) };
                    let _ = ndata;
                    // The count it answers is the one already had; glibc
                    // ignores it too.
                    let _ = arginfo(
                        conv,
                        &a.info,
                        n.saturating_sub(a.data_arg),
                        at,
                        &raw mut size,
                    );
                }
            }
        }
        if conv == 0 {
            break;
        }
        fpos = fpos.wrapping_add(1);
    }
    posn.max(max_ref)
}

/// One specification's arguments as glibc's parser (`__parse_one_specmb`)
/// assigns and types them: its `*` width's and `*` precision's, and the
/// conversion's own -- how many, from which, and the first one's type and,
/// for a user type, size.
pub(crate) struct SpecArgs {
    /// The specification as glibc's parser leaves it: a `*` width is 0 and a
    /// `*` precision -1, until the arguments are read.
    pub(crate) info: PrintfInfo,
    pub(crate) width_arg: Option<usize>,
    pub(crate) prec_arg: Option<usize>,
    /// How many arguments the conversion takes, from `data_arg`.
    pub(crate) ndata: usize,
    pub(crate) data_arg: usize,
    pub(crate) data_type: i32,
    pub(crate) size: i32,
}

/// Assign and type the arguments of `raw`, whose conversion is `conv`: the
/// `*`s first, each the next unnumbered argument (`*posn`) unless it names
/// one, then the conversion's, typed by the arginfo function registered
/// for `conv` unless there is none or it answers -1, and else by glibc's
/// own table ([`builtin_arg_type`]). `max_ref` keeps the highest position
/// named. `wide` is the wide family's `info.wide`.
pub(crate) fn spec_args(
    raw: &crate::printf::RawSpec,
    conv: u8,
    wide: bool,
    posn: &mut usize,
    max_ref: &mut usize,
) -> SpecArgs {
    let position = raw.position.filter(|&p| p <= INT_MAX);
    if let Some(p) = position {
        *max_ref = (*max_ref).max(p);
    }
    let width_arg = star_arg(raw.width, posn, max_ref);
    let prec_arg = raw
        .precision
        .and_then(|count| star_arg(count, posn, max_ref));
    let info = info_for(raw, conv, wide);
    let mut data_type = PA_INT;
    let mut size = -1;
    let registered = arginfo(conv, &info, 1, &raw mut data_type, &raw mut size)
        .and_then(|n| usize::try_from(n).ok());
    let ndata = registered.unwrap_or_else(|| match builtin_arg_type(conv, info.bits) {
        Some(ty) => {
            data_type = ty;
            1
        }
        None => 0,
    });
    let data_arg = if ndata == 0 {
        0
    } else if let Some(p) = position {
        p.saturating_sub(1)
    } else {
        let i = *posn;
        *posn = posn.saturating_add(ndata);
        i
    };
    SpecArgs {
        info,
        width_arg,
        prec_arg,
        ndata,
        data_arg,
        data_type,
        size,
    }
}

/// glibc's `struct printf_info` for `raw`, as its parser fills it: a `*`
/// width is 0 and a `*` precision -1 until the arguments are read; `pad` is
/// `'0'` for the `0` flag without `-`.
pub(crate) fn info_for(raw: &crate::printf::RawSpec, conv: u8, wide: bool) -> PrintfInfo {
    let f = &raw.flags;
    let mut bits = length_bits(raw.length);
    for (on, bit) in [
        (f.alt_form, PrintfInfo::ALT),
        (f.space_sign, PrintfInfo::SPACE),
        (f.left_align, PrintfInfo::LEFT),
        (f.force_sign, PrintfInfo::SHOWSIGN),
        (f.group, PrintfInfo::GROUP),
        (f.i18n, PrintfInfo::I18N),
        (wide, PrintfInfo::WIDE),
    ] {
        if on {
            bits |= bit;
        }
    }
    let clamp = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
    PrintfInfo {
        prec: match raw.precision {
            Some(Count::Given(p)) => clamp(p),
            None | Some(Count::Star(_)) => -1,
        },
        width: match raw.width {
            Count::Given(w) => clamp(w),
            Count::Star(_) => 0,
        },
        spec: WcharT::from(conv),
        bits,
        user: raw.user,
        pad: if f.zero_pad && !f.left_align {
            WcharT::from(b'0')
        } else {
            WcharT::from(b' ')
        },
    }
}

// ---------------------------------------------------------------------------
// Registration: conversions, modifiers and types a program defines
// ---------------------------------------------------------------------------

/// `printf_function`: format one conversion to `stream`, from `args`, an
/// array of pointers to its arguments; the count written, -1 for an error,
/// or -2 to have the conversion formatted as if nothing were registered.
pub type PrintfFunction =
    unsafe extern "C" fn(*mut u8, *const PrintfInfo, *const *const core::ffi::c_void) -> i32;
/// `printf_arginfo_size_function`: how many arguments a conversion takes,
/// their types in the array (room for the count given), and a user type's
/// size; -1 to have it typed as if nothing were registered.
pub type PrintfArginfoSizeFunction =
    unsafe extern "C" fn(*const PrintfInfo, usize, *mut i32, *mut i32) -> i32;
/// `printf_arginfo_function`: the older [`PrintfArginfoSizeFunction`],
/// without the size.
pub type PrintfArginfoFunction = unsafe extern "C" fn(*const PrintfInfo, usize, *mut i32) -> i32;
/// `printf_va_arg_function`: read one argument of a registered type from
/// the `va_list` into the memory given, the size its arginfo said.
pub type PrintfVaArgFunction =
    unsafe extern "C" fn(*mut core::ffi::c_void, *mut crate::printf::VaList);

use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicUsize, Ordering};

/// The registered handlers and arginfo functions, by conversion character:
/// each an address, 0 for none.
static FUNCTIONS: [AtomicUsize; 256] = [const { AtomicUsize::new(0) }; 256];
static ARGINFOS: [AtomicUsize; 256] = [const { AtomicUsize::new(0) }; 256];
/// Whether an arginfo is the sized form (`register_printf_specifier`) or
/// the older one (`register_printf_function`), which is called without the
/// size -- not, as glibc does, through a cast that hands it one argument
/// more than it takes.
static SIZED: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];
/// The readers of the registered types, by type less `PA_LAST`.
static VA_ARGS: [AtomicUsize; 0x100 - 8] = [const { AtomicUsize::new(0) }; 0x100 - 8];
static NEXT_TYPE: AtomicI32 = AtomicI32::new(PA_LAST);
/// The registered modifiers, in the order registered: published one at a
/// time, never removed.
static MODIFIERS: [AtomicPtr<Modifier>; 16] = [const { AtomicPtr::new(core::ptr::null_mut()) }; 16];
static MODIFIER_COUNT: AtomicUsize = AtomicUsize::new(0);
/// Anything registered. From then on every format takes glibc's
/// positional route, as glibc's printf does once its tables exist.
static ANY: AtomicBool = AtomicBool::new(false);
/// Held while registering: the registration functions may be called from
/// any thread, and a handler and its arginfo are set together.
static LOCK: AtomicBool = AtomicBool::new(false);

/// A registered modifier: its text -- as bytes, glibc refusing any
/// character past 255 -- and the bit it sets in `info.user`.
#[repr(C)]
struct Modifier {
    bit: u16,
    len: usize,
    /// `len` bytes, allocated with the record, after it.
    text: *const u8,
}

/// [`LOCK`], held for a scope.
struct Registering;

impl Registering {
    fn new() -> Self {
        while LOCK
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        Self
    }
}

impl Drop for Registering {
    fn drop(&mut self) {
        LOCK.store(false, Ordering::Release);
    }
}

/// Whether anything is registered: the engine's question, every call.
pub(crate) fn any_registered() -> bool {
    ANY.load(Ordering::Acquire)
}

/// The handler registered for `conv`.
pub(crate) fn handler(conv: u8) -> Option<PrintfFunction> {
    let addr = FUNCTIONS.get(usize::from(conv))?.load(Ordering::Acquire);
    // SAFETY: a nonzero entry is the address of a `PrintfFunction`, stored
    // by `register_printf_specifier` from one.
    (addr != 0).then(|| unsafe { core::mem::transmute::<usize, PrintfFunction>(addr) })
}

/// The arginfo function registered for `conv`, called: its answer, or
/// `None` without one. `size` is written only by the sized form.
pub(crate) fn arginfo(
    conv: u8,
    info: &PrintfInfo,
    n: usize,
    argtypes: *mut i32,
    size: *mut i32,
) -> Option<i32> {
    let i = usize::from(conv);
    let addr = ARGINFOS.get(i)?.load(Ordering::Acquire);
    if addr == 0 {
        return None;
    }
    let sized = SIZED.get(i).is_some_and(|s| s.load(Ordering::Acquire));
    // SAFETY: a nonzero entry is the address of the arginfo function of the
    // form `SIZED` records, stored by the registering call from one; the
    // pointers are the caller's, with room for `n` types and a size.
    Some(unsafe {
        if sized {
            core::mem::transmute::<usize, PrintfArginfoSizeFunction>(addr)(info, n, argtypes, size)
        } else {
            core::mem::transmute::<usize, PrintfArginfoFunction>(addr)(info, n, argtypes)
        }
    })
}

/// The reader registered for type `ty`.
pub(crate) fn va_arg_reader(ty: i32) -> Option<PrintfVaArgFunction> {
    let i = usize::try_from(ty.checked_sub(PA_LAST)?).ok()?;
    let addr = VA_ARGS.get(i)?.load(Ordering::Acquire);
    // SAFETY: a nonzero entry is the address of a `PrintfVaArgFunction`,
    // stored by `register_printf_type` from one.
    (addr != 0).then(|| unsafe { core::mem::transmute::<usize, PrintfVaArgFunction>(addr) })
}

/// The registered modifier at `fmt[pos..]`, as glibc matches it: the
/// longest whose text the format continues with -- the newest of equally
/// long ones -- its bit and its length. `None` when none matches, or none
/// is registered.
pub(crate) fn match_modifier(fmt: *const u8, pos: usize) -> Option<(u16, usize)> {
    let count = MODIFIER_COUNT.load(Ordering::Acquire);
    let mut best: Option<(u16, usize)> = None;
    for slot in MODIFIERS.iter().take(count).rev() {
        let m = slot.load(Ordering::Acquire);
        if m.is_null() {
            continue;
        }
        // SAFETY: a published record lives for the process, its text
        // `len` bytes; the format is NUL-terminated and is read only while
        // it matches, so never past its NUL (no text byte is 0).
        let (bit, len, matches) = unsafe {
            let m = &*m;
            let mut k = 0usize;
            while k < m.len && *fmt.add(pos.wrapping_add(k)) == *m.text.add(k) {
                k = k.wrapping_add(1);
            }
            (m.bit, m.len, k == m.len)
        };
        if matches && best.is_none_or(|(_, l)| len > l) {
            best = Some((bit, len));
        }
    }
    best
}

/// `register_printf_specifier(spec, func, arginfo)`: have `func` format the
/// conversion `spec`, its arguments typed by `arginfo` -- glibc's. Either
/// may be NULL: without `func` the conversion is formatted as built in;
/// without `arginfo` its arguments are typed so. `EINVAL` for a `spec` past
/// 255. From the first registration on, every format is read as glibc's
/// positional pass reads one -- as glibc's are.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn register_printf_specifier(
    spec: i32,
    func: Option<PrintfFunction>,
    arginfo: Option<PrintfArginfoSizeFunction>,
) -> i32 {
    register(
        spec,
        func.map_or(0, |f| f as usize),
        arginfo.map_or(0, |f| f as usize),
        true,
    )
}

/// `register_printf_function(spec, func, arginfo)`: the older
/// [`register_printf_specifier`], whose arginfo takes no size -- so it
/// cannot type a registered user type.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn register_printf_function(
    spec: i32,
    func: Option<PrintfFunction>,
    arginfo: Option<PrintfArginfoFunction>,
) -> i32 {
    register(
        spec,
        func.map_or(0, |f| f as usize),
        arginfo.map_or(0, |f| f as usize),
        false,
    )
}

fn register(spec: i32, func: usize, arginfo: usize, sized: bool) -> i32 {
    let Some(i) = usize::try_from(spec).ok().filter(|&i| i <= 255) else {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    };
    let _held = Registering::new();
    if let (Some(f), Some(a), Some(s)) = (FUNCTIONS.get(i), ARGINFOS.get(i), SIZED.get(i)) {
        s.store(sized, Ordering::Release);
        a.store(arginfo, Ordering::Release);
        f.store(func, Ordering::Release);
    }
    ANY.store(true, Ordering::Release);
    0
}

/// `register_printf_modifier(str)`: a length modifier of the program's own
/// -- the bit it sets in `info.user` when a specification has it, or -1:
/// `EINVAL` for an empty one or a character past 255, `ENOSPC` once
/// `info.user`'s 16 bits are given out, `ENOMEM` -- glibc's. Where the
/// format continues with a registered modifier, the longest, no built-in
/// one is read.
///
/// # Safety
///
/// `str` must be a NUL-terminated wide string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn register_printf_modifier(str: *const WcharT) -> i32 {
    if str.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    let mut len = 0usize;
    loop {
        // SAFETY: `str` is NUL-terminated (the caller's contract); this
        // stops at the NUL.
        let wc = unsafe { *str.add(len) };
        if wc == 0 {
            break;
        }
        if !(0..=255).contains(&wc) {
            crate::errno::set_errno(crate::errno::EINVAL);
            return -1;
        }
        len = len.saturating_add(1);
    }
    if len == 0 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    let _held = Registering::new();
    let count = MODIFIER_COUNT.load(Ordering::Acquire);
    let Some(slot) = MODIFIERS.get(count) else {
        crate::errno::set_errno(crate::errno::ENOSPC);
        return -1;
    };
    let size = core::mem::size_of::<Modifier>().saturating_add(len);
    let mem = crate::malloc::malloc(size);
    if mem.is_null() {
        crate::errno::set_errno(crate::errno::ENOMEM);
        return -1;
    }
    let Ok(shift) = u32::try_from(count) else {
        return -1;
    };
    let bit = 1u16 << shift;
    // SAFETY: `mem` holds a `Modifier` and then `len` bytes; `malloc`'s
    // memory is aligned for any type. Each character is in 0..=255.
    unsafe {
        let text = mem.add(core::mem::size_of::<Modifier>());
        for k in 0..len {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            text.add(k).write(*str.add(k) as u8);
        }
        mem.cast::<Modifier>().write(Modifier { bit, len, text });
    }
    slot.store(mem.cast(), Ordering::Release);
    MODIFIER_COUNT.store(count.saturating_add(1), Ordering::Release);
    ANY.store(true, Ordering::Release);
    i32::from(bit)
}

/// Whether any modifier is registered: [`crate::printf::parse_spec`]'s
/// question.
pub(crate) fn modifiers_registered() -> bool {
    MODIFIER_COUNT.load(Ordering::Acquire) != 0
}

/// `register_printf_type(fct)`: a type of the program's own, `fct` reading
/// one from a `va_list` -- the number its arginfo functions give for it
/// (`PA_LAST` and up), or -1, `ENOSPC`, once the 248 numbers are given out
/// -- glibc's.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn register_printf_type(fct: Option<PrintfVaArgFunction>) -> i32 {
    let _held = Registering::new();
    let ty = NEXT_TYPE.load(Ordering::Acquire);
    let Some(slot) = ty
        .checked_sub(PA_LAST)
        .and_then(|i| usize::try_from(i).ok())
        .and_then(|i| VA_ARGS.get(i))
    else {
        crate::errno::set_errno(crate::errno::ENOSPC);
        return -1;
    };
    slot.store(fct.map_or(0, |f| f as usize), Ordering::Release);
    NEXT_TYPE.store(ty.saturating_add(1), Ordering::Release);
    ANY.store(true, Ordering::Release);
    ty
}

/// `printf_size_info(info, n, argtypes)`: [`printf_size`]'s arginfo -- one
/// `double`, or `long double` for `L`.
///
/// # Safety
///
/// `info` must point to a `printf_info`, and `argtypes` have room for `n`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn printf_size_info(
    info: *const PrintfInfo,
    n: usize,
    argtypes: *mut i32,
) -> i32 {
    if n >= 1 && !argtypes.is_null() && !info.is_null() {
        // SAFETY: the caller's contract.
        let long = unsafe { (*info).bits } & PrintfInfo::IS_LONG_DOUBLE != 0;
        // SAFETY: as above: room for one.
        unsafe {
            argtypes.write(PA_DOUBLE | if long { PA_FLAG_LONG_DOUBLE } else { 0 });
        }
    }
    1
}

/// `printf_size(fp, info, args)`: a number with the unit its size calls
/// for, as glibc's: divided by 1024 -- or for an upper-case conversion by
/// 1000 -- until it is under that or the units run out, and written as `%f`
/// is (precision 3 when none is given) with the unit's letter after it,
/// ` kmgtpezy` or ` KMGTPEZY`: a space for a number that was not divided.
/// `nan` and `inf` stand alone. A handler, for [`register_printf_specifier`]
/// with [`printf_size_info`].
///
/// # Safety
///
/// `fp` must be a stream, `info` a `printf_info`, and `args[0]` point to a
/// `double` -- a `long double` for `L`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn printf_size(
    fp: *mut u8,
    info: *const PrintfInfo,
    args: *const *const core::ffi::c_void,
) -> i32 {
    if fp.is_null() || info.is_null() || args.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    // SAFETY: the caller's contract.
    let info = unsafe { *info };
    // SAFETY: as above.
    let value = unsafe { *args };
    if value.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    let upper = u8::try_from(info.spec).is_ok_and(|c| c.is_ascii_uppercase());
    let units: &[u8; 9] = if upper { b" KMGTPEZY" } else { b" kmgtpezy" };
    let long = info.bits & PrintfInfo::IS_LONG_DOUBLE != 0;
    // SAFETY: `args[0]` points to the value, of the type `L` says.
    let (scaled, tag) = unsafe { crate::printf::size_scale(value, long, upper) };
    let unit = units.get(tag).copied().unwrap_or(b' ');
    // SAFETY: `fp` is a stream (the caller's contract).
    unsafe { crate::printf::printf_size_write(fp, &info, scaled, unit) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// glibc's `struct printf_info`, as measured: 20 bytes, align 4, the
    /// bitfields at 12, `user` at 14, `pad` at 16.
    #[test]
    fn printf_info_is_glibcs() {
        assert_eq!(core::mem::size_of::<PrintfInfo>(), 20);
        assert_eq!(core::mem::align_of::<PrintfInfo>(), 4);
        assert_eq!(core::mem::offset_of!(PrintfInfo, spec), 8);
        assert_eq!(core::mem::offset_of!(PrintfInfo, bits), 12);
        assert_eq!(core::mem::offset_of!(PrintfInfo, user), 14);
        assert_eq!(core::mem::offset_of!(PrintfInfo, pad), 16);
    }

    /// Every line of `printf_parse_oracle.txt`
    /// (`posix/tools/oracle/printf_parse_harness.py`): glibc's
    /// `parse_printf_format` of each format, its return value and the 16
    /// entries of `argtypes` it was given, each -1 before the call.
    #[test]
    fn parse_printf_format_is_glibcs() {
        let oracle = include_str!("printf_parse_oracle.txt");
        let mut failures = std::vec::Vec::new();
        let mut compared = 0usize;
        for line in oracle.lines() {
            let (left, right) = line.split_once(" | ").expect("line");
            let (fmt_hex, n_text) = left.split_once(' ').expect("format and n");
            let mut fmt: std::vec::Vec<u8> = if fmt_hex == "-" {
                std::vec::Vec::new()
            } else {
                (0..fmt_hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&fmt_hex[i..i + 2], 16).expect("hex"))
                    .collect()
            };
            fmt.push(0);
            let n: usize = n_text.parse().expect("n");
            let (ret_text, types_text) = right.split_once(' ').expect("return and types");
            let want_ret: usize = ret_text.parse().expect("return");
            let want: std::vec::Vec<i32> = types_text
                .split(',')
                .map(|t| t.parse().expect("type"))
                .collect();
            let mut got = [-1i32; 16];
            // SAFETY: a NUL-terminated format, and 16 entries for `n <= 16`.
            let got_ret = unsafe { parse_printf_format(fmt.as_ptr(), n, got.as_mut_ptr()) };
            compared += 1;
            if got_ret != want_ret || got[..] != want[..] {
                failures.push(std::format!(
                    "{:?} n={n}: glibc {want_ret} {want:?}, here {got_ret} {got:?}",
                    std::string::String::from_utf8_lossy(&fmt[..fmt.len() - 1])
                ));
            }
        }
        assert_eq!(compared, 521, "the whole oracle");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// A NULL format takes nothing, rather than faulting.
    #[test]
    fn a_null_format_takes_nothing() {
        let mut t = [-1i32; 4];
        // SAFETY: a NULL format is this function's to refuse.
        assert_eq!(
            unsafe { parse_printf_format(core::ptr::null(), 4, t.as_mut_ptr()) },
            0
        );
        assert_eq!(t, [-1; 4]);
    }

    // -- registration, in a process of its own --

    use core::ffi::c_void;
    use std::sync::atomic::{AtomicI32, Ordering as AtOrd};

    /// Set by [`registration_is_glibcs`] for the child it runs.
    const CHILD_ENV: &str = "POSIX_PRINTF_REGISTRATION_CHILD";

    static USER_TYPE: AtomicI32 = AtomicI32::new(-1);
    static R_BIT: AtomicI32 = AtomicI32::new(-1);

    fn write(fp: *mut u8, text: &str) -> i32 {
        let n = crate::stdio::write_stream(fp, text.as_ptr(), text.len());
        i32::try_from(n).unwrap_or(-1)
    }

    /// The harness's `pair_va_arg`: a `{ long; double; }`.
    unsafe extern "C" fn pair_va_arg(mem: *mut c_void, ap: *mut crate::printf::VaList) {
        // SAFETY: `ap` is the call's `va_list`, and `mem` room for the pair.
        unsafe {
            let i = crate::printf::va_arg_int(&mut *ap);
            let d = crate::printf::va_arg_double(&mut *ap);
            mem.cast::<[u64; 2]>().write([i, d]);
        }
    }

    /// The harness's `pair_arginfo`: as many pairs as the precision, else one.
    unsafe extern "C" fn pair_arginfo(
        info: *const PrintfInfo,
        n: usize,
        types: *mut i32,
        size: *mut i32,
    ) -> i32 {
        // SAFETY: the engine's contract: `info`, and room for `n` of each.
        unsafe {
            let count = if (*info).prec >= 0 { (*info).prec } else { 1 };
            for k in 0..usize::try_from(count).unwrap_or(0).min(n) {
                types.add(k).write(USER_TYPE.load(AtOrd::Relaxed));
                size.add(k).write(16);
            }
            count
        }
    }

    /// The harness's `pair_out`.
    unsafe extern "C" fn pair_out(
        fp: *mut u8,
        info: *const PrintfInfo,
        args: *const *const c_void,
    ) -> i32 {
        // SAFETY: the engine's contract: `info`, and a pointer to each pair's
        // pointer.
        unsafe {
            let prec = (*info).prec;
            let count = if prec >= 0 { prec } else { 1 };
            let mut text = std::string::String::new();
            if prec >= 0 {
                text.push('{');
            }
            for k in 0..usize::try_from(count).unwrap_or(0) {
                let pair = *(*args.add(k)).cast::<*const [u64; 2]>();
                let [i, d] = *pair;
                #[allow(clippy::cast_possible_truncation)]
                let hundredths = (f64::from_bits(d) * 100.0) as i64;
                text.push_str(&std::format!(
                    "{}({},{hundredths})",
                    if k > 0 { "," } else { "" },
                    i.cast_signed()
                ));
            }
            if prec >= 0 {
                text.push('}');
            }
            write(fp, &text)
        }
    }

    /// The harness's `y_arginfo`: one `int`.
    unsafe extern "C" fn y_arginfo(
        _info: *const PrintfInfo,
        n: usize,
        types: *mut i32,
        _size: *mut i32,
    ) -> i32 {
        if n >= 1 {
            // SAFETY: room for `n`.
            unsafe { types.write(PA_INT) };
        }
        1
    }

    /// The harness's `y_out`: what it was given.
    unsafe extern "C" fn y_out(
        fp: *mut u8,
        info: *const PrintfInfo,
        args: *const *const c_void,
    ) -> i32 {
        // SAFETY: the engine's contract.
        let (info, v) = unsafe { (*info, *(*args).cast::<i32>()) };
        let mut flags = std::string::String::new();
        for (bit, c) in [
            (PrintfInfo::ALT, '#'),
            (PrintfInfo::SPACE, 's'),
            (PrintfInfo::LEFT, '-'),
            (PrintfInfo::SHOWSIGN, '+'),
            (PrintfInfo::GROUP, '\''),
            (PrintfInfo::I18N, 'I'),
        ] {
            if info.bits & bit != 0 {
                flags.push(c);
            }
        }
        if info.pad == 0x30 {
            flags.push('0');
        }
        for (bit, c) in [
            (PrintfInfo::IS_LONG_DOUBLE, 'L'),
            (PrintfInfo::IS_LONG, 'l'),
            (PrintfInfo::IS_SHORT, 'h'),
            (PrintfInfo::IS_CHAR, 'c'),
            (PrintfInfo::WIDE, 'w'),
        ] {
            if info.bits & bit != 0 {
                flags.push(c);
            }
        }
        let user = if i32::from(info.user) == R_BIT.load(AtOrd::Relaxed) {
            1
        } else {
            i32::from(info.user)
        };
        write(
            fp,
            &std::format!("<Y{v};w{};p{};u{user};{flags}>", info.width, info.prec),
        )
    }

    /// The harness's `d_out`: -2 unless `#`.
    unsafe extern "C" fn d_out(
        fp: *mut u8,
        info: *const PrintfInfo,
        args: *const *const c_void,
    ) -> i32 {
        // SAFETY: the engine's contract.
        unsafe {
            let info = *info;
            if info.bits & PrintfInfo::ALT == 0 {
                return -2;
            }
            if info.bits & PrintfInfo::IS_LONG != 0 {
                write(fp, &std::format!("#d={}", *(*args).cast::<i64>()))
            } else {
                write(fp, &std::format!("#d={}", *(*args).cast::<i32>()))
            }
        }
    }

    /// A C `%a`, as Python's `float.hex` writes it, or `nan`, `inf`, `-inf`.
    fn hexfloat(text: &str) -> f64 {
        match text {
            "nan" => return f64::NAN,
            "inf" => return f64::INFINITY,
            "-inf" => return f64::NEG_INFINITY,
            _ => {}
        }
        let (negative, rest) = text.strip_prefix('-').map_or((false, text), |r| (true, r));
        let rest = rest.strip_prefix("0x").expect("0x");
        let (mantissa, exp) = rest.split_once('p').expect("p");
        let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let digits = std::format!("{int}{frac}");
        let m = u64::from_str_radix(&digits, 16).expect("hex digits");
        let e: i32 =
            exp.parse::<i32>().expect("exponent") - 4 * i32::try_from(frac.len()).expect("short");
        #[allow(clippy::cast_precision_loss)]
        let v = m as f64 * 2f64.powi(e);
        if negative { -v } else { v }
    }

    /// glibc's printf with `%Y`, `%P` (a registered type), a `%d` that hands
    /// the plain conversion back, `printf_size` on `%b` and `%B`, and the
    /// modifier `R` registered: every line of `printf_reg_oracle.txt`
    /// (`posix/tools/oracle/printf_reg_harness.py`), in a process of its own
    /// -- a registration lasts as long as the process, and turns every
    /// format in it to glibc's positional pass, which the rest of this
    /// binary's tests must not see.
    #[test]
    fn registration_is_glibcs() {
        let exe = std::env::current_exe().expect("this test binary");
        let out = std::process::Command::new(exe)
            .args([
                "--ignored",
                "--exact",
                "printf_h::tests::registration_child",
                "--test-threads=1",
            ])
            .env(CHILD_ENV, "1")
            .output()
            .expect("the child runs");
        let text = std::format!(
            "{}{}",
            std::string::String::from_utf8_lossy(&out.stdout),
            std::string::String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{text}");
        assert!(
            text.contains("1 passed"),
            "the child test did not run: {text}"
        );
    }

    #[test]
    #[ignore = "registration_is_glibcs runs it, in a process of its own"]
    fn registration_child() {
        if std::env::var_os(CHILD_ENV).is_none() {
            return;
        }
        USER_TYPE.store(register_printf_type(Some(pair_va_arg)), AtOrd::Relaxed);
        let r: [WcharT; 2] = [WcharT::from(b'R'), 0];
        // SAFETY: a NUL-terminated wide string.
        R_BIT.store(
            unsafe { register_printf_modifier(r.as_ptr()) },
            AtOrd::Relaxed,
        );
        assert_eq!(USER_TYPE.load(AtOrd::Relaxed), PA_LAST);
        assert_eq!(R_BIT.load(AtOrd::Relaxed), 1);
        assert_eq!(
            register_printf_specifier(i32::from(b'P'), Some(pair_out), Some(pair_arginfo)),
            0
        );
        assert_eq!(
            register_printf_specifier(i32::from(b'Y'), Some(y_out), Some(y_arginfo)),
            0
        );
        assert_eq!(
            register_printf_specifier(i32::from(b'd'), Some(d_out), None),
            0
        );
        assert_eq!(
            register_printf_function(i32::from(b'b'), Some(printf_size), Some(printf_size_info)),
            0
        );
        assert_eq!(
            register_printf_function(i32::from(b'B'), Some(printf_size), Some(printf_size_info)),
            0
        );

        let oracle = include_str!("printf_reg_oracle.txt");
        let mut failures = std::vec::Vec::new();
        let mut compared = 0usize;
        for line in oracle.lines() {
            let (left, right) = line.split_once(" | ").expect("line");
            let (fmt_hex, args_text) = left.split_once(' ').expect("format and arguments");
            let unhex = |h: &str| -> std::vec::Vec<u8> {
                (0..h.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&h[i..i + 2], 16).expect("hex"))
                    .collect()
            };
            let mut fmt = unhex(fmt_hex);
            fmt.push(0);
            let mut ints = std::vec::Vec::new();
            let mut floats = std::vec::Vec::new();
            let mut strings: std::vec::Vec<std::vec::Vec<u8>> = std::vec::Vec::new();
            let mut store = [0x55i32; 1];
            if args_text != "-" {
                for token in args_text.split(',') {
                    let (t, v) = token.split_once(':').expect("type:value");
                    match t {
                        // An `int` as a C caller passes one here: its high
                        // half zero, which `%ld` of one reads.
                        "i" => ints.push(u64::from(v.parse::<i32>().expect("int").cast_unsigned())),
                        "d" => floats.push(hexfloat(v).to_bits()),
                        "s" => {
                            let mut s = unhex(v);
                            s.push(0);
                            ints.push(s.as_ptr() as u64);
                            strings.push(s);
                        }
                        "P" => {
                            let (i, d) = v.split_once('/').expect("i/d");
                            ints.push(i.parse::<i64>().expect("long").cast_unsigned());
                            floats.push(hexfloat(d).to_bits());
                        }
                        "n" => ints.push(store.as_mut_ptr() as u64),
                        _ => panic!("unknown argument {token}"),
                    }
                }
            }
            let mut buf = [0u8; 512];
            crate::errno::set_errno(0);
            let got = crate::printf::tests::with_valist(&ints, &floats, |ap| {
                // SAFETY: a buffer of 512, a NUL-terminated format, and the
                // arguments it names.
                unsafe { crate::printf::vsnprintf(buf.as_mut_ptr(), buf.len(), fmt.as_ptr(), ap) }
            });
            let errno = crate::errno::get_errno();
            compared += 1;
            let parts: std::vec::Vec<&str> = right.split(' ').collect();
            let want: i32 = parts[0].parse().expect("return value");
            let shown = std::string::String::from_utf8_lossy(&fmt[..fmt.len() - 1]).into_owned();
            if want < 0 {
                let want_errno: i32 = parts[1]
                    .strip_prefix("-e")
                    .and_then(|e| e.parse().ok())
                    .expect("errno");
                if got != -1 || errno != want_errno {
                    failures.push(std::format!(
                        "{shown:?}: glibc -1 errno {want_errno}, here {got} errno {errno}"
                    ));
                }
                continue;
            }
            let want_text = if parts[1] == "-" {
                std::vec::Vec::new()
            } else {
                unhex(parts[1])
            };
            let n = usize::try_from(got).unwrap_or(0).min(buf.len());
            if got != want || buf[..n] != want_text[..] {
                failures.push(std::format!(
                    "{shown:?} {args_text}: glibc {want} {:?}, here {got} {:?}",
                    std::string::String::from_utf8_lossy(&want_text),
                    std::string::String::from_utf8_lossy(&buf[..n])
                ));
            }
            if let Some(s) = parts.get(2) {
                let want_store: i32 = s.parse().expect("stored");
                if store[0] != want_store {
                    failures.push(std::format!(
                        "{shown:?}: %n stored {}, glibc {want_store}",
                        store[0]
                    ));
                }
            }
        }
        assert_eq!(compared, 84, "the whole oracle");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
