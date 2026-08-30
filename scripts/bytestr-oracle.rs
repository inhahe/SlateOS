// Oracle for `kernel/src/bytestr.rs`.
//
//   rustc -O -o /tmp/bytestr-oracle scripts/bytestr-oracle.rs && /tmp/bytestr-oracle
//
// `bytestr.rs` claims its methods behave *exactly* like their `str`
// counterparts, and a ~1500-site mechanical conversion of the kshell parser
// rides on that claim: a helper that differs in one edge case plants a bug at
// whichever site hits it, with nothing in the diff to show for it. So the
// claim has to be checked against the real implementation rather than against
// a reading of the documentation -- documentation is where the awkward edges
// (`splitn(0)` yielding nothing, `"a,".splitn(2)` yielding a trailing empty,
// `"".split(',')` yielding one empty field, `find("")` yielding `Some(0)`)
// are easiest to get confidently wrong.
//
// This runs the exact cases from `bytestr::self_test` through `std::str` and
// prints what std actually does. It lives here rather than in the kernel
// because the kernel is `no_std` and cannot link std -- which is precisely
// why the equivalence cannot be asserted in-tree and needs an external
// oracle.
//
// If you add a method to `ByteStrExt`, add its cases here too and re-run.
// Last verified 2026-08-14: all cases matched.
fn main() {
    println!("1 split_ascii_whitespace");
    println!("  '  a  bb \t c \n' -> {:?}", "  a  bb \t c \n".split_ascii_whitespace().collect::<Vec<_>>());
    println!("  '' count            -> {}", "".split_ascii_whitespace().count());
    println!("  ' \t\r\n' count     -> {}", " \t\r\n".split_ascii_whitespace().count());
    println!("  'solo' count        -> {}", "solo".split_ascii_whitespace().count());

    println!("2 split");
    println!("  'a,,b'  -> {:?}", "a,,b".split(',').collect::<Vec<_>>());
    println!("  ',a,'   -> {:?}", ",a,".split(',').collect::<Vec<_>>());
    println!("  ''      -> {:?}", "".split(',').collect::<Vec<_>>());

    println!("3 splitn");
    println!("  'a,b'.splitn(0)   -> {:?} (count {})", "a,b".splitn(0, ',').collect::<Vec<_>>(), "a,b".splitn(0, ',').count());
    println!("  'a,b,c'.splitn(1) -> {:?}", "a,b,c".splitn(1, ',').collect::<Vec<_>>());
    println!("  'a,b,c'.splitn(2) -> {:?}", "a,b,c".splitn(2, ',').collect::<Vec<_>>());
    println!("  'a,b'.splitn(9)   -> {:?}", "a,b".splitn(9, ',').collect::<Vec<_>>());
    println!("  'a,'.splitn(2)    -> {:?}", "a,".splitn(2, ',').collect::<Vec<_>>());

    println!("4 split_once / rsplit_once");
    println!("  'key=val=ue'.split_once  -> {:?}", "key=val=ue".split_once('='));
    println!("  'key=val=ue'.rsplit_once -> {:?}", "key=val=ue".rsplit_once('='));
    println!("  'nodelim'.split_once     -> {:?}", "nodelim".split_once('='));
    println!("  '=v'.split_once          -> {:?}", "=v".split_once('='));
    println!("  'k='.split_once          -> {:?}", "k=".split_once('='));

    println!("5 multi-byte delim and find");
    println!("  'a::b::c'.split_once(\"::\") -> {:?}", "a::b::c".split_once("::"));
    println!("  'abc'.find(\"bc\")  -> {:?}", "abc".find("bc"));
    println!("  'abc'.find(\"bd\")  -> {:?}", "abc".find("bd"));
    println!("  'ab'.find(\"abc\")  -> {:?}", "ab".find("abc"));
    println!("  ''.find(\"a\")      -> {:?}", "".find("a"));
    println!("  'abc'.find(\"\")    -> {:?}", "abc".find(""));
    println!("  ''.find(\"\")       -> {:?}", "".find(""));
}
