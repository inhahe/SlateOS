//! `deflate.c`: finding repeated strings with hash chains over a 64 KiB
//! sliding window, lazily for levels 4 to 9, and handing literals and
//! matches to [`trees`](crate::trees).
//!
//! The window here is [`Gz::window`], and what it holds past the input's
//! last byte is whatever the previous file left there -- see the crate
//! docs on why that is part of the output.

// Window and hash-table indexes are bounded as upstream bounds them:
// every position is below `2 * WSIZE`, every hash below `HASH_SIZE`, and a
// match is read no further than `strstart + MAX_MATCH`, which the
// `window_size - MIN_LOOKAHEAD` test keeps inside the window.

use crate::{Gz, Io, MAX_DIST, MAX_MATCH, MIN_LOOKAHEAD, MIN_MATCH, Res, WSIZE};

/// Bits in a hash.
const HASH_BITS: u32 = 15;
/// Hash heads.
const HASH_SIZE: u32 = 1 << HASH_BITS;
const HASH_MASK: u32 = HASH_SIZE - 1;
const WMASK: u32 = WSIZE as u32 - 1;
/// The end of a hash chain.
const NIL: u32 = 0;
/// A match of length 3 farther back than this is not worth its bits.
const TOO_FAR: u32 = 4096;
/// `--rsyncable`'s window.
const RSYNC_WIN: u32 = 4096;
/// "No rsync sequence point pending."
const NO_CHUNK_END: u64 = 0xFFFF_FFFF;
/// The window's whole size.
const WINDOW_SIZE: u64 = 2 * WSIZE as u64;
/// Bits each byte shifts the running hash.
const H_SHIFT: u32 = HASH_BITS.div_ceil(MIN_MATCH);
/// Where the hash heads live in [`Gz::prev`].
const HEAD: usize = WSIZE;

/// `configuration_table`: good length, lazy length, nice length, chain.
const CONFIGURATION_TABLE: [(u32, u32, u32, u32); 10] = [
    (0, 0, 0, 0),
    (4, 4, 8, 4),
    (4, 5, 16, 8),
    (4, 6, 32, 32),
    (4, 4, 16, 16),
    (8, 16, 32, 32),
    (8, 16, 128, 128),
    (8, 32, 128, 256),
    (32, 128, 258, 1024),
    (32, 258, 258, 4096),
];

/// The match finder's state: upstream's statics in `deflate.c`.
#[derive(Debug, Default, Clone)]
pub struct Lm {
    /// Window position of the current block's start; negative once the
    /// window has slid past it.
    pub block_start: i64,
    /// The running hash of the string being inserted.
    ins_h: u32,
    /// The best match's length at the previous step.
    prev_length: u32,
    /// The string being inserted.
    pub strstart: u32,
    /// The best match's start.
    match_start: u32,
    /// The input has ended.
    eofile: bool,
    /// Valid bytes ahead of `strstart`.
    lookahead: u32,
    /// Hash chains are followed no further than this.
    max_chain_length: u32,
    /// `max_lazy_match`, which levels 1-3 use as `max_insert_length`.
    max_lazy_match: u32,
    /// Search less once a match is this long.
    good_match: u32,
    /// Stop searching once a match is this long.
    nice_match: u32,
    /// `--rsyncable`'s rolling sum, and the next sequence point.
    rsync_sum: u64,
    rsync_chunk_end: u64,
}

impl<I: Io> Gz<I> {
    /// `UPDATE_HASH (h, c)`.
    fn update_hash(h: u32, c: u8) -> u32 {
        ((h << H_SHIFT) ^ u32::from(c)) & HASH_MASK
    }

    /// `INSERT_STRING (s, match_head)`: put the string at `s` in the hash
    /// chains, answering the chain's previous head.
    fn insert_string(&mut self, s: u32) -> u32 {
        self.lm.ins_h = Self::update_hash(self.lm.ins_h, self.window[(s + MIN_MATCH - 1) as usize]);
        let head = u32::from(self.prev[HEAD + self.lm.ins_h as usize]);
        self.prev[(s & WMASK) as usize] = head as u16;
        self.prev[HEAD + self.lm.ins_h as usize] = s as u16;
        head
    }

    /// `read_buf` for deflate: `file_read` into `window[at..at + size]`.
    /// The bytes read; 0 at the end.
    fn read_window(&mut self, at: usize, size: usize) -> Res<u32> {
        let mut window = std::mem::take(&mut self.window);
        let r = self.file_read(&mut window[at..at + size]);
        self.window = window;
        r.map(|n| n as u32)
    }

    /// `lm_init (pack_level)`: start a file at a level.
    ///
    /// # Errors
    ///
    /// As [`Gz::file_read`]; a level outside 1-9 aborts.
    fn lm_init(&mut self, pack_level: i32) -> Res<()> {
        if !(1..=9).contains(&pack_level) {
            return self.gzip_error("bad pack level");
        }
        for h in &mut self.prev[HEAD..HEAD + HASH_SIZE as usize] {
            *h = 0;
        }
        self.lm.rsync_chunk_end = NO_CHUNK_END;
        self.lm.rsync_sum = 0;

        let (good, lazy, nice, chain) = CONFIGURATION_TABLE[pack_level as usize];
        self.lm.max_lazy_match = lazy;
        self.lm.good_match = good;
        self.lm.nice_match = nice;
        self.lm.max_chain_length = chain;

        self.lm.strstart = 0;
        self.lm.block_start = 0;

        self.lm.lookahead = self.read_window(0, 2 * WSIZE)?;
        if self.lm.lookahead == 0 {
            self.lm.eofile = true;
            self.lm.lookahead = 0;
            return Ok(());
        }
        self.lm.eofile = false;
        while self.lm.lookahead < MIN_LOOKAHEAD && !self.lm.eofile {
            self.fill_window()?;
        }
        self.lm.ins_h = 0;
        for j in 0..(MIN_MATCH - 1) as usize {
            self.lm.ins_h = Self::update_hash(self.lm.ins_h, self.window[j]);
        }
        Ok(())
    }

    /// `longest_match (cur_match)`: the longest match for the string at
    /// `strstart`, following the hash chain from `cur_match`; sets
    /// `match_start`. A match no longer than `prev_length` answers
    /// `prev_length`.
    fn longest_match(&mut self, mut cur_match: u32) -> u32 {
        let lm = &mut self.lm;
        let window = &self.window;
        let prev = &self.prev;
        let mut chain_length = lm.max_chain_length;
        let scan = lm.strstart as usize;
        let mut best_len = lm.prev_length;
        let limit = if lm.strstart > MAX_DIST {
            lm.strstart - MAX_DIST
        } else {
            NIL
        };
        let strend = scan + MAX_MATCH as usize;
        let mut scan_end1 = window[scan + best_len as usize - 1];
        let mut scan_end = window[scan + best_len as usize];

        if lm.prev_length >= lm.good_match {
            chain_length >>= 2;
        }

        loop {
            let m = cur_match as usize;
            // Skip to the next match if this one cannot be longer, or does
            // not even begin as the string does.
            if !(window[m + best_len as usize] != scan_end
                || window[m + best_len as usize - 1] != scan_end1
                || window[m] != window[scan]
                || window[m + 1] != window[scan + 1])
            {
                // `scan[2]` and `match[2]` are equal when the rest are,
                // given equal hashes; compare from 3 on, eight at a time,
                // checking the end only between rounds as upstream does.
                // Upstream's `*++match` in the test above and its
                // `scan += 2, match++` leave both two bytes in.
                let mut s = scan + 2;
                let mut mm = m + 2;
                loop {
                    let mut stop = false;
                    for _ in 0..8 {
                        s += 1;
                        mm += 1;
                        if window[s] != window[mm] {
                            stop = true;
                            break;
                        }
                    }
                    if stop || s >= strend {
                        break;
                    }
                }
                let len = MAX_MATCH - (strend - s) as u32;
                if len > best_len {
                    lm.match_start = cur_match;
                    best_len = len;
                    if len >= lm.nice_match {
                        break;
                    }
                    scan_end1 = window[scan + best_len as usize - 1];
                    scan_end = window[scan + best_len as usize];
                }
            }
            cur_match = u32::from(prev[(cur_match & WMASK) as usize]);
            if cur_match <= limit {
                break;
            }
            chain_length -= 1;
            if chain_length == 0 {
                break;
            }
        }
        best_len
    }

    /// `fill_window`: read more input when the lookahead is short, sliding
    /// the window down first when it is nearly full.
    fn fill_window(&mut self) -> Res<()> {
        let mut more =
            (WINDOW_SIZE - u64::from(self.lm.lookahead) - u64::from(self.lm.strstart)) as u32;
        if more == u32::MAX {
            more -= 1;
        } else if self.lm.strstart >= WSIZE as u32 + MAX_DIST {
            self.window.copy_within(WSIZE..2 * WSIZE, 0);
            self.lm.match_start = self.lm.match_start.wrapping_sub(WSIZE as u32);
            self.lm.strstart -= WSIZE as u32;
            if self.lm.rsync_chunk_end != NO_CHUNK_END {
                self.lm.rsync_chunk_end = self.lm.rsync_chunk_end.wrapping_sub(WSIZE as u64);
            }
            self.lm.block_start -= WSIZE as i64;

            for h in &mut self.prev[HEAD..HEAD + HASH_SIZE as usize] {
                let m = u32::from(*h);
                *h = if m >= WSIZE as u32 {
                    (m - WSIZE as u32) as u16
                } else {
                    NIL as u16
                };
            }
            for p in &mut self.prev[..WSIZE] {
                let m = u32::from(*p);
                *p = if m >= WSIZE as u32 {
                    (m - WSIZE as u32) as u16
                } else {
                    NIL as u16
                };
            }
            more += WSIZE as u32;
        }
        if !self.lm.eofile {
            let at = (self.lm.strstart + self.lm.lookahead) as usize;
            let n = self.read_window(at, more as usize)?;
            if n == 0 {
                self.lm.eofile = true;
                // Don't let garbage pollute the dictionary.
                for b in &mut self.window[at..at + (MIN_MATCH - 1) as usize] {
                    *b = 0;
                }
            } else {
                self.lm.lookahead += n;
            }
        }
        Ok(())
    }

    /// `rsync_roll (start, num)`: advance `--rsyncable`'s rolling sum over
    /// `num` bytes from window position `start`.
    fn rsync_roll(&mut self, mut start: u32, mut num: u32) {
        if start < RSYNC_WIN {
            let mut i = start;
            while i < RSYNC_WIN {
                if i == start + num {
                    return;
                }
                self.lm.rsync_sum = self
                    .lm
                    .rsync_sum
                    .wrapping_add(u64::from(self.window[i as usize]));
                i += 1;
            }
            num -= RSYNC_WIN - start;
            start = RSYNC_WIN;
        }
        for i in start..start + num {
            self.lm.rsync_sum = self
                .lm
                .rsync_sum
                .wrapping_add(u64::from(self.window[i as usize]));
            self.lm.rsync_sum = self
                .lm
                .rsync_sum
                .wrapping_sub(u64::from(self.window[(i - RSYNC_WIN) as usize]));
            if self.lm.rsync_chunk_end == NO_CHUNK_END
                && self.lm.rsync_sum.is_multiple_of(u64::from(RSYNC_WIN))
            {
                self.lm.rsync_chunk_end = u64::from(i);
            }
        }
    }

    /// `RSYNC_ROLL (s, n)`.
    fn rsync_roll_if(&mut self, s: u32, n: u32) {
        if self.rsync {
            self.rsync_roll(s, n);
        }
    }

    /// `FLUSH_BLOCK (eof)`, with the current block's `flush` request.
    fn flush_block_here(&mut self, flush: i32, eof: bool) -> Res<i64> {
        let buf = if self.lm.block_start >= 0 {
            Some(self.lm.block_start as usize)
        } else {
            None
        };
        let stored_len = (i64::from(self.lm.strstart) - self.lm.block_start) as u64;
        self.flush_block(buf, stored_len, flush - 1 != 0, eof)
    }

    /// `rsync && strstart > rsync_chunk_end`.
    fn at_rsync_point(&self) -> bool {
        self.rsync && u64::from(self.lm.strstart) > self.lm.rsync_chunk_end
    }

    /// `deflate_fast`: levels 1 to 3 -- no lazy matching, and the strings a
    /// long match covers are not hashed.
    fn deflate_fast(&mut self) -> Res<i64> {
        let mut flush = 0;
        let mut match_length: u32 = 0;

        self.lm.prev_length = MIN_MATCH - 1;
        while self.lm.lookahead != 0 {
            let hash_head = self.insert_string(self.lm.strstart);

            if hash_head != NIL
                && self.lm.strstart.wrapping_sub(hash_head) <= MAX_DIST
                && u64::from(self.lm.strstart) <= WINDOW_SIZE - u64::from(MIN_LOOKAHEAD)
            {
                match_length = self.longest_match(hash_head);
                if match_length > self.lm.lookahead {
                    match_length = self.lm.lookahead;
                }
            }
            if match_length >= MIN_MATCH {
                let dist = self.lm.strstart.wrapping_sub(self.lm.match_start);
                flush = i32::from(self.ct_tally(dist, match_length - MIN_MATCH));
                self.lm.lookahead -= match_length;
                self.rsync_roll_if(self.lm.strstart, match_length);
                if match_length <= self.lm.max_lazy_match {
                    match_length -= 1;
                    loop {
                        self.lm.strstart += 1;
                        self.insert_string(self.lm.strstart);
                        match_length -= 1;
                        if match_length == 0 {
                            break;
                        }
                    }
                    self.lm.strstart += 1;
                } else {
                    self.lm.strstart += match_length;
                    match_length = 0;
                    self.lm.ins_h = u32::from(self.window[self.lm.strstart as usize]);
                    self.lm.ins_h = Self::update_hash(
                        self.lm.ins_h,
                        self.window[self.lm.strstart as usize + 1],
                    );
                }
            } else {
                let c = self.window[self.lm.strstart as usize];
                flush = i32::from(self.ct_tally(0, u32::from(c)));
                self.rsync_roll_if(self.lm.strstart, 1);
                self.lm.lookahead -= 1;
                self.lm.strstart += 1;
            }
            if self.at_rsync_point() {
                self.lm.rsync_chunk_end = NO_CHUNK_END;
                flush = 2;
            }
            if flush != 0 {
                self.flush_block_here(flush, false)?;
                self.lm.block_start = i64::from(self.lm.strstart);
            }
            while self.lm.lookahead < MIN_LOOKAHEAD && !self.lm.eofile {
                self.fill_window()?;
            }
        }
        self.flush_block_here(flush, true)
    }

    /// `deflate (pack_level)`: compress the input. The compressed length.
    ///
    /// # Errors
    ///
    /// [`Stop::Abort`](crate::Stop::Abort) for a read or a write that failed.
    pub fn deflate(&mut self, pack_level: i32) -> Res<i64> {
        let mut flush = 0;
        let mut match_available = false;
        let mut match_length: u32 = MIN_MATCH - 1;

        self.lm_init(pack_level)?;
        if pack_level <= 3 {
            return self.deflate_fast();
        }

        while self.lm.lookahead != 0 {
            let hash_head = self.insert_string(self.lm.strstart);

            self.lm.prev_length = match_length;
            let prev_match = self.lm.match_start;
            match_length = MIN_MATCH - 1;

            if hash_head != NIL
                && self.lm.prev_length < self.lm.max_lazy_match
                && self.lm.strstart.wrapping_sub(hash_head) <= MAX_DIST
                && u64::from(self.lm.strstart) <= WINDOW_SIZE - u64::from(MIN_LOOKAHEAD)
            {
                match_length = self.longest_match(hash_head);
                if match_length > self.lm.lookahead {
                    match_length = self.lm.lookahead;
                }
                // `match_start` may be stale here, and wrap, as upstream's.
                if match_length == MIN_MATCH
                    && self.lm.strstart.wrapping_sub(self.lm.match_start) > TOO_FAR
                {
                    match_length -= 1;
                }
            }

            if self.lm.prev_length >= MIN_MATCH && match_length <= self.lm.prev_length {
                flush = i32::from(self.ct_tally(
                    self.lm.strstart.wrapping_sub(1).wrapping_sub(prev_match),
                    self.lm.prev_length - MIN_MATCH,
                ));
                self.lm.lookahead -= self.lm.prev_length - 1;
                self.lm.prev_length -= 2;
                self.rsync_roll_if(self.lm.strstart, self.lm.prev_length + 1);
                loop {
                    self.lm.strstart += 1;
                    self.insert_string(self.lm.strstart);
                    self.lm.prev_length -= 1;
                    if self.lm.prev_length == 0 {
                        break;
                    }
                }
                match_available = false;
                match_length = MIN_MATCH - 1;
                self.lm.strstart += 1;

                if self.at_rsync_point() {
                    self.lm.rsync_chunk_end = NO_CHUNK_END;
                    flush = 2;
                }
                if flush != 0 {
                    self.flush_block_here(flush, false)?;
                    self.lm.block_start = i64::from(self.lm.strstart);
                }
            } else if match_available {
                let c = self.window[self.lm.strstart as usize - 1];
                flush = i32::from(self.ct_tally(0, u32::from(c)));
                if self.at_rsync_point() {
                    self.lm.rsync_chunk_end = NO_CHUNK_END;
                    flush = 2;
                }
                if flush != 0 {
                    self.flush_block_here(flush, false)?;
                    self.lm.block_start = i64::from(self.lm.strstart);
                }
                self.rsync_roll_if(self.lm.strstart, 1);
                self.lm.strstart += 1;
                self.lm.lookahead -= 1;
            } else {
                if self.at_rsync_point() {
                    self.lm.rsync_chunk_end = NO_CHUNK_END;
                    flush = 2;
                    self.flush_block_here(flush, false)?;
                    self.lm.block_start = i64::from(self.lm.strstart);
                }
                match_available = true;
                self.rsync_roll_if(self.lm.strstart, 1);
                self.lm.strstart += 1;
                self.lm.lookahead -= 1;
            }

            while self.lm.lookahead < MIN_LOOKAHEAD && !self.lm.eofile {
                self.fill_window()?;
            }
        }
        if match_available {
            let c = self.window[self.lm.strstart as usize - 1];
            self.ct_tally(0, u32::from(c));
        }
        self.flush_block_here(flush, true)
    }
}
