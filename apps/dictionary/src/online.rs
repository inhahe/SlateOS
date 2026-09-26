//! Looking a word up in a real dictionary: WordNet, over the DICT protocol
//! (RFC 2229), at dict.org.
//!
//! The built-in list is thirty words, and a dictionary of thirty words is a
//! demonstration. A word it lacks is looked up here -- **when the reader asks,
//! never as they type**. What is sent is the word, to a server this program
//! does not run; a lookup per keystroke would send every prefix of every word
//! typed into the box, including the ones the reader thought better of.
//!
//! # The conversation
//!
//! One connection per lookup, in plain text: the server's `220` greeting,
//! `CLIENT` (it names this program, as the RFC asks), `DEFINE wn "<word>"`,
//! and -- when WordNet has no entry -- `MATCH wn lev "<word>"`, which returns
//! the words one edit away, as "did you mean" suggestions. Then `QUIT`.
//!
//! Every reply is bounded: a line at [`MAX_LINE`] bytes, the whole
//! conversation at [`MAX_REPLY`], and each read and write at the timeout the
//! caller gives. A server that talks for ever is cut off and said to have,
//! rather than filling the window's memory.
//!
//! # What WordNet's text becomes
//!
//! WordNet's entries are regular enough to read into the same shape as the
//! built-in ones (checked against dict.org's replies; the tests replay them):
//!
//! ```text
//! bank
//!     n 1: sloping land (especially the slope beside a body of water);
//!          "they pulled the canoe up on the bank"
//!     2: a financial institution ...; "he cashed a check at the
//!        bank" [syn: {depository financial institution}, {bank}]
//!     v 1: tip laterally; "the pilot had to bank the aircraft"
//! ```
//!
//! A line indented four places starts a sense -- with a part of speech when
//! it changes, and a number; the deeper lines continue it. In a sense's text,
//! the quoted phrases after the gloss are examples, and bracketed lists name
//! synonyms (`syn`), antonyms (`ant`) and anything else (`also`, ...) as
//! related words. A part of speech this reader does not know makes the whole
//! entry unreadable, and it says so -- rather than filing a sense under a
//! part of speech nothing named.

use crate::{Definition, DictEntry, PartOfSpeech};
use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// The public DICT server, and its port.
pub const DICT_ORG: &str = "dict.org:2628";

/// WordNet, of the databases dict.org serves: the one whose entries have the
/// structure the entry screen draws -- senses, parts of speech, examples,
/// synonyms and antonyms.
pub const DATABASE: &str = "wn";

/// The longest line a reply may carry. WordNet's run to about 70 bytes.
pub const MAX_LINE: usize = 8 * 1024;

/// The most a whole conversation may bring back. The longest WordNet entry
/// ("set", "run", "break") is some tens of kilobytes.
pub const MAX_REPLY: usize = 1024 * 1024;

/// The longest word this sends. The RFC caps a command line at 1024 bytes;
/// no word in any dictionary comes near this.
pub const MAX_WORD: usize = 200;

/// What a lookup found.
#[derive(Clone, Debug)]
pub enum Answer {
    /// The entry, in the built-in entries' shape.
    Found(DictEntry),
    /// No entry; the words one edit away, if any, to offer instead.
    NotFound { suggestions: Vec<String> },
}

/// Why a lookup has no answer.
#[derive(Debug)]
pub enum LookupError {
    /// The word cannot be sent: empty, too long, or holding a control
    /// character the protocol's line framing cannot carry.
    Unsendable(&'static str),
    /// No connection, or it broke, or the server was silent past the timeout.
    Network(std::io::Error),
    /// The server answered with something other than an answer: a refusal,
    /// a reply past the bounds above, or text in a form this cannot read.
    Server(String),
}

impl fmt::Display for LookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsendable(why) => write!(f, "the word cannot be sent: {why}"),
            Self::Network(e) => write!(f, "{e}"),
            Self::Server(said) => f.write_str(said),
        }
    }
}

/// A lookup, as the window calls it: the real one is [`look_up`]; a test's
/// stands in, so no test reaches the network.
pub type Lookup = fn(&str, &str) -> Result<Answer, LookupError>;

/// How long a connection, a read or a write may take before the lookup is
/// given up.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// Look `word` up in WordNet at `server` (`host:port`), with [`TIMEOUT`].
///
/// # Errors
///
/// [`LookupError`] when the word cannot be sent, the server cannot be
/// reached, or it answers with anything but an entry or "no match".
pub fn look_up(server: &str, word: &str) -> Result<Answer, LookupError> {
    look_up_with(server, word, TIMEOUT)
}

/// [`look_up`], with the timeout given.
///
/// # Errors
///
/// As [`look_up`].
pub fn look_up_with(server: &str, word: &str, timeout: Duration) -> Result<Answer, LookupError> {
    // Checked before a connection is made: a word that cannot be sent should
    // not cost the reader a round trip to find out.
    let quoted = quote(word)?;
    let stream = connect(server, timeout)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(LookupError::Network)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(LookupError::Network)?;
    let mut writer = stream.try_clone().map_err(LookupError::Network)?;
    let mut conn = Conn {
        reader: BufReader::new(stream),
        budget: MAX_REPLY,
    };

    let (code, text) = conn.status()?;
    if code != 220 {
        return Err(LookupError::Server(format!(
            "the server turned us away: {text}"
        )));
    }
    send(&mut writer, "CLIENT slateos-dictionary")?;
    // A server may decline to be told who is asking; that is not a reason to
    // give up on the word.
    let _ = conn.status()?;

    send(&mut writer, &format!("DEFINE {DATABASE} {quoted}"))?;
    let answer = match conn.status()? {
        (150, _) => Answer::Found(conn.definitions(word)?),
        (552, _) => {
            send(&mut writer, &format!("MATCH {DATABASE} lev {quoted}"))?;
            let suggestions = match conn.status()? {
                (152, _) => conn.matches(word)?,
                (552, _) => Vec::new(),
                (_, text) => return Err(LookupError::Server(text)),
            };
            Answer::NotFound { suggestions }
        }
        (_, text) => return Err(LookupError::Server(text)),
    };
    // The answer is in hand, so nothing from here on is reported: a server
    // that has gone by now changes nothing. But its goodbye is waited for,
    // briefly -- closing with the `221` unread would reset the connection
    // rather than end it -- and a server that never sends one costs a
    // second, not the whole timeout.
    if send(&mut writer, "QUIT").is_ok()
        && conn
            .reader
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(1)))
            .is_ok()
    {
        let _ = conn.status();
    }
    Ok(answer)
}

/// Open a connection to `server`, trying each address its name resolves to.
fn connect(server: &str, timeout: Duration) -> Result<TcpStream, LookupError> {
    let addrs = server.to_socket_addrs().map_err(LookupError::Network)?;
    let mut last = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(stream) => return Ok(stream),
            Err(e) => last = Some(e),
        }
    }
    Err(LookupError::Network(last.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{server} names no address"),
        )
    })))
}

/// Send one command line.
fn send(writer: &mut TcpStream, line: &str) -> Result<(), LookupError> {
    writer
        .write_all(format!("{line}\r\n").as_bytes())
        .map_err(LookupError::Network)
}

/// `word` as a DICT string: in double quotes, with `"` and `\` escaped.
///
/// A control character is refused rather than escaped: CR or LF would end the
/// command line early and start another -- the word would become a command --
/// and the RFC gives the rest no meaning inside a string.
///
/// # Errors
///
/// [`LookupError::Unsendable`] for an empty word, one over [`MAX_WORD`]
/// bytes, or one holding a control character.
pub fn quote(word: &str) -> Result<String, LookupError> {
    let word = word.trim();
    if word.is_empty() {
        return Err(LookupError::Unsendable("it is empty"));
    }
    if word.len() > MAX_WORD {
        return Err(LookupError::Unsendable("it is longer than a word can be"));
    }
    if word.chars().any(char::is_control) {
        return Err(LookupError::Unsendable("it holds a control character"));
    }
    let mut out = String::with_capacity(word.len().saturating_add(2));
    out.push('"');
    for c in word.chars() {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    Ok(out)
}

/// A connection's reading half, with what it may still read.
struct Conn<R> {
    reader: BufReader<R>,
    /// Bytes the rest of the conversation may bring back.
    budget: usize,
}

impl<R: Read> Conn<R> {
    /// The next line, without its line ending: UTF-8 as dict.org sends, or
    /// read byte for byte as Latin-1 when it is not, so no byte is lost or
    /// replaced (`netscan`'s WHOIS reader does the same).
    fn line(&mut self) -> Result<String, LookupError> {
        let mut raw = Vec::new();
        let cap = MAX_LINE.min(self.budget).saturating_add(1);
        let got = (&mut self.reader)
            .take(cap as u64)
            .read_until(b'\n', &mut raw)
            .map_err(LookupError::Network)?;
        if got == 0 {
            return Err(LookupError::Server(String::from(
                "the server closed the connection mid-reply",
            )));
        }
        if !raw.ends_with(b"\n") {
            return Err(LookupError::Server(if got > MAX_LINE {
                String::from("the server sent a line longer than any reply has")
            } else {
                String::from("the reply was longer than any dictionary entry")
            }));
        }
        self.budget = self.budget.saturating_sub(got);
        raw.pop();
        if raw.last() == Some(&b'\r') {
            raw.pop();
        }
        Ok(String::from_utf8(raw)
            .unwrap_or_else(|e| e.into_bytes().iter().map(|&b| char::from(b)).collect()))
    }

    /// A status line: its three-digit code and its text.
    fn status(&mut self) -> Result<(u16, String), LookupError> {
        let line = self.line()?;
        let code = line
            .get(..3)
            .filter(|c| c.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|c| c.parse().ok())
            .ok_or_else(|| LookupError::Server(format!("not a status line: {line}")))?;
        Ok((code, line))
    }

    /// A text block, up to the line holding a lone `.`, with the RFC's
    /// dot-stuffing undone: a line the server began with `..` began with `.`.
    fn text(&mut self) -> Result<Vec<String>, LookupError> {
        let mut lines = Vec::new();
        loop {
            let line = self.line()?;
            if line == "." {
                return Ok(lines);
            }
            lines.push(match line.strip_prefix("..") {
                Some(rest) => format!(".{rest}"),
                None => line,
            });
        }
    }

    /// After `150`: each `151` definition and its text, then `250`; the
    /// definitions read into one entry.
    fn definitions(&mut self, asked: &str) -> Result<DictEntry, LookupError> {
        let mut entry: Option<DictEntry> = None;
        loop {
            match self.status()? {
                (151, header) => {
                    let source = source_of(&header);
                    let text = self.text()?;
                    let read = parse_wordnet(&text, source).ok_or_else(|| {
                        LookupError::Server(format!(
                            "WordNet's entry for \"{asked}\" is not in the form this program reads"
                        ))
                    })?;
                    entry = Some(match entry {
                        None => read,
                        Some(first) => merge(first, read),
                    });
                }
                (250, _) => break,
                (_, text) => return Err(LookupError::Server(text)),
            }
        }
        entry.ok_or_else(|| {
            LookupError::Server(format!(
                "the server said it found \"{asked}\" and sent no definition"
            ))
        })
    }

    /// After `152`: the matching words, then `250`. The word asked about is
    /// left out -- suggesting it back would be a loop -- as are repeats.
    fn matches(&mut self, asked: &str) -> Result<Vec<String>, LookupError> {
        let lines = self.text()?;
        match self.status()? {
            (250, _) => {}
            (_, text) => return Err(LookupError::Server(text)),
        }
        let mut out: Vec<String> = Vec::new();
        for line in &lines {
            // `wn "hay"`: the database, then the word as a DICT string.
            let Some(word) = tokens(line).into_iter().nth(1) else {
                continue;
            };
            if !word.eq_ignore_ascii_case(asked.trim())
                && !out.iter().any(|w| w.eq_ignore_ascii_case(&word))
            {
                out.push(word);
            }
        }
        Ok(out)
    }
}

/// The words of a DICT line: atoms, and strings in quotes with `\` escapes
/// undone.
fn tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let mut token = String::new();
        if c == '"' || c == '\'' {
            chars.next();
            while let Some(ch) = chars.next() {
                match ch {
                    '\\' => {
                        if let Some(escaped) = chars.next() {
                            token.push(escaped);
                        }
                    }
                    _ if ch == c => break,
                    _ => token.push(ch),
                }
            }
        } else {
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                token.push(ch);
                chars.next();
            }
        }
        out.push(token);
    }
    out
}

/// What a `151` line says the text came from: `151 "bank" wn "WordNet (r) 3.0
/// (2006)"` gives the description, or the database's name without one.
fn source_of(header: &str) -> String {
    let words = tokens(header);
    words
        .get(3)
        .or_else(|| words.get(2))
        .cloned()
        .unwrap_or_else(|| String::from(DATABASE))
}

/// Two definitions of one word -- WordNet keeps "Polish" and "polish" apart,
/// and a case-blind DEFINE returns both -- as one entry, in the order sent.
fn merge(mut first: DictEntry, second: DictEntry) -> DictEntry {
    first.definitions.extend(second.definitions);
    for (into, from) in [
        (&mut first.synonyms, second.synonyms),
        (&mut first.antonyms, second.antonyms),
        (&mut first.related, second.related),
    ] {
        for word in from {
            if !into.contains(&word) {
                into.push(word);
            }
        }
    }
    first
}

/// WordNet's short part-of-speech names.
fn part_of_speech(tag: &str) -> Option<PartOfSpeech> {
    match tag {
        "n" => Some(PartOfSpeech::Noun),
        "v" => Some(PartOfSpeech::Verb),
        "adj" => Some(PartOfSpeech::Adjective),
        "adv" => Some(PartOfSpeech::Adverb),
        _ => None,
    }
}

/// Where a sense starts: `n 1: text`, `2: text`, `adv : text` -- with the part
/// of speech if the line names one, and the text after the colon.
///
/// Only a line indented five places or fewer can start one. WordNet indents a
/// sense four places and its continuation lines past the colon, so a
/// continuation that happens to begin "3:" is not mistaken for sense three.
fn sense_start(line: &str) -> Option<(Option<&str>, &str)> {
    let body = line.trim_start();
    let indent = line.len().saturating_sub(body.len());
    if indent == 0 || indent > 5 {
        return None;
    }
    let (tag, rest) = match body.split_once(' ') {
        Some((first, rest)) if !first.starts_with(|c: char| c.is_ascii_digit()) => {
            (Some(first), rest.trim_start())
        }
        _ => (None, body),
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let after = rest.get(digits..)?.trim_start();
    let text = after.strip_prefix(':')?;
    if tag.is_none() && digits == 0 {
        return None;
    }
    Some((tag, text.trim_start()))
}

/// One WordNet definition -- the lines between `151` and `.` -- as an entry,
/// its source recorded as `source`. `None` when the text is not WordNet's
/// shape: no sense lines, or a part of speech this does not know.
#[must_use]
pub fn parse_wordnet(lines: &[String], source: String) -> Option<DictEntry> {
    let (head, rest) = lines.split_first()?;
    let word = head.trim().to_owned();
    if word.is_empty() {
        return None;
    }
    // Each sense's lines, joined, with the part of speech it falls under.
    let mut senses: Vec<(PartOfSpeech, String)> = Vec::new();
    let mut current: Option<PartOfSpeech> = None;
    for line in rest {
        if line.trim().is_empty() {
            continue;
        }
        if let Some((tag, text)) = sense_start(line) {
            if let Some(tag) = tag {
                current = Some(part_of_speech(tag)?);
            }
            senses.push((current?, text.to_owned()));
        } else {
            let (_, text) = senses.last_mut()?;
            text.push(' ');
            text.push_str(line.trim());
        }
    }
    if senses.is_empty() {
        return None;
    }

    let mut entry = DictEntry {
        word,
        pronunciation: String::new(),
        definitions: Vec::new(),
        synonyms: Vec::new(),
        antonyms: Vec::new(),
        etymology: String::new(),
        related: Vec::new(),
        source: Some(source),
    };
    for (part_of_speech, text) in senses {
        let (text, lists) = take_lists(&text);
        for (label, words) in lists {
            let into = match label.as_str() {
                "syn" => &mut entry.synonyms,
                "ant" => &mut entry.antonyms,
                _ => &mut entry.related,
            };
            for w in words {
                // The headword lists itself among its own synonyms; a chip
                // that opens the page it is on goes nowhere.
                if !w.eq_ignore_ascii_case(&entry.word) && !into.contains(&w) {
                    into.push(w);
                }
            }
        }
        let (gloss, examples) = split_examples(&text);
        entry.definitions.push(Definition {
            part_of_speech,
            text: gloss,
            examples,
        });
    }
    Some(entry)
}

/// A sense's text without its bracketed lists, and the lists: `[syn: {glad},
/// {happy}]` becomes `("syn", ["glad", "happy"])`.
fn take_lists(text: &str) -> (String, Vec<(String, Vec<String>)>) {
    let mut kept = String::with_capacity(text.len());
    let mut lists = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let (before, from) = rest.split_at(open);
        let Some(close) = from.find(']') else {
            break;
        };
        let inside = from.get(1..close).unwrap_or_default();
        let Some((label, body)) = inside.split_once(':') else {
            // Brackets that are not a list are part of the gloss.
            kept.push_str(before);
            kept.push_str(from.get(..=close).unwrap_or_default());
            rest = from.get(close.saturating_add(1)..).unwrap_or_default();
            continue;
        };
        kept.push_str(before);
        let words = braced(body);
        lists.push((label.trim().to_owned(), words));
        rest = from.get(close.saturating_add(1)..).unwrap_or_default();
    }
    kept.push_str(rest);
    (kept.split_whitespace().collect::<Vec<_>>().join(" "), lists)
}

/// The `{...}` words in a list, with the whitespace a wrapped line put in
/// them collapsed: `{banking\n concern}` is "banking concern".
fn braced(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find('{') {
        let after = rest.get(open.saturating_add(1)..).unwrap_or_default();
        let Some(close) = after.find('}') else {
            break;
        };
        let word = after
            .get(..close)
            .unwrap_or_default()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !word.is_empty() {
            out.push(word);
        }
        rest = after.get(close.saturating_add(1)..).unwrap_or_default();
    }
    out
}

/// A sense's gloss and its examples: the quoted phrases after the first
/// `; "`, or all of it when it opens with one.
fn split_examples(text: &str) -> (String, Vec<String>) {
    let text = text.trim();
    let (gloss, tail) = if let Some(rest) = text.strip_prefix('"') {
        ("", format!("\"{rest}"))
    } else if let Some(at) = text.find("; \"") {
        (
            text.get(..at).unwrap_or_default(),
            text.get(at.saturating_add(2)..)
                .unwrap_or_default()
                .to_owned(),
        )
    } else {
        (text, String::new())
    };
    let mut examples = Vec::new();
    let mut rest = tail.as_str();
    while let Some(open) = rest.find('"') {
        let after = rest.get(open.saturating_add(1)..).unwrap_or_default();
        let Some(close) = after.find('"') else {
            // An example the server never closed is still the reader's.
            let tail = after.trim();
            if !tail.is_empty() {
                examples.push(tail.to_owned());
            }
            break;
        };
        let example = after.get(..close).unwrap_or_default().trim();
        if !example.is_empty() {
            examples.push(example.to_owned());
        }
        rest = after.get(close.saturating_add(1)..).unwrap_or_default();
    }
    (gloss.trim().to_owned(), examples)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::*;
    use std::net::TcpListener;

    /// dict.org's replies, recorded 2026-09-26 with a script speaking the
    /// same commands this module does, byte for byte (CRLF endings and all).
    /// A fixture written from memory tests the memory.
    const HAPPY: &str = "220 alt0 dictd 1.13.1/rf on Linux 6.12.86+deb13-amd64 <auth.mime> <4386561.904.1790461015@alt0>\r
250 ok\r
150 1 definitions retrieved\r
151 \"happy\" wn \"WordNet (r) 3.0 (2006)\"\r
happy\r
    adj 1: enjoying or showing or marked by joy or pleasure; \"a\r
           happy smile\"; \"spent many happy days on the beach\"; \"a\r
           happy marriage\" [ant: {unhappy}]\r
    2: marked by good fortune; \"a felicitous life\"; \"a happy\r
       outcome\" [syn: {felicitous}, {happy}]\r
    3: eagerly disposed to act or to be of service; \"glad to help\"\r
       [syn: {glad}, {happy}]\r
    4: well expressed and to the point; \"a happy turn of phrase\"; \"a\r
       few well-chosen words\" [syn: {happy}, {well-chosen}]\r
.\r
250 ok [d/m/c = 1/0/16; 0.000r 0.000u 0.000s]\r
221 bye [d/m/c = 0/0/0; 0.000r 0.000u 0.000s]\r
";

    const BANK: &str = "220 alt0 dictd 1.13.1/rf on Linux 6.12.86+deb13-amd64 <auth.mime> <4386562.905.1790461016@alt0>\r
250 ok\r
150 1 definitions retrieved\r
151 \"bank\" wn \"WordNet (r) 3.0 (2006)\"\r
bank\r
    n 1: sloping land (especially the slope beside a body of water);\r
         \"they pulled the canoe up on the bank\"; \"he sat on the bank\r
         of the river and watched the currents\"\r
    2: a financial institution that accepts deposits and channels\r
       the money into lending activities; \"he cashed a check at the\r
       bank\"; \"that bank holds the mortgage on my home\" [syn:\r
       {depository financial institution}, {bank}, {banking\r
       concern}, {banking company}]\r
    3: a long ridge or pile; \"a huge bank of earth\"\r
    4: an arrangement of similar objects in a row or in tiers; \"he\r
       operated a bank of switches\"\r
    5: a supply or stock held in reserve for future use (especially\r
       in emergencies)\r
    6: the funds held by a gambling house or the dealer in some\r
       gambling games; \"he tried to break the bank at Monte Carlo\"\r
    7: a slope in the turn of a road or track; the outside is higher\r
       than the inside in order to reduce the effects of centrifugal\r
       force [syn: {bank}, {cant}, {camber}]\r
    8: a container (usually with a slot in the top) for keeping\r
       money at home; \"the coin bank was empty\" [syn: {savings\r
       bank}, {coin bank}, {money box}, {bank}]\r
    9: a building in which the business of banking transacted; \"the\r
       bank is on the corner of Nassau and Witherspoon\" [syn:\r
       {bank}, {bank building}]\r
    10: a flight maneuver; aircraft tips laterally about its\r
        longitudinal axis (especially in turning); \"the plane went\r
        into a steep bank\"\r
    v 1: tip laterally; \"the pilot had to bank the aircraft\"\r
    2: enclose with a bank; \"bank roads\"\r
    3: do business with a bank or keep an account at a bank; \"Where\r
       do you bank in this town?\"\r
    4: act as the banker in a game or in gambling\r
    5: be in the banking business\r
    6: put into a bank account; \"She deposits her paycheck every\r
       month\" [syn: {deposit}, {bank}] [ant: {draw}, {draw off},\r
       {take out}, {withdraw}]\r
    7: cover with ashes so to control the rate of burning; \"bank a\r
       fire\"\r
    8: have confidence or faith in; \"We can trust in God\"; \"Rely on\r
       your friends\"; \"bank on your good education\"; \"I swear by my\r
       grandmother's recipes\" [syn: {trust}, {swear}, {rely},\r
       {bank}] [ant: {distrust}, {mistrust}, {suspect}]\r
.\r
250 ok [d/m/c = 1/0/17; 0.000r 0.000u 0.000s]\r
221 bye [d/m/c = 0/0/0; 0.000r 0.000u 0.000s]\r
";

    const HAPY: &str = "220 alt0 dictd 1.13.1/rf on Linux 6.12.86+deb13-amd64 <auth.mime> <4386567.917.1790461018@alt0>\r
250 ok\r
552 no match [d/m/c = 0/0/14; 0.000r 0.000u 0.000s]\r
152 6 matches found\r
wn \"hay\"\r
wn \"hap\"\r
wn \"happy\"\r
wn \"harpy\"\r
wn \"haply\"\r
wn \"hazy\"\r
.\r
250 ok [d/m/c = 0/6/9696; 0.000r 0.000u 0.000s]\r
221 bye [d/m/c = 0/0/0; 0.000r 0.000u 0.000s]\r
";

    const NO_MATCH: &str = "220 alt0 dictd 1.13.1/rf on Linux 6.12.86+deb13-amd64 <auth.mime> <4386566.913.1790461018@alt0>\r
250 ok\r
552 no match [d/m/c = 0/0/10; 0.000r 0.000u 0.000s]\r
552 no match [d/m/c = 0/0/8893; 0.000r 0.000u 0.000s]\r
221 bye [d/m/c = 0/0/0; 0.000r 0.000u 0.000s]\r
";

    /// A DICT server on a loopback port that sends `reply` whatever it is
    /// asked, and hands back what it was sent.
    fn server(reply: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let addr = listener.local_addr().expect("its address").to_string();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("the client");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("a timeout");
            stream.write_all(reply.as_bytes()).expect("the reply");
            let mut asked = String::new();
            // Everything the client sends, until it hangs up.
            let _ = stream.read_to_string(&mut asked);
            asked
        });
        (addr, handle)
    }

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(String::from).collect()
    }

    /// **A word is looked up and read into the entry's shape** -- senses,
    /// parts of speech, examples, synonyms, antonyms -- from what dict.org
    /// actually sends, and the conversation is the one the RFC describes.
    #[test]
    fn a_word_is_looked_up_and_read() {
        let (addr, asked) = server(HAPPY);
        let Answer::Found(entry) =
            look_up_with(&addr, "happy", Duration::from_secs(5)).unwrap_or_else(|e| panic!("{e}"))
        else {
            panic!("no entry");
        };
        assert_eq!(entry.word, "happy");
        assert_eq!(entry.source.as_deref(), Some("WordNet (r) 3.0 (2006)"));
        assert_eq!(entry.definitions.len(), 4);
        let first = &entry.definitions[0];
        assert_eq!(first.part_of_speech, PartOfSpeech::Adjective);
        assert_eq!(
            first.text,
            "enjoying or showing or marked by joy or pleasure"
        );
        assert_eq!(
            first.examples,
            [
                "a happy smile",
                "spent many happy days on the beach",
                "a happy marriage"
            ]
        );
        assert!(
            entry
                .definitions
                .iter()
                .all(|d| d.part_of_speech == PartOfSpeech::Adjective),
            "a numbered sense lost the part of speech it falls under"
        );
        assert_eq!(entry.synonyms, ["felicitous", "glad", "well-chosen"]);
        assert_eq!(entry.antonyms, ["unhappy"]);

        let sent = asked.join().expect("the server");
        assert_eq!(
            sent, "CLIENT slateos-dictionary\r\nDEFINE wn \"happy\"\r\nQUIT\r\n",
            "the conversation was not the one the RFC describes"
        );
    }

    /// Two parts of speech, a list wrapped mid-word, two lists on one sense.
    #[test]
    fn a_long_entry_keeps_its_parts_and_its_wrapped_lists() {
        let (addr, _) = server(BANK);
        let Answer::Found(entry) =
            look_up_with(&addr, "bank", Duration::from_secs(5)).unwrap_or_else(|e| panic!("{e}"))
        else {
            panic!("no entry");
        };
        let nouns = entry
            .definitions
            .iter()
            .filter(|d| d.part_of_speech == PartOfSpeech::Noun)
            .count();
        let verbs = entry
            .definitions
            .iter()
            .filter(|d| d.part_of_speech == PartOfSpeech::Verb)
            .count();
        assert_eq!((nouns, verbs), (10, 8));
        assert!(
            entry.synonyms.iter().any(|s| s == "banking concern"),
            "a synonym wrapped across a line was not put back together: {:?}",
            entry.synonyms
        );
        assert!(entry.synonyms.iter().any(|s| s == "savings bank"));
        assert!(entry.antonyms.iter().any(|s| s == "take out"));
        assert!(entry.antonyms.iter().any(|s| s == "suspect"));
        // "10:" is a sense, not a continuation of nine.
        assert_eq!(
            entry.definitions[9].text,
            "a flight maneuver; aircraft tips laterally about its longitudinal axis (especially in turning)"
        );
        // A question in an example is not the end of it.
        assert_eq!(
            entry.definitions[12].examples,
            ["Where do you bank in this town?"]
        );
        // A sense with no examples has none.
        assert!(entry.definitions[4].examples.is_empty());
        assert_eq!(
            entry.definitions[4].text,
            "a supply or stock held in reserve for future use (especially in emergencies)"
        );
    }

    /// No entry: the words one edit away are offered, without the word
    /// itself; and the MATCH asked is the one for suggestions.
    #[test]
    fn a_misspelling_is_answered_with_suggestions() {
        let (addr, asked) = server(HAPY);
        let answer =
            look_up_with(&addr, "hapy", Duration::from_secs(5)).unwrap_or_else(|e| panic!("{e}"));
        let Answer::NotFound { suggestions } = answer else {
            panic!("an entry for a misspelling");
        };
        assert_eq!(
            suggestions,
            ["hay", "hap", "happy", "harpy", "haply", "hazy"]
        );
        let sent = asked.join().expect("the server");
        assert!(sent.contains("MATCH wn lev \"hapy\"\r\n"), "{sent}");
    }

    /// No entry and nothing near it is an answer too -- not an error.
    #[test]
    fn a_word_nobody_has_is_an_answer_not_an_error() {
        let (addr, _) = server(NO_MATCH);
        let answer =
            look_up_with(&addr, "qwzxv", Duration::from_secs(5)).unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(answer, Answer::NotFound { suggestions } if suggestions.is_empty()));
    }

    /// A word that would break the command line is never sent, and never
    /// costs a connection to find that out.
    #[test]
    fn a_word_that_would_break_the_command_is_refused_before_connecting() {
        for word in [
            "",
            "   ",
            "two\r\nQUIT",
            "tab\there",
            &"x".repeat(MAX_WORD + 1),
        ] {
            assert!(
                matches!(quote(word), Err(LookupError::Unsendable(_))),
                "{word:?} would have been sent"
            );
            // Port 9 on loopback: nothing is listening, so a connection
            // attempt would fail with a network error, not this one.
            assert!(
                matches!(
                    look_up_with("127.0.0.1:9", word, Duration::from_secs(1)),
                    Err(LookupError::Unsendable(_))
                ),
                "{word:?} was taken to the network before being refused"
            );
        }
        assert_eq!(quote("ice cream").unwrap(), "\"ice cream\"");
        assert_eq!(quote(r#"say "hi"\"#).unwrap(), r#""say \"hi\"\\""#);
    }

    /// A server that talks for ever is cut off, and it is said to have.
    #[test]
    fn a_reply_past_its_bounds_is_cut_off() {
        let long_line: &'static str =
            Box::leak(format!("220 {}\r\n", "x".repeat(MAX_LINE + 10)).into_boxed_str());
        let (addr, _) = server(long_line);
        let err = look_up_with(&addr, "word", Duration::from_secs(5))
            .expect_err("read a line past the bound");
        assert!(err.to_string().contains("longer"), "{err}");
    }

    /// A refusal at the door is reported with the server's own words.
    #[test]
    fn a_server_that_turns_the_client_away_is_reported() {
        let (addr, _) = server("530 access denied\r\n");
        let err =
            look_up_with(&addr, "word", Duration::from_secs(5)).expect_err("accepted a refusal");
        assert!(err.to_string().contains("530 access denied"), "{err}");
    }

    /// The RFC's dot-stuffing is undone: a line that begins with a dot
    /// arrives with two, and is read with one.
    #[test]
    fn a_line_beginning_with_a_dot_is_read_as_sent() {
        let mut conn = Conn {
            reader: BufReader::new(&b"..NET framework\r\nplain\r\n.\r\n"[..]),
            budget: MAX_REPLY,
        };
        assert_eq!(conn.text().unwrap(), [".NET framework", "plain"]);
    }

    /// Text that is not UTF-8 is read byte for byte, not replaced.
    #[test]
    fn a_line_that_is_not_utf8_loses_no_byte() {
        let mut conn = Conn {
            reader: BufReader::new(&b"caf\xe9\r\n"[..]),
            budget: MAX_REPLY,
        };
        assert_eq!(conn.line().unwrap(), "caf\u{e9}");
    }

    /// Text that is not WordNet's shape is refused, not guessed at.
    #[test]
    fn text_that_is_not_wordnets_is_not_guessed_at() {
        let src = || String::from("x");
        assert!(parse_wordnet(&lines("word\nsome free prose\n"), src()).is_none());
        assert!(
            parse_wordnet(&lines("word\n    prep 1: of\n"), src()).is_none(),
            "an unknown part of speech was filed under another"
        );
        assert!(
            parse_wordnet(
                &lines("word\n    1: a sense with no part of speech\n"),
                src()
            )
            .is_none()
        );
        assert!(parse_wordnet(&[], src()).is_none());
        // One sense whose part of speech comes from the line before.
        let e = parse_wordnet(&lines("ice cream\n    n 1: frozen dessert containing cream and sugar and flavoring\n         [syn: {ice cream}, {icecream}]\n"), src()).expect("an entry");
        assert_eq!(e.definitions.len(), 1);
        assert_eq!(
            e.synonyms,
            ["icecream"],
            "the headword was listed as its own synonym"
        );
    }

    /// A continuation line that happens to begin like a sense -- a number
    /// and a colon -- is still a continuation: only a line indented as a
    /// sense is starts one.
    #[test]
    fn a_continuation_that_looks_like_a_sense_is_not_one() {
        let e = parse_wordnet(
            &lines("word\n    n 1: a sense that wraps onto a line\n         3: beginning with a number\n"),
            String::from("wn"),
        )
        .expect("an entry");
        assert_eq!(
            e.definitions.len(),
            1,
            "a continuation was read as sense three"
        );
        assert!(
            e.definitions[0]
                .text
                .ends_with("3: beginning with a number")
        );
    }

    /// Both definitions of a word WordNet keeps apart by case are kept.
    #[test]
    fn two_definitions_of_one_word_are_one_entry() {
        let a = parse_wordnet(
            &lines("Polish\n    adj 1: of or relating to Poland [syn: {Polish}]\n"),
            String::from("wn"),
        )
        .unwrap();
        let b = parse_wordnet(&lines("polish\n    n 1: a highly developed state of perfection [syn: {polish}, {refinement}]\n    v 1: make (a surface) shine [syn: {polish}, {refine}]\n"), String::from("wn")).unwrap();
        let both = merge(a, b);
        assert_eq!(both.definitions.len(), 3);
        assert_eq!(both.synonyms, ["refinement", "refine"]);
    }
}
