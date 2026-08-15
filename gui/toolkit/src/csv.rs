//! RFC 4180 comma-separated values: both directions in one place.
//!
//! Writing and reading CSV are not two separate problems, and the bugs this
//! module exists to prevent all came from treating them as if they were. Two
//! apps (`dbviewer` and `spreadsheet`) independently grew a correct,
//! quote-aware *field* parser and then fed it records produced by splitting
//! the input on `\n` — which tears a quoted field containing a newline in
//! half before the field parser ever sees it. Both could therefore produce an
//! export that they themselves could not read back. Keeping the writer and
//! the reader adjacent is what makes that asymmetry visible.
//!
//! The two entry points are [`field`] (write one field) and [`parse_records`]
//! (read a whole document). Neither trims: whitespace is data as far as this
//! module is concerned. A caller that wants the lenient "trim unquoted
//! fields" import convention can apply it itself, using [`Field::quoted`] to
//! tell the two cases apart — quoting is precisely how a writer says the
//! surrounding spaces are significant.

/// Render a string as one field of a CSV record, per RFC 4180.
///
/// The field is wrapped in quotes, with any embedded quote doubled, exactly
/// when it contains a character that would otherwise end the field or the
/// record: a comma, a quote, or a line break. Fields without one are returned
/// unchanged, which keeps ordinary output readable.
///
/// `\r` belongs in that set as much as `\n` does. RFC 4180 records are
/// CRLF-terminated, so a bare CR inside a field splits the record in most
/// readers even though it looks harmless in a terminal.
///
/// ```
/// # use guitk::csv;
/// assert_eq!(csv::field("plain"), "plain");
/// assert_eq!(csv::field("a,b"), "\"a,b\"");
/// assert_eq!(csv::field("say \"hi\""), "\"say \"\"hi\"\"\"");
/// ```
#[must_use]
pub fn field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// One decoded field, plus whether the source spelled it in quotes.
///
/// The flag is not decoration: it is the only thing that distinguishes a
/// field whose leading spaces were deliberate from one where they were
/// incidental, and it is what lets a lenient importer trim the second without
/// corrupting the first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The field's contents, with quoting removed and doubled quotes undone.
    pub text: String,
    /// Whether the field appeared in quotes in the source.
    pub quoted: bool,
}

impl Field {
    /// The text, trimmed only if the field was *not* quoted.
    ///
    /// This is the lenient-import convention: a bare `  x  ` in a
    /// hand-written file almost certainly means `x`, but `"  x  "` means what
    /// it says.
    #[must_use]
    pub fn trimmed_if_bare(&self) -> &str {
        if self.quoted {
            &self.text
        } else {
            self.text.trim()
        }
    }
}

/// Parse CSV text into records of decoded fields, per RFC 4180.
///
/// This is record-oriented rather than line-oriented on purpose: a quoted
/// field may contain a newline, so splitting on `\n` before parsing fields
/// tears such a field in half and drops the rest of the record.
///
/// Lenient where the RFC is silent or where real files vary: a lone `\n`
/// terminates a record just as `\r\n` does, an unterminated final record is
/// returned rather than discarded, and an unclosed quote runs to end of input
/// instead of erroring — a truncated file should still yield the rows it
/// does have.
///
/// ```
/// # use guitk::csv;
/// let records = csv::parse_records("a,b\n\"x,y\",\"two\nlines\"");
/// assert_eq!(records.len(), 2);
/// assert_eq!(records[1][0].text, "x,y");
/// assert_eq!(records[1][1].text, "two\nlines");
/// ```
#[must_use]
pub fn parse_records(data: &str) -> Vec<Vec<Field>> {
    let mut records: Vec<Vec<Field>> = Vec::new();
    let mut record: Vec<Field> = Vec::new();
    let mut text = String::new();
    let mut quoted = false;
    let mut in_quotes = false;
    let mut chars = data.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    text.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                text.push(c);
            }
            continue;
        }
        match c {
            '"' => {
                in_quotes = true;
                quoted = true;
            }
            ',' => record.push(Field {
                text: std::mem::take(&mut text),
                quoted: std::mem::take(&mut quoted),
            }),
            '\n' => {
                record.push(Field {
                    text: std::mem::take(&mut text),
                    quoted: std::mem::take(&mut quoted),
                });
                records.push(std::mem::take(&mut record));
            }
            // A CR outside quotes is the first half of a CRLF terminator; the
            // LF that follows ends the record.
            '\r' => {}
            _ => text.push(c),
        }
    }
    // A trailing newline ends the last record cleanly and must not manufacture
    // an empty one; anything else left over is a real, unterminated record.
    if in_quotes || !text.is_empty() || !record.is_empty() {
        record.push(Field { text, quoted });
        records.push(record);
    }
    records
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    // -- writing ---------------------------------------------------------

    #[test]
    fn an_ordinary_field_is_left_unquoted() {
        assert_eq!(field("plain text 42"), "plain text 42");
        assert_eq!(field(""), "");
    }

    #[test]
    fn every_character_that_could_end_a_field_forces_quoting() {
        assert_eq!(field("a,b"), "\"a,b\"");
        assert_eq!(field("a\nb"), "\"a\nb\"");
        // The one the hand-rolled copies kept forgetting.
        assert_eq!(field("a\rb"), "\"a\rb\"");
        assert_eq!(field("a\"b"), "\"a\"\"b\"");
    }

    /// The property that matters for the writer on its own: whatever a field
    /// contains, the rendered record still has exactly as many fields as it
    /// was built from, and none of them escaped into a new record.
    #[test]
    fn a_hostile_field_cannot_add_a_column_or_a_row() {
        let row = ["ok", "evil,injected", "x\"y", "line\nbreak", "cr\rhere"];
        let rendered: Vec<String> = row.iter().map(|f| field(f)).collect();
        let line = rendered.join(",");

        // Count separators that are outside quotes -- the real field count.
        let mut in_quotes = false;
        let mut fields = 1;
        let mut newlines_outside = 0;
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' if in_quotes && chars.peek() == Some(&'"') => {
                    chars.next();
                }
                '"' => in_quotes = !in_quotes,
                ',' if !in_quotes => fields += 1,
                '\n' | '\r' if !in_quotes => newlines_outside += 1,
                _ => {}
            }
        }
        assert_eq!(fields, row.len(), "field count changed: {line}");
        assert_eq!(newlines_outside, 0, "a field broke the record: {line}");
        assert!(!in_quotes, "unbalanced quotes: {line}");
    }

    // -- reading ---------------------------------------------------------

    #[test]
    fn a_quoted_newline_stays_inside_its_field() {
        let records = parse_records("a,b\n\"one\ntwo\",c");
        assert_eq!(records.len(), 2, "record torn in half: {records:?}");
        assert_eq!(records[1][0].text, "one\ntwo");
        assert_eq!(records[1][1].text, "c");
    }

    #[test]
    fn crlf_and_lf_both_terminate_a_record() {
        assert_eq!(parse_records("a,b\r\nc,d").len(), 2);
        assert_eq!(parse_records("a,b\nc,d").len(), 2);
    }

    #[test]
    fn a_trailing_newline_does_not_add_an_empty_record() {
        assert_eq!(parse_records("a,b\n").len(), 1);
        assert_eq!(parse_records("a,b\r\n").len(), 1);
    }

    #[test]
    fn an_empty_input_yields_no_records() {
        assert!(parse_records("").is_empty());
    }

    #[test]
    fn a_doubled_quote_decodes_to_one() {
        let records = parse_records("\"say \"\"hi\"\"\"");
        assert_eq!(records[0][0].text, "say \"hi\"");
    }

    #[test]
    fn an_unclosed_quote_yields_the_rest_of_the_input() {
        // A truncated file should still give up the data it does contain.
        let records = parse_records("a,\"unterminated");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0][1].text, "unterminated");
    }

    #[test]
    fn quoting_is_reported_so_a_caller_can_trim_safely() {
        let records = parse_records("  bare  ,\"  quoted  \"");
        assert!(!records[0][0].quoted);
        assert!(records[0][1].quoted);
        assert_eq!(records[0][0].trimmed_if_bare(), "bare");
        assert_eq!(records[0][1].trimmed_if_bare(), "  quoted  ");
    }

    #[test]
    fn empty_fields_are_preserved() {
        let records = parse_records("a,,c");
        assert_eq!(records[0].len(), 3);
        assert_eq!(records[0][1].text, "");
    }

    // -- the property that ties the two halves together --------------------

    #[test]
    fn anything_written_can_be_read_back() {
        let hostile = [
            "plain",
            "",
            "a,b",
            "a\nb",
            "a\rb",
            "say \"hi\"",
            "\"leading quote",
            "trailing quote\"",
            "  spaces  ",
            ",,,",
            "\"\"\"\"",
            "mixed \",\" and \n newline",
        ];
        let line: Vec<String> = hostile.iter().map(|s| field(s)).collect();
        let records = parse_records(&line.join(","));
        assert_eq!(records.len(), 1, "a field forged a record: {records:?}");
        let got: Vec<&str> = records[0].iter().map(|f| f.text.as_str()).collect();
        assert_eq!(got, hostile, "round trip changed the data");
    }

    #[test]
    fn a_multi_record_document_round_trips() {
        let rows = [
            vec!["id", "note"],
            vec!["1", "has, comma"],
            vec!["2", "has\nnewline"],
            vec!["3", "has \"quotes\""],
        ];
        let text: String = rows
            .iter()
            .map(|r| r.iter().map(|c| field(c)).collect::<Vec<_>>().join(","))
            .collect::<Vec<_>>()
            .join("\n");
        let records = parse_records(&text);
        assert_eq!(records.len(), rows.len());
        for (got, want) in records.iter().zip(rows.iter()) {
            let got: Vec<&str> = got.iter().map(|f| f.text.as_str()).collect();
            assert_eq!(&got, want);
        }
    }
}
