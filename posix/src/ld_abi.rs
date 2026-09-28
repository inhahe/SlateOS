//! The C calling convention for `long double`, which Rust cannot express.
//!
//! On x86-64 SysV a `long double` is class X87: an *argument* is passed in
//! memory, 16 bytes on the caller's stack in argument order, never in a
//! register, and a *result* comes back in `%st(0)`. A Rust `extern "C"` fn
//! can do neither -- given a [`LongDouble`] by value it classifies the struct
//! INTEGER and uses two general registers -- so every C entry point taking or
//! returning one is a short assembly thunk: it hands the Rust implementation
//! pointers to the stack arguments (and to a result slot) and loads the result
//! into `%st(0)` on the way out. `strtold` has done this since the ABI fix
//! (`stdlib.rs`); [`ld_c`] is the same shape, generated once per signature.
//!
//! # The stack, the same in every thunk
//!
//! At entry `%rsp` is `16n + 8` -- the return address was pushed onto an
//! aligned stack -- and the first stack argument is at `8(%rsp)`, each
//! further one 16 bytes on. A thunk that needs a result slot takes 24 bytes,
//! 16 for the slot and 8 to realign, so its `call` is made with `%rsp`
//! aligned as the ABI requires, and the stack arguments are then at
//! `32(%rsp)`, `48(%rsp)`, `64(%rsp)`. A thunk with no result slot jumps to
//! the implementation, which then sees the stack exactly as the caller left it
//! and returns straight to the caller.
//!
//! # The Rust half
//!
//! `extern "C" fn` named by the invocation (`__slate_ld_<name>` by
//! convention), taking `*const LongDouble` for each long-double argument in
//! order, then the C function's own integer and pointer arguments, then --
//! for a long-double result -- `*mut LongDouble`. The thunk moves the integer
//! registers along to make room. Only the bare-metal build defines the C
//! names: on the host the tests call the Rust implementations directly, and
//! the host's own C library owns `sinl` and the rest.

/// Define the C entry point `$c` (a string) as a thunk into the Rust
/// `extern "C" fn $rust`, for one of these shapes (`L` a long double, `I` an
/// `int`, `N` a `long`, `P` a pointer, `D` a `double`, `F` a `float`):
///
/// | kind | C signature | Rust signature |
/// |---|---|---|
/// | `l_l` | `L f(L)` | `(*const L, *mut L)` |
/// | `l_ll` | `L f(L, L)` | `(*const L, *const L, *mut L)` |
/// | `l_lll` | `L f(L, L, L)` | `(*const L, *const L, *const L, *mut L)` |
/// | `l_li` | `L f(L, I)` | `(*const L, i32, *mut L)` |
/// | `l_ln` | `L f(L, N)` | `(*const L, i64, *mut L)` |
/// | `l_lp` | `L f(L, P)` | `(*const L, P, *mut L)` |
/// | `l_llp` | `L f(L, L, P)` | `(*const L, *const L, P, *mut L)` |
/// | `l_p` | `L f(P)` | `(P, *mut L)` |
/// | `v_lpp` | `void f(L, P, P)` | `(*const L, P, P)` |
/// | `i_l` | `I f(L)` (or `N f(L)`) | `(*const L) -> I` |
/// | `d_dl` | `D f(D, L)` | `(*const L, D) -> D` |
/// | `f_fl` | `F f(F, L)` | `(*const L, F) -> F` |
#[macro_export]
#[doc(hidden)]
macro_rules! ld_c {
    (@thunk $c:literal, $($body:expr),* $(,)?) => {
        #[cfg(target_os = "none")]
        core::arch::global_asm!(
            concat!(".global ", $c),
            concat!(".type ", $c, ", @function"),
            concat!($c, ":"),
            $($body,)*
            concat!(".size ", $c, ", . - ", $c),
        );
    };
    (l_l $c:literal => $rust:ident) => {
        $crate::ld_c!(@thunk $c,
            "sub rsp, 24",
            "lea rdi, [rsp + 32]",
            "mov rsi, rsp",
            concat!("call ", stringify!($rust)),
            "fld tbyte ptr [rsp]",
            "add rsp, 24",
            "ret",
        );
    };
    (l_ll $c:literal => $rust:ident) => {
        $crate::ld_c!(@thunk $c,
            "sub rsp, 24",
            "lea rdi, [rsp + 32]",
            "lea rsi, [rsp + 48]",
            "mov rdx, rsp",
            concat!("call ", stringify!($rust)),
            "fld tbyte ptr [rsp]",
            "add rsp, 24",
            "ret",
        );
    };
    (l_lll $c:literal => $rust:ident) => {
        $crate::ld_c!(@thunk $c,
            "sub rsp, 24",
            "lea rdi, [rsp + 32]",
            "lea rsi, [rsp + 48]",
            "lea rdx, [rsp + 64]",
            "mov rcx, rsp",
            concat!("call ", stringify!($rust)),
            "fld tbyte ptr [rsp]",
            "add rsp, 24",
            "ret",
        );
    };
    // The integer (or pointer) arrives in rdi; it moves to rsi so that rdi
    // can carry the long double's address.
    (l_li $c:literal => $rust:ident) => {
        $crate::ld_c!(@one_int $c => $rust, "mov esi, edi");
    };
    (l_ln $c:literal => $rust:ident) => {
        $crate::ld_c!(@one_int $c => $rust, "mov rsi, rdi");
    };
    (l_lp $c:literal => $rust:ident) => {
        $crate::ld_c!(@one_int $c => $rust, "mov rsi, rdi");
    };
    (@one_int $c:literal => $rust:ident, $mov:literal) => {
        $crate::ld_c!(@thunk $c,
            $mov,
            "sub rsp, 24",
            "lea rdi, [rsp + 32]",
            "mov rdx, rsp",
            concat!("call ", stringify!($rust)),
            "fld tbyte ptr [rsp]",
            "add rsp, 24",
            "ret",
        );
    };
    // remquol(x, y, quo): quo arrives in rdi, goes third.
    (l_llp $c:literal => $rust:ident) => {
        $crate::ld_c!(@thunk $c,
            "mov rdx, rdi",
            "sub rsp, 24",
            "lea rdi, [rsp + 32]",
            "lea rsi, [rsp + 48]",
            "mov rcx, rsp",
            concat!("call ", stringify!($rust)),
            "fld tbyte ptr [rsp]",
            "add rsp, 24",
            "ret",
        );
    };
    // nanl(tag): no long-double argument; the pointer stays in rdi.
    (l_p $c:literal => $rust:ident) => {
        $crate::ld_c!(@thunk $c,
            "sub rsp, 24",
            "mov rsi, rsp",
            concat!("call ", stringify!($rust)),
            "fld tbyte ptr [rsp]",
            "add rsp, 24",
            "ret",
        );
    };
    // sincosl(x, sin, cos): the two pointers move up one register; no
    // result slot, so a tail jump.
    (v_lpp $c:literal => $rust:ident) => {
        $crate::ld_c!(@thunk $c,
            "mov rdx, rsi",
            "mov rsi, rdi",
            "lea rdi, [rsp + 8]",
            concat!("jmp ", stringify!($rust)),
        );
    };
    // An integer result comes back in rax on its own: a tail jump.
    (i_l $c:literal => $rust:ident) => {
        $crate::ld_c!(@thunk $c,
            "lea rdi, [rsp + 8]",
            concat!("jmp ", stringify!($rust)),
        );
    };
    // nexttoward(x, y) and nexttowardf: x stays in xmm0, y's address goes
    // in rdi, the result comes back in xmm0.
    (d_dl $c:literal => $rust:ident) => {
        $crate::ld_c!(i_l $c => $rust);
    };
    (f_fl $c:literal => $rust:ident) => {
        $crate::ld_c!(i_l $c => $rust);
    };
}
