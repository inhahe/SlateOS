//! `ask.c`: the dialogs, and the messages that travel the same way.
//!
//! The library never reads the terminal or writes to it. When it needs a
//! number, a yes or no, a string or a menu choice -- and when it has
//! something to say -- it builds an [`Ask`] and hands it to the program's
//! callback ([`Context::set_ask`]), which fills in the result.

use std::rc::Rc;

use crate::context::Context;
use crate::errno::EINVAL;
use crate::{LABEL_FL_INCHARS_PARTNO, Sector};

/// `enum fdisk_asktype`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskType {
    /// `FDISK_ASKTYPE_NONE`.
    None,
    /// A number in a range.
    Number,
    /// An offset: a number, or `+size` relative to a base.
    Offset,
    /// A warning with an errno.
    Warn,
    /// A warning.
    Warnx,
    /// An informational message.
    Info,
    /// Yes or no.
    YesNo,
    /// A string.
    String,
    /// A key from a menu.
    Menu,
}

/// `struct ask_menuitem`.
#[derive(Clone, Debug)]
pub struct MenuItem {
    /// The key that selects it.
    pub key: i32,
    /// Its name.
    pub name: &'static str,
    /// What it does.
    pub desc: std::borrow::Cow<'static, str>,
}

/// `struct ask_number`: [`AskType::Number`] and [`AskType::Offset`].
#[derive(Clone, Debug, Default)]
pub struct AskNumber {
    /// The highest acceptable.
    pub hig: u64,
    /// The lowest acceptable.
    pub low: u64,
    /// The default.
    pub dfl: u64,
    /// The answer.
    pub result: u64,
    /// What a relative answer is relative to.
    pub base: u64,
    /// Bytes per unit, for a size given with a suffix.
    pub unit: u64,
    /// The acceptable numbers as text (`1,3,5-10`), when not a range.
    pub range: Option<Vec<u8>>,
    /// The answer was relative (`+100M`).
    pub relative: bool,
    /// Partitions are letters (BSD).
    pub inchars: bool,
    /// `-N` means N below the high limit.
    pub wrap_negative: bool,
}

/// The per-type part of an [`Ask`] (`ask->data`).
#[derive(Clone, Debug)]
pub enum AskData {
    /// Nothing asked yet.
    None,
    /// A number or an offset.
    Number(AskNumber),
    /// A message, and the errno for [`AskType::Warn`].
    Print {
        /// The message.
        mesg: Vec<u8>,
        /// The errno, or 0.
        errnum: i32,
    },
    /// Yes (1) or no (0).
    YesNo(i32),
    /// The string answered.
    String(Option<Vec<u8>>),
    /// A menu: its items, the default key and the answer.
    Menu {
        /// The default key.
        dfl: i32,
        /// The key chosen.
        result: i32,
        /// The items, in order.
        items: Vec<MenuItem>,
    },
}

/// `struct fdisk_ask`.
#[derive(Clone, Debug)]
pub struct Ask {
    /// What is asked.
    pub ty: AskType,
    /// The question.
    pub query: Option<Vec<u8>>,
    /// The type-specific part.
    pub data: AskData,
}

/// The program's dialog callback (`ask_cb` with its `ask_data`): answers
/// `ask` by filling in its result, and returns 0, or a negative errno
/// (`-ECANCELED` when the user gave up).
pub type AskCallback = Rc<dyn Fn(&mut Context, &mut Ask) -> i32>;

impl Ask {
    /// `fdisk_new_ask ()` with `fdisk_ask_set_type`.
    #[must_use]
    pub fn new(ty: AskType) -> Self {
        let data = match ty {
            AskType::Number | AskType::Offset => AskData::Number(AskNumber::default()),
            AskType::Warn | AskType::Warnx | AskType::Info => AskData::Print {
                mesg: Vec::new(),
                errnum: 0,
            },
            AskType::YesNo => AskData::YesNo(0),
            AskType::String => AskData::String(None),
            AskType::Menu => AskData::Menu {
                dfl: 0,
                result: 0,
                items: Vec::new(),
            },
            AskType::None => AskData::None,
        };
        Self {
            ty,
            query: None,
            data,
        }
    }

    /// `fdisk_ask_get_query`.
    #[must_use]
    pub fn get_query(&self) -> Option<&[u8]> {
        self.query.as_deref()
    }

    /// `fdisk_ask_set_query`.
    pub fn set_query(&mut self, q: &[u8]) {
        self.query = Some(q.to_vec());
    }

    /// `fdisk_ask_get_type`.
    #[must_use]
    pub fn get_type(&self) -> AskType {
        self.ty
    }

    /// The number part; a default one for any other type, as C's union
    /// would read.
    #[must_use]
    pub fn num(&self) -> AskNumber {
        match &self.data {
            AskData::Number(n) => n.clone(),
            _ => AskNumber::default(),
        }
    }

    fn num_mut(&mut self) -> Option<&mut AskNumber> {
        match &mut self.data {
            AskData::Number(n) => Some(n),
            _ => None,
        }
    }

    /// `fdisk_ask_number_get_range`.
    #[must_use]
    pub fn number_get_range(&self) -> Option<Vec<u8>> {
        self.num().range
    }

    /// `fdisk_ask_number_set_range`.
    pub fn number_set_range(&mut self, range: &[u8]) {
        if let Some(n) = self.num_mut() {
            n.range = Some(range.to_vec());
        }
    }

    /// `fdisk_ask_number_get_default`.
    #[must_use]
    pub fn number_get_default(&self) -> u64 {
        self.num().dfl
    }

    /// `fdisk_ask_number_set_default`.
    pub fn number_set_default(&mut self, dflt: u64) {
        if let Some(n) = self.num_mut() {
            n.dfl = dflt;
        }
    }

    /// `fdisk_ask_number_get_low`.
    #[must_use]
    pub fn number_get_low(&self) -> u64 {
        self.num().low
    }

    /// `fdisk_ask_number_set_low`.
    pub fn number_set_low(&mut self, low: u64) {
        if let Some(n) = self.num_mut() {
            n.low = low;
        }
    }

    /// `fdisk_ask_number_get_high`.
    #[must_use]
    pub fn number_get_high(&self) -> u64 {
        self.num().hig
    }

    /// `fdisk_ask_number_set_high`.
    pub fn number_set_high(&mut self, high: u64) {
        if let Some(n) = self.num_mut() {
            n.hig = high;
        }
    }

    /// `fdisk_ask_number_get_result`.
    #[must_use]
    pub fn number_get_result(&self) -> u64 {
        self.num().result
    }

    /// `fdisk_ask_number_set_result`.
    pub fn number_set_result(&mut self, result: u64) -> i32 {
        if let Some(n) = self.num_mut() {
            n.result = result;
        }
        0
    }

    /// `fdisk_ask_number_get_base`.
    #[must_use]
    pub fn number_get_base(&self) -> u64 {
        self.num().base
    }

    /// `fdisk_ask_number_set_base`.
    pub fn number_set_base(&mut self, base: u64) {
        if let Some(n) = self.num_mut() {
            n.base = base;
        }
    }

    /// `fdisk_ask_number_get_unit`: bytes per unit.
    #[must_use]
    pub fn number_get_unit(&self) -> u64 {
        self.num().unit
    }

    /// `fdisk_ask_number_set_unit`.
    pub fn number_set_unit(&mut self, unit: u64) {
        if let Some(n) = self.num_mut() {
            n.unit = unit;
        }
    }

    /// `fdisk_ask_number_is_relative`.
    #[must_use]
    pub fn number_is_relative(&self) -> bool {
        self.num().relative
    }

    /// `fdisk_ask_number_set_relative`.
    pub fn number_set_relative(&mut self, relative: bool) {
        if let Some(n) = self.num_mut() {
            n.relative = relative;
        }
    }

    /// `fdisk_ask_number_is_wrap_negative`.
    #[must_use]
    pub fn number_is_wrap_negative(&self) -> bool {
        self.num().wrap_negative
    }

    /// `fdisk_ask_number_set_wrap_negative`.
    pub fn number_set_wrap_negative(&mut self, wrap: bool) {
        if let Some(n) = self.num_mut() {
            n.wrap_negative = wrap;
        }
    }

    /// `fdisk_ask_number_inchars`: partitions are letters.
    #[must_use]
    pub fn number_inchars(&self) -> bool {
        self.num().inchars
    }

    /// `fdisk_ask_string_get_result`.
    #[must_use]
    pub fn string_get_result(&self) -> Option<&[u8]> {
        match &self.data {
            AskData::String(r) => r.as_deref(),
            _ => None,
        }
    }

    /// `fdisk_ask_string_set_result`.
    pub fn string_set_result(&mut self, result: Vec<u8>) {
        self.data = AskData::String(Some(result));
    }

    /// `fdisk_ask_yesno_get_result`: 1 for yes.
    #[must_use]
    pub fn yesno_get_result(&self) -> i32 {
        match self.data {
            AskData::YesNo(r) => r,
            _ => 0,
        }
    }

    /// `fdisk_ask_yesno_set_result`.
    pub fn yesno_set_result(&mut self, result: i32) {
        self.data = AskData::YesNo(result);
    }

    /// `fdisk_ask_menu_set_default`.
    pub fn menu_set_default(&mut self, key: i32) {
        if let AskData::Menu { dfl, .. } = &mut self.data {
            *dfl = key;
        }
    }

    /// `fdisk_ask_menu_get_default`.
    #[must_use]
    pub fn menu_get_default(&self) -> i32 {
        match self.data {
            AskData::Menu { dfl, .. } => dfl,
            _ => 0,
        }
    }

    /// `fdisk_ask_menu_set_result`.
    pub fn menu_set_result(&mut self, key: i32) -> i32 {
        if let AskData::Menu { result, .. } = &mut self.data {
            *result = key;
        }
        0
    }

    /// `fdisk_ask_menu_get_result`.
    #[must_use]
    pub fn menu_get_result(&self) -> i32 {
        match self.data {
            AskData::Menu { result, .. } => result,
            _ => 0,
        }
    }

    /// `fdisk_ask_menu_get_item (ask, idx, ...)`: `None` past the end.
    #[must_use]
    pub fn menu_get_item(&self, idx: usize) -> Option<&MenuItem> {
        match &self.data {
            AskData::Menu { items, .. } => items.get(idx),
            _ => None,
        }
    }

    /// `fdisk_ask_menu_get_nitems`.
    #[must_use]
    pub fn menu_get_nitems(&self) -> usize {
        match &self.data {
            AskData::Menu { items, .. } => items.len(),
            _ => 0,
        }
    }

    /// `fdisk_ask_menu_add_item`.
    pub fn menu_add_item(
        &mut self,
        key: i32,
        name: &'static str,
        desc: impl Into<std::borrow::Cow<'static, str>>,
    ) {
        if let AskData::Menu { items, .. } = &mut self.data {
            items.push(MenuItem {
                key,
                name,
                desc: desc.into(),
            });
        }
    }

    /// `fdisk_ask_print_get_errno`.
    #[must_use]
    pub fn print_get_errno(&self) -> i32 {
        match self.data {
            AskData::Print { errnum, .. } => errnum,
            _ => 0,
        }
    }

    /// `fdisk_ask_print_get_mesg`.
    #[must_use]
    pub fn print_get_mesg(&self) -> Option<&[u8]> {
        match &self.data {
            AskData::Print { mesg, .. } => Some(mesg),
            _ => None,
        }
    }
}

/// `mk_string_list`: the partition numbers asked for, as `1,3,5-10`, built
/// as upstream builds it into a `BUFSIZ` buffer. `None` when it would not
/// fit, as `snprintf` reports there.
struct RangeList {
    out: Vec<u8>,
    begin: usize,
    run: usize,
    inchar: bool,
}

/// `BUFSIZ`, glibc's: the size of the buffer the list is built in.
const BUFSIZ: usize = 8192;

impl RangeList {
    fn tochar(n: usize) -> u8 {
        // `'a' + num - 1`: partitions past `z` are not letters, but upstream
        // prints whatever byte that is.
        let b = n.wrapping_add(usize::from(b'a')).wrapping_sub(1);
        b.to_le_bytes().first().copied().unwrap_or(b'?')
    }

    fn piece(&self) -> Vec<u8> {
        let b = self.begin;
        let r = self.run;
        let one = |n: usize| -> Vec<u8> {
            if self.inchar {
                vec![Self::tochar(n)]
            } else {
                n.to_string().into_bytes()
            }
        };
        let mut v = one(b);
        if r == 1 {
            v.push(b',');
            v.extend(one(b.wrapping_add(1)));
        } else if r > 1 {
            v.push(b'-');
            v.extend(one(b.wrapping_add(r)));
        }
        v.push(b',');
        v
    }

    /// One call of `mk_string_list (ptr, &len, &begin, &run, cur, inchar)`;
    /// `cur` `None` ends the list. `false` when the buffer is full.
    fn add(&mut self, cur: Option<usize>) -> bool {
        match cur {
            Some(cur) => {
                if self.begin == 0 {
                    self.begin = cur.wrapping_add(1);
                    return true;
                }
                if self.begin.wrapping_add(self.run) == cur {
                    self.run = self.run.wrapping_add(1);
                    return true;
                }
            }
            None if self.begin == 0 => return true,
            None => {}
        }
        let piece = self.piece();
        // `rlen >= *len` with `len` what is left of BUFSIZ.
        if self.out.len().saturating_add(piece.len()) >= BUFSIZ {
            return false;
        }
        self.out.extend(piece);
        match cur {
            None => {
                // The trailing comma off.
                self.out.pop();
            }
            Some(cur) => {
                self.begin = cur.wrapping_add(1);
                self.run = 0;
            }
        }
        true
    }
}

impl Context {
    /// `fdisk_set_ask (cxt, ask_cb, data)`.
    pub fn set_ask(&mut self, cb: Option<AskCallback>) {
        self.ask_cb = cb;
    }

    /// `fdisk_do_ask (cxt, ask)`: the callback's answer; -EINVAL without a
    /// callback, or for a question when dialogs are disabled.
    pub fn do_ask(&mut self, ask: &mut Ask) -> i32 {
        if !self.has_dialogs() && !matches!(ask.ty, AskType::Info | AskType::Warnx | AskType::Warn)
        {
            return -EINVAL;
        }
        let Some(cb) = self.ask_cb.clone() else {
            return -EINVAL;
        };
        cb(self, ask)
    }

    /// `fdisk_ask_partnum (cxt, &partnum, wantnew)`: a used (or, with
    /// `wantnew`, a free) partition number, from 0, asked for unless there
    /// is only one. `Err` holds upstream's status: 1 when there is none, or
    /// a negative errno.
    pub fn ask_partnum(&mut self, wantnew: bool) -> Result<usize, i32> {
        let Some(lb) = self.label_ref() else {
            return Err(-EINVAL);
        };
        let inchar = lb.flags & LABEL_FL_INCHARS_PARTNO != 0;
        let nmax = lb.nparts_max;
        let mut ask = Ask::new(AskType::Number);
        let mut num = AskNumber {
            inchars: inchar,
            ..AskNumber::default()
        };
        let mut list = RangeList {
            out: Vec::new(),
            begin: 0,
            run: 0,
            inchar,
        };
        let mut rc = 0;

        for i in 0..nmax {
            let used = self.is_partition_used(i) != 0;
            let n = u64::try_from(i).unwrap_or(u64::MAX).wrapping_add(1);
            if wantnew && !used {
                if !list.add(Some(i)) {
                    rc = -EINVAL;
                    break;
                }
                if num.low == 0 {
                    num.low = n;
                    num.dfl = n;
                }
                num.hig = n;
            } else if !wantnew && used {
                // Upstream does not check this one: its buffer only
                // overflows past a thousand partitions.
                let _ = list.add(Some(i));
                if num.low == 0 {
                    num.low = n;
                }
                num.dfl = n;
                num.hig = n;
            }
        }

        if rc == 0 && num.low == num.hig {
            if num.low > 0 {
                num.result = num.low;
                ask.data = AskData::Number(num.clone());
                self.info(format!("Selected partition {}", num.low).as_bytes());
            } else {
                self.warnx(if wantnew {
                    b"No free partition available!".as_slice()
                } else {
                    b"No partition is defined yet!".as_slice()
                });
                rc = 1;
            }
        } else if rc == 0 {
            list.add(None);
            num.range = Some(list.out);
            ask.data = AskData::Number(num);
            ask.set_query(b"Partition number");
            rc = self.do_ask(&mut ask);
        }
        if rc != 0 {
            return Err(rc);
        }
        let r = ask.number_get_result();
        let r = usize::try_from(r).unwrap_or(usize::MAX);
        Ok(if r != 0 { r.wrapping_sub(1) } else { r })
    }

    /// `fdisk_ask_number (cxt, low, dflt, high, query, &result)`.
    pub fn ask_number(&mut self, low: u64, dflt: u64, high: u64, query: &[u8]) -> Result<u64, i32> {
        let mut ask = Ask::new(AskType::Number);
        ask.number_set_low(low);
        ask.number_set_default(dflt);
        ask.number_set_high(high);
        ask.set_query(query);
        match self.do_ask(&mut ask) {
            0 => Ok(ask.number_get_result()),
            rc => Err(rc),
        }
    }

    /// `fdisk_ask_string (cxt, query, &result)`.
    pub fn ask_string(&mut self, query: &[u8]) -> Result<Option<Vec<u8>>, i32> {
        let mut ask = Ask::new(AskType::String);
        ask.set_query(query);
        match self.do_ask(&mut ask) {
            0 => Ok(ask.string_get_result().map(<[u8]>::to_vec)),
            rc => Err(rc),
        }
    }

    /// `fdisk_ask_yesno (cxt, query, &result)`: whether the answer was yes.
    pub fn ask_yesno(&mut self, query: &[u8]) -> Result<bool, i32> {
        let mut ask = Ask::new(AskType::YesNo);
        ask.set_query(query);
        match self.do_ask(&mut ask) {
            0 => Ok(ask.yesno_get_result() == 1),
            rc => Err(rc),
        }
    }

    /// `do_vprint`: a message of type `ty` through the callback.
    fn do_print(&mut self, ty: AskType, errnum: i32, mesg: &[u8]) -> i32 {
        let mut ask = Ask::new(ty);
        ask.data = AskData::Print {
            mesg: mesg.to_vec(),
            errnum,
        };
        self.do_ask(&mut ask)
    }

    /// `fdisk_info (cxt, fmt, ...)`.
    pub fn info(&mut self, mesg: &[u8]) -> i32 {
        self.do_print(AskType::Info, 0, mesg)
    }

    /// `fdisk_warn (cxt, fmt, ...)`: a warning that `errnum`, upstream's
    /// `errno` at the call, explains.
    pub fn warn(&mut self, errnum: i32, mesg: &[u8]) -> i32 {
        self.do_print(AskType::Warn, errnum, mesg)
    }

    /// `fdisk_warnx (cxt, fmt, ...)`.
    pub fn warnx(&mut self, mesg: &[u8]) -> i32 {
        self.do_print(AskType::Warnx, 0, mesg)
    }

    /// `fdisk_info_new_partition (cxt, num, start, stop, t)`.
    pub fn info_new_partition(
        &mut self,
        num: i64,
        start: Sector,
        stop: Sector,
        t: Option<&crate::PartType>,
    ) -> i32 {
        let bytes = stop
            .wrapping_sub(start)
            .wrapping_add(1)
            .wrapping_mul(self.sector_size);
        let size = ulstrutils::size_to_human_string(
            ulstrutils::SIZE_SUFFIX_3LETTER | ulstrutils::SIZE_SUFFIX_SPACE,
            bytes,
        );
        let mut m = format!("Created a new partition {num} of type '").into_bytes();
        m.extend_from_slice(t.and_then(crate::PartType::get_name).unwrap_or(b"Unknown"));
        m.extend_from_slice(format!("' and of size {size}.").as_bytes());
        self.info(&m)
    }
}
