//! The print job format: what an application hands to the printing service.
//!
//! # Why this crate exists
//!
//! `design-decisions.md` §540 settled that printing is a **background service
//! applications submit jobs to**, not a library they link, because a print job
//! is not part of the application's lifetime -- close a viewer after sending
//! forty pages and the pages must keep coming. The consequence it names is the
//! one this crate is:
//!
//! > the application depends on a message format, not on code.
//!
//! Two halves of a printing system already existed and had never been
//! introduced to each other. The PDF viewer knew how to work out *which
//! pages* -- including `1-3, 5, 7-9` -- and the desktop knew about printers,
//! paper, copies and a queue. Neither could reach the other, and each was
//! richer than the other in a different dimension. This is the join: the union
//! of the two vocabularies, owned by neither.
//!
//! # What this crate is not
//!
//! It is **not** a printing library. It has no printers in it, no queue, no
//! I/O and no dependencies. Linking it does not let a program print; it lets a
//! program *say what it wants printed*, in terms the service will understand.
//! §540 is explicit that a shared library was the rejected option, and that
//! the difference that matters is who owns the job.
//!
//! Nothing submits one of these yet, because the service does not exist. That
//! is deliberate: §540 also says *"until the service exists, neither half
//! gains new callers"*, and wiring an application straight to the desktop's
//! printing code is the stop-gap it refused.
//!
//! # The page range came from the PDF viewer, and was not rewritten
//!
//! §540: *"The PDF viewer's page-range parser is the good half and should be
//! lifted into the job format, not reimplemented -- it already handles
//! `1-3, 5, 7-9`, which the desktop's from/to pair cannot express."* So
//! [`PageRange`] is the viewer's model, moved rather than reinvented, and the
//! desktop's single `(start, end)` pair is representable in it as one span.

#![deny(clippy::all, clippy::pedantic)]

/// Which pages of a document to print.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PageRange {
    /// Every page.
    #[default]
    All,
    /// Only the page the reader is looking at.
    ///
    /// Resolved against a *current page* the sender supplies, because the
    /// service has no idea what anyone is looking at. It stays a distinct
    /// variant rather than being flattened to a one-page span at parse time so
    /// that a dialog can offer "Current page" as a choice and read it back.
    CurrentPage,
    /// Half-open-free inclusive spans of zero-based page indices.
    ///
    /// `[(0, 2), (4, 4)]` is "pages 1-3 and 5" as a user would say it. Spans
    /// rather than a flat list of pages because that is what the user typed,
    /// and because a thousand-page document's "all but the last" should not
    /// cost a thousand entries.
    Custom(Vec<(usize, usize)>),
}

impl PageRange {
    /// Parse what a user typed into a page-range box.
    ///
    /// Accepts `1-3, 5, 7-9`, single pages, whitespace anywhere, and the words
    /// an empty box implies. One-based going in, zero-based coming out,
    /// because that is the boundary between what a person says and what a
    /// document is indexed by, and it should be crossed exactly once.
    ///
    /// # What it does with nonsense
    ///
    /// Ignores it, span by span, rather than failing the whole input. A user
    /// typing `1-3, x, 7` means the two ranges they got right; refusing all of
    /// it teaches them to distrust the box. A span that begins past the last
    /// page names no pages and is dropped for the same reason.
    ///
    /// A reversed span (`9-7`) is read as the span it plainly means. Refusing
    /// it would be technically defensible and useless: nobody types `9-7`
    /// meaning nothing.
    #[must_use]
    pub fn parse(input: &str, page_count: usize) -> Self {
        let trimmed = input.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("all") {
            return Self::All;
        }

        let mut spans = Vec::new();
        for part in trimmed.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let Some((lo, hi)) = Self::parse_span(part) else {
                continue;
            };
            // One-based to zero-based, once, here.
            let (lo, hi) = (lo.saturating_sub(1), hi.saturating_sub(1));
            let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
            if lo >= page_count {
                continue;
            }
            spans.push((lo, hi.min(page_count.saturating_sub(1))));
        }

        if spans.is_empty() {
            // Everything the user typed was unusable. `All` would print the
            // whole document, which is the most expensive possible way to be
            // wrong about a print job.
            return Self::Custom(Vec::new());
        }
        Self::Custom(spans)
    }

    /// One `7` or `7-9`, one-based, or `None` if it is neither.
    fn parse_span(part: &str) -> Option<(usize, usize)> {
        if let Some((a, b)) = part.split_once('-') {
            let lo = a.trim().parse::<usize>().ok()?;
            let hi = b.trim().parse::<usize>().ok()?;
            if lo == 0 || hi == 0 {
                return None;
            }
            Some((lo, hi))
        } else {
            let only = part.parse::<usize>().ok()?;
            if only == 0 {
                return None;
            }
            Some((only, only))
        }
    }

    /// The zero-based page indices this range names, in order and without
    /// duplicates.
    ///
    /// Overlapping spans (`1-5, 3-7`) print each page once. A user who typed
    /// both meant the union; printing page 3 twice is a waste of paper they
    /// did not ask for.
    #[must_use]
    pub fn resolve(&self, page_count: usize, current_page: usize) -> Vec<usize> {
        match self {
            Self::All => (0..page_count).collect(),
            Self::CurrentPage => {
                if current_page < page_count {
                    vec![current_page]
                } else {
                    Vec::new()
                }
            }
            Self::Custom(spans) => {
                let mut pages: Vec<usize> = spans
                    .iter()
                    .filter(|(lo, _)| *lo < page_count)
                    .flat_map(|(lo, hi)| *lo..=(*hi).min(page_count.saturating_sub(1)))
                    .collect();
                pages.sort_unstable();
                pages.dedup();
                pages
            }
        }
    }

    /// The single span the desktop's older `(start, end)` pair expresses, if
    /// this range is one.
    ///
    /// For talking to code that predates this format. `None` when the range
    /// cannot be said that way -- which is most of why this format exists.
    #[must_use]
    pub fn as_single_span(&self) -> Option<(usize, usize)> {
        match self {
            Self::Custom(spans) if spans.len() == 1 => spans.first().copied(),
            _ => None,
        }
    }
}

/// Paper size, in the vocabulary the desktop already uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaperSize {
    A3,
    #[default]
    A4,
    A5,
    Letter,
    Legal,
    Tabloid,
}

/// How much ink and time to spend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quality {
    Draft,
    #[default]
    Normal,
    High,
}

/// Which way up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    #[default]
    Portrait,
    Landscape,
}

/// Colour or not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorMode {
    #[default]
    Color,
    Grayscale,
    Monochrome,
}

/// Everything an application says about how it wants a document printed.
///
/// The union of the two vocabularies §540 describes: the range from the
/// viewer, the rest from the desktop. Deliberately without a printer, a job
/// id, a state or a timestamp -- those belong to the *service*, which assigns
/// them, and an application that could set them would be an application that
/// could claim someone else's job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintJob {
    /// What to call this in a queue the user is looking at.
    pub document_name: String,
    /// Which pages.
    pub range: PageRange,
    /// The page the sender is looking at, for [`PageRange::CurrentPage`].
    ///
    /// Carried rather than resolved before sending so that the job says what
    /// the user chose, not what it happened to mean at that instant -- a
    /// dialog that reopens should read back "Current page", not "page 7".
    pub current_page: usize,
    /// How many pages the document has, so the service can resolve a range
    /// without being able to read the document.
    pub page_count: usize,
    /// How many copies. Zero is not a print job; see [`Self::validate`].
    pub copies: u32,
    pub paper: PaperSize,
    pub orientation: Orientation,
    pub quality: Quality,
    pub color: ColorMode,
    pub duplex: bool,
    /// Collate multi-copy output (1,2,3 / 1,2,3 rather than 1,1 / 2,2 / 3,3).
    pub collate: bool,
    /// Percentage scale; 100 is actual size.
    pub scale_percent: u32,
}

impl Default for PrintJob {
    fn default() -> Self {
        Self {
            document_name: String::new(),
            range: PageRange::All,
            current_page: 0,
            page_count: 0,
            copies: 1,
            paper: PaperSize::default(),
            orientation: Orientation::default(),
            quality: Quality::default(),
            color: ColorMode::default(),
            duplex: false,
            collate: true,
            scale_percent: 100,
        }
    }
}

/// Why a job cannot be printed as asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalid {
    /// Zero copies, which is a request to do nothing rather than to print.
    NoCopies,
    /// A scale of zero, or one so large the page cannot hold any of it.
    ImpossibleScale,
    /// The range names no page of this document.
    NoPages,
}

impl PrintJob {
    /// The pages this job actually prints, in order.
    #[must_use]
    pub fn pages(&self) -> Vec<usize> {
        self.range.resolve(self.page_count, self.current_page)
    }

    /// Refuse a job that cannot mean anything.
    ///
    /// Checked here, in the format, rather than in each sender: a service that
    /// trusted its callers would be a service whose queue fills with jobs that
    /// print nothing, and every application would have to remember the same
    /// three rules.
    ///
    /// # Errors
    ///
    /// The first thing wrong with it, in the order a user would notice.
    pub fn validate(&self) -> Result<(), Invalid> {
        if self.copies == 0 {
            return Err(Invalid::NoCopies);
        }
        if self.scale_percent == 0 || self.scale_percent > 1000 {
            return Err(Invalid::ImpossibleScale);
        }
        if self.pages().is_empty() {
            return Err(Invalid::NoPages);
        }
        Ok(())
    }

    /// Total sheets of paper, given the copies and duplex.
    ///
    /// Duplex halves it, rounding *up*: eleven pages double-sided is six
    /// sheets, not five and a half.
    #[must_use]
    pub fn sheets(&self) -> usize {
        let pages = self.pages().len();
        let per_copy = if self.duplex {
            pages.div_ceil(2)
        } else {
            pages
        };
        per_copy.saturating_mul(self.copies as usize)
    }
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it. The defensive lints exist to keep panics out of code
    // that runs on a user's data, which this is not.
    #![allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn the_range_the_desktop_could_not_express_round_trips() {
        // The example from design-decisions 540, which is the reason this
        // format is not just the desktop's `(start, end)` pair.
        let range = PageRange::parse("1-3, 5, 7-9", 20);
        assert_eq!(range.resolve(20, 0), vec![0, 1, 2, 4, 6, 7, 8]);
        assert_eq!(
            range.as_single_span(),
            None,
            "a discontiguous range must not pretend to be one span"
        );
    }

    #[test]
    fn an_empty_box_means_every_page() {
        for input in ["", "   ", "all", "ALL"] {
            assert_eq!(PageRange::parse(input, 5), PageRange::All, "{input:?}");
        }
    }

    /// Nonsense is ignored span by span, not fatal to the whole input.
    #[test]
    fn a_bad_span_does_not_throw_away_the_good_ones() {
        let range = PageRange::parse("1-3, x, 7", 10);
        assert_eq!(range.resolve(10, 0), vec![0, 1, 2, 6]);
    }

    /// But input that is *entirely* unusable prints nothing, not everything.
    #[test]
    fn wholly_unusable_input_prints_nothing_rather_than_the_document() {
        let range = PageRange::parse("x, y, 0", 10);
        assert!(
            range.resolve(10, 0).is_empty(),
            "unreadable input fell back to printing the whole document, which \
             is the most expensive way to be wrong about a print job"
        );
    }

    #[test]
    fn a_span_past_the_end_is_dropped_and_one_that_straddles_is_clipped() {
        assert!(PageRange::parse("50-60", 10).resolve(10, 0).is_empty());
        assert_eq!(PageRange::parse("8-60", 10).resolve(10, 0), vec![7, 8, 9]);
    }

    #[test]
    fn a_reversed_span_is_read_as_what_it_plainly_means() {
        assert_eq!(PageRange::parse("9-7", 10).resolve(10, 0), vec![6, 7, 8]);
    }

    /// Overlapping spans print each page once.
    #[test]
    fn overlapping_spans_do_not_print_a_page_twice() {
        assert_eq!(
            PageRange::parse("1-5, 3-7", 10).resolve(10, 0),
            vec![0, 1, 2, 3, 4, 5, 6]
        );
    }

    #[test]
    fn page_numbers_are_one_based_going_in_and_zero_based_coming_out() {
        assert_eq!(PageRange::parse("1", 10).resolve(10, 0), vec![0]);
        assert_eq!(
            PageRange::parse("0", 10).resolve(10, 0),
            Vec::<usize>::new(),
            "there is no page zero, and it must not become page one"
        );
    }

    #[test]
    fn the_current_page_is_resolved_against_what_the_sender_was_looking_at() {
        let range = PageRange::CurrentPage;
        assert_eq!(range.resolve(10, 4), vec![4]);
        assert!(
            range.resolve(10, 99).is_empty(),
            "a current page past the end names no page"
        );
    }

    #[test]
    fn a_job_with_no_copies_is_refused() {
        let job = PrintJob {
            page_count: 5,
            copies: 0,
            ..PrintJob::default()
        };
        assert_eq!(job.validate(), Err(Invalid::NoCopies));
    }

    #[test]
    fn a_job_whose_range_names_no_page_is_refused() {
        let job = PrintJob {
            page_count: 5,
            range: PageRange::parse("50-60", 5),
            ..PrintJob::default()
        };
        assert_eq!(job.validate(), Err(Invalid::NoPages));
    }

    #[test]
    fn an_ordinary_job_is_accepted() {
        let job = PrintJob {
            document_name: "report.pdf".to_string(),
            page_count: 10,
            range: PageRange::parse("1-3", 10),
            ..PrintJob::default()
        };
        assert_eq!(job.validate(), Ok(()));
        assert_eq!(job.pages(), vec![0, 1, 2]);
    }

    /// Duplex rounds up: eleven pages is six sheets, not five.
    #[test]
    fn duplex_halves_the_sheets_rounding_up() {
        let job = PrintJob {
            page_count: 11,
            duplex: true,
            ..PrintJob::default()
        };
        assert_eq!(job.sheets(), 6);
    }

    #[test]
    fn copies_multiply_the_sheets() {
        let job = PrintJob {
            page_count: 4,
            copies: 3,
            ..PrintJob::default()
        };
        assert_eq!(job.sheets(), 12);
    }

    /// The desktop's single-pair form survives the trip when it is expressible.
    #[test]
    fn a_single_span_can_still_be_read_as_the_old_pair() {
        assert_eq!(
            PageRange::parse("3-7", 10).as_single_span(),
            Some((2, 6)),
            "the old from/to form must still be recoverable"
        );
        assert_eq!(
            PageRange::All.as_single_span(),
            None,
            "\"all\" is not a span; a document's length is the service's to know"
        );
    }
}
