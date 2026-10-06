//! What TTML needs of XML: XML 1.0 documents in UTF-8, read into a tree
//! with every element's and attribute's name resolved against its
//! namespace (Namespaces in XML 1.0).
//!
//! A subtitle sample is a stranger's document, so this reader is strict
//! where XML is -- a document that is not well-formed is refused whole, as
//! expat (which ttconv reads through) refuses it -- and bounded where XML is
//! not: elements nest at most [`MAX_DEPTH`] deep, and no entity expands to
//! more than a character. A DOCTYPE's declarations are passed over unread,
//! and an entity other than XML's five and the character references is an
//! error, so no document can make the reader expand one entity into many
//! (the "billion laughs").
//!
//! What it does not do, as TTML does not need it: other encodings than
//! UTF-8, validation, default attributes from a DTD.
//!
//! What a document costs to read is its length, in time and in memory. The
//! tree borrows from the document: a name, an attribute's value or a run of
//! text is a slice of it, copied only where a reference or a line ending
//! makes it other than as written. A namespace is one string, shared by
//! every name in it -- each name its own copy, a namespace a megabyte long
//! in a document of a hundred thousand elements would be a hundred
//! gigabytes. An element's attributes are told apart by sorting their
//! names, not each against every other, and a prefix is resolved by a hash
//! of the bindings in scope, not a walk of every declaration -- either of
//! which a document of a hundred thousand attributes or declarations would
//! make quadratic.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// The namespace the `xml` prefix is bound to, always.
pub(crate) const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// The namespace the `xmlns` prefix is bound to, always: no declaration may
/// bind a prefix to it.
const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";

/// How deep elements may nest. TTML's documents are a handful deep -- `tt`,
/// `body`, `div`, `p`, `span` -- and a document nesting more is made to
/// exhaust whoever walks it.
pub(crate) const MAX_DEPTH: usize = 64;

/// A name as XML resolves it: its namespace (empty for none) and its local
/// part.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Name<'a> {
    /// The one string the document's names in this namespace share.
    pub namespace: Rc<str>,
    pub local: &'a str,
}

impl Name<'_> {
    /// Whether this is `local` in `namespace`.
    pub(crate) fn is(&self, namespace: &str, local: &str) -> bool {
        *self.namespace == *namespace && self.local == local
    }
}

/// An element: its name, its attributes in the order written (the
/// namespace declarations not among them), and what it holds.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Element<'a> {
    pub name: Name<'a>,
    pub attributes: Vec<(Name<'a>, Cow<'a, str>)>,
    pub children: Vec<Node<'a>>,
}

impl<'a> Element<'a> {
    /// The value of the attribute named `local` in `namespace`.
    pub(crate) fn attribute(&self, namespace: &str, local: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(name, _)| name.is(namespace, local))
            .map(|(_, value)| &**value)
    }

    /// The elements it holds, in order.
    pub(crate) fn elements(&self) -> impl Iterator<Item = &Element<'a>> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }
}

/// What an element holds: elements, and text -- character data, CDATA
/// sections and references, run together between elements.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Node<'a> {
    Element(Element<'a>),
    Text(Cow<'a, str>),
}

/// Why a document is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum XmlError {
    /// The bytes are not UTF-8.
    NotUtf8,
    /// Not well-formed: what was wrong.
    Malformed(&'static str),
    /// An entity other than XML's five, or a character reference to no
    /// character XML allows.
    Reference,
    /// A prefix no namespace declaration in scope binds.
    UnboundPrefix,
    /// Elements nested past [`MAX_DEPTH`].
    TooDeep,
}

/// `input` as an XML document: its root element, borrowing from `input`.
///
/// # Errors
///
/// [`XmlError`] for a document not in UTF-8, not well-formed, using a
/// prefix it does not bind, or nested too deep.
pub(crate) fn parse(input: &[u8]) -> Result<Element<'_>, XmlError> {
    let text = core::str::from_utf8(input).map_err(|_| XmlError::NotUtf8)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // Anywhere -- in a comment too, as expat refuses it there.
    if !text.chars().all(is_xml_char) {
        return Err(XmlError::Malformed("a character XML does not allow"));
    }
    let none: Rc<str> = Rc::from("");
    let xml: Rc<str> = Rc::from(XML_NAMESPACE);
    Parser {
        rest: text,
        open: Vec::new(),
        scopes: Vec::new(),
        bindings: HashMap::new(),
        namespaces: HashSet::from([Rc::clone(&none), Rc::clone(&xml)]),
        none,
        xml,
        root: None,
        raw: Vec::new(),
        written: Vec::new(),
        expanded: Vec::new(),
    }
    .document()
}

/// An element begun and not yet ended: its name as written, for its end
/// tag, and the element so far.
struct Open<'a> {
    written: &'a str,
    element: Element<'a>,
}

struct Parser<'a> {
    rest: &'a str,
    /// The elements begun and not yet ended, outermost first.
    open: Vec<Open<'a>>,
    /// The prefixes each open element declares, outermost first: the empty
    /// prefix the default namespace.
    scopes: Vec<Vec<&'a str>>,
    /// Each prefix's namespaces in scope, the innermost declaration's last.
    bindings: HashMap<&'a str, Vec<Rc<str>>>,
    /// Every namespace met, once each: a name's namespace is one of these,
    /// so two names are in one namespace exactly when they share it.
    namespaces: HashSet<Rc<str>>,
    /// No namespace, and the `xml` prefix's: two of `namespaces`.
    none: Rc<str>,
    xml: Rc<str>,
    root: Option<Element<'a>>,
    /// The element being begun's attributes as written, their names sorted,
    /// and their names resolved -- each namespace by its string's address
    /// -- sorted: kept from element to element, so an element of a few
    /// attributes allocates for none of them.
    raw: Vec<(&'a str, Cow<'a, str>)>,
    written: Vec<&'a str>,
    expanded: Vec<(usize, &'a str)>,
}

/// What a run of a document is: what XML reads in it differs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Run {
    /// Character data between markup.
    Text,
    /// A CDATA section's: no reference is read in it.
    Cdata,
    /// An attribute's value.
    Value,
}

impl<'a> Parser<'a> {
    fn document(mut self) -> Result<Element<'a>, XmlError> {
        loop {
            if self.rest.is_empty() {
                break;
            }
            if let Some(after) = self.rest.strip_prefix("<?") {
                self.rest = after;
                self.skip_past("?>", "a processing instruction not ended")?;
            } else if let Some(after) = self.rest.strip_prefix("<!--") {
                self.rest = after;
                self.skip_past("-->", "a comment not ended")?;
            } else if self.rest.starts_with("<!DOCTYPE") {
                if self.root.is_some() || !self.open.is_empty() {
                    return Err(XmlError::Malformed("a DOCTYPE after the root"));
                }
                self.doctype()?;
            } else if let Some(after) = self.rest.strip_prefix("<![CDATA[") {
                if self.open.is_empty() {
                    return Err(XmlError::Malformed("CDATA outside the root"));
                }
                let end = after
                    .find("]]>")
                    .ok_or(XmlError::Malformed("a CDATA section not ended"))?;
                let (data, tail) = after.split_at(end);
                self.text(as_read(data, Run::Cdata)?);
                self.rest = tail.get(3..).unwrap_or("");
            } else if let Some(after) = self.rest.strip_prefix("</") {
                self.rest = after;
                self.end_tag()?;
            } else if self.rest.starts_with('<') {
                self.start_tag()?;
            } else {
                let end = self.rest.find('<').unwrap_or(self.rest.len());
                let (chars, tail) = self.rest.split_at(end);
                self.rest = tail;
                if self.open.is_empty() {
                    if !chars.chars().all(is_space) {
                        return Err(XmlError::Malformed("text outside the root"));
                    }
                } else {
                    let value = as_read(chars, Run::Text)?;
                    self.text(value);
                }
            }
        }
        if !self.open.is_empty() {
            return Err(XmlError::Malformed("an element not ended"));
        }
        self.root.ok_or(XmlError::Malformed("no root element"))
    }

    /// Past the next `end`, or the error `what`.
    fn skip_past(&mut self, end: &str, what: &'static str) -> Result<(), XmlError> {
        let at = self.rest.find(end).ok_or(XmlError::Malformed(what))?;
        self.rest = self.rest.get(at.saturating_add(end.len())..).unwrap_or("");
        Ok(())
    }

    /// A DOCTYPE, passed over: its internal subset's declarations are not
    /// read, so none of its entities is ever expanded.
    fn doctype(&mut self) -> Result<(), XmlError> {
        let mut depth = 0usize;
        let mut quote: Option<char> = None;
        for (i, c) in self.rest.char_indices() {
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (Some(_), _) => {}
                (None, '"' | '\'') => quote = Some(c),
                (None, '[') => depth = depth.saturating_add(1),
                (None, ']') => depth = depth.saturating_sub(1),
                (None, '>') if depth == 0 => {
                    self.rest = self.rest.get(i.saturating_add(1)..).unwrap_or("");
                    return Ok(());
                }
                _ => {}
            }
        }
        Err(XmlError::Malformed("a DOCTYPE not ended"))
    }

    /// Text into the element open innermost, run together with the text
    /// before it.
    fn text(&mut self, value: Cow<'a, str>) {
        if value.is_empty() {
            return;
        }
        let Some(open) = self.open.last_mut() else {
            return;
        };
        if let Some(Node::Text(before)) = open.element.children.last_mut() {
            before.to_mut().push_str(&value);
        } else {
            open.element.children.push(Node::Text(value));
        }
    }

    fn start_tag(&mut self) -> Result<(), XmlError> {
        if self.root.is_some() {
            return Err(XmlError::Malformed("a second root element"));
        }
        if self.open.len() >= MAX_DEPTH {
            return Err(XmlError::TooDeep);
        }
        let after = self.rest.get(1..).unwrap_or("");
        let (written, mut tail) = name(after)?;
        let mut raw = std::mem::take(&mut self.raw);
        raw.clear();
        let empty = loop {
            let trimmed = tail.trim_start_matches(is_space);
            let spaced = trimmed.len() < tail.len();
            tail = trimmed;
            if let Some(t) = tail.strip_prefix("/>") {
                tail = t;
                break true;
            }
            if let Some(t) = tail.strip_prefix('>') {
                tail = t;
                break false;
            }
            if !spaced {
                return Err(XmlError::Malformed("attributes not apart"));
            }
            let (attribute, t) = name(tail)?;
            let t = t.trim_start_matches(is_space);
            let t = t
                .strip_prefix('=')
                .ok_or(XmlError::Malformed("an attribute without a value"))?
                .trim_start_matches(is_space);
            let quote = t
                .chars()
                .next()
                .filter(|&q| q == '"' || q == '\'')
                .ok_or(XmlError::Malformed("an attribute's value not quoted"))?;
            let t = t.get(1..).unwrap_or("");
            let end = t
                .find(quote)
                .ok_or(XmlError::Malformed("an attribute's value not ended"))?;
            let (value, t) = t.split_at(end);
            if value.contains('<') {
                return Err(XmlError::Malformed("a '<' in an attribute's value"));
            }
            raw.push((attribute, as_read(value, Run::Value)?));
            tail = t.get(1..).unwrap_or("");
        };
        self.rest = tail;
        // Each attribute written once: sorted, two written alike are side by
        // side.
        self.written.clear();
        self.written
            .extend(raw.iter().map(|&(attribute, _)| attribute));
        self.written.sort_unstable();
        if self
            .written
            .windows(2)
            .any(|w| matches!(w, [a, b] if a == b))
        {
            return Err(XmlError::Malformed("an attribute given twice"));
        }
        // The declarations first: they are in scope on the element itself.
        let mut declared = Vec::new();
        for (attribute, value) in &raw {
            if let Some(prefix) = declares(attribute)? {
                self.declare(prefix, value)?;
                declared.push(prefix);
            }
        }
        let declarations = declared.len();
        self.scopes.push(declared);
        let element_name = self.resolve(written, true)?;
        let mut attributes = Vec::with_capacity(raw.len().saturating_sub(declarations));
        for (attribute, value) in raw.drain(..) {
            if declares(attribute)?.is_none() {
                attributes.push((self.resolve(attribute, false)?, value));
            }
        }
        self.raw = raw;
        // Each name resolved once: two written apart may resolve alike, a
        // namespace's by two prefixes.
        self.expanded.clear();
        self.expanded.extend(
            attributes
                .iter()
                .map(|(n, _)| (Rc::as_ptr(&n.namespace).cast::<u8>().addr(), n.local)),
        );
        self.expanded.sort_unstable();
        if self
            .expanded
            .windows(2)
            .any(|w| matches!(w, [a, b] if a == b))
        {
            return Err(XmlError::Malformed("an attribute given twice"));
        }
        self.open.push(Open {
            written,
            element: Element {
                name: element_name,
                attributes,
                children: Vec::new(),
            },
        });
        if empty {
            self.close();
        }
        Ok(())
    }

    /// `prefix` bound to `uri` -- the empty prefix the default namespace --
    /// as Namespaces in XML allows: never the `xmlns` prefix, the `xml`
    /// prefix only to its own namespace, and no other prefix to either's.
    fn declare(&mut self, prefix: &'a str, uri: &str) -> Result<(), XmlError> {
        if prefix == "xmlns" {
            return Err(XmlError::Malformed("the xmlns prefix declared"));
        }
        if prefix == "xml" {
            if uri != XML_NAMESPACE {
                return Err(XmlError::Malformed(
                    "the xml prefix bound to another namespace",
                ));
            }
        } else if uri == XML_NAMESPACE || uri == XMLNS_NAMESPACE {
            return Err(XmlError::Malformed(
                "a prefix bound to a reserved namespace",
            ));
        } else if uri.is_empty() && !prefix.is_empty() {
            return Err(XmlError::Malformed("a prefix bound to no namespace"));
        }
        let namespace = self.intern(uri);
        self.bindings.entry(prefix).or_default().push(namespace);
        Ok(())
    }

    /// `uri` as the one string its namespace is.
    fn intern(&mut self, uri: &str) -> Rc<str> {
        if let Some(known) = self.namespaces.get(uri) {
            return Rc::clone(known);
        }
        let new: Rc<str> = Rc::from(uri);
        self.namespaces.insert(Rc::clone(&new));
        new
    }

    fn end_tag(&mut self) -> Result<(), XmlError> {
        let (written, tail) = name(self.rest)?;
        let tail = tail.trim_start_matches(is_space);
        self.rest = tail
            .strip_prefix('>')
            .ok_or(XmlError::Malformed("an end tag not ended"))?;
        match self.open.last() {
            Some(open) if open.written == written => {
                self.close();
                Ok(())
            }
            _ => Err(XmlError::Malformed("an end tag not of the element open")),
        }
    }

    /// The element open innermost, ended: into its parent, or the root; its
    /// declarations out of scope.
    fn close(&mut self) {
        for prefix in self.scopes.pop().unwrap_or_default() {
            if let Some(namespaces) = self.bindings.get_mut(prefix) {
                namespaces.pop();
                if namespaces.is_empty() {
                    self.bindings.remove(prefix);
                }
            }
        }
        let Some(open) = self.open.pop() else {
            return;
        };
        match self.open.last_mut() {
            Some(parent) => parent.element.children.push(Node::Element(open.element)),
            None => self.root = Some(open.element),
        }
    }

    /// A name as written, resolved: an element's unprefixed name in the
    /// default namespace, an attribute's in none.
    fn resolve(&self, written: &'a str, element: bool) -> Result<Name<'a>, XmlError> {
        let (prefix, local) = match written.split_once(':') {
            Some((p, l)) if !p.is_empty() && !l.is_empty() && !l.contains(':') => (p, l),
            Some(_) => return Err(XmlError::Malformed("a name of more than one prefix")),
            None => ("", written),
        };
        let namespace = if prefix == "xml" {
            &self.xml
        } else if prefix.is_empty() && !element {
            &self.none
        } else {
            match self
                .bindings
                .get(prefix)
                .and_then(|namespaces| namespaces.last())
            {
                Some(namespace) => namespace,
                None if prefix.is_empty() => &self.none,
                None => return Err(XmlError::UnboundPrefix),
            }
        };
        Ok(Name {
            namespace: Rc::clone(namespace),
            local,
        })
    }
}

/// What an attribute named `written` declares: `Some("")` the default
/// namespace, `Some(prefix)` a prefix, `None` nothing.
fn declares(written: &str) -> Result<Option<&str>, XmlError> {
    if written == "xmlns" {
        return Ok(Some(""));
    }
    match written.strip_prefix("xmlns:") {
        Some(prefix) if prefix.is_empty() || prefix.contains(':') => {
            Err(XmlError::Malformed("a declaration's prefix no name"))
        }
        declared => Ok(declared),
    }
}

/// XML's white space.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

/// Whether XML allows `c` (XML 1.0's Char): a `char` is never a surrogate,
/// so what is left out is the control characters but tab, line feed and
/// carriage return, and U+FFFE and U+FFFF.
fn is_xml_char(c: char) -> bool {
    !matches!(
        c,
        '\0'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}'
    )
}

/// A name at the start of `s`, and what follows it.
fn name(s: &str) -> Result<(&str, &str), XmlError> {
    let end = s
        .find(|c: char| is_space(c) || matches!(c, '/' | '>' | '=' | '<' | '"' | '\'' | '&'))
        .unwrap_or(s.len());
    let (n, rest) = s.split_at(end);
    let first = n
        .chars()
        .next()
        .ok_or(XmlError::Malformed("a name missing"))?;
    if first.is_ascii_digit() || matches!(first, '-' | '.') {
        return Err(XmlError::Malformed("a name beginning as no name may"));
    }
    Ok((n, rest))
}

/// A run of the document as XML reads it: each line ending a line feed
/// (XML 1.0 section 2.11); in an attribute's value, each white-space
/// character then a space (section 3.3.3); and, but in a CDATA section,
/// XML's five entities and the character references read. Borrowed from
/// the document where that changes nothing -- most runs of most documents.
fn as_read(s: &str, run: Run) -> Result<Cow<'_, str>, XmlError> {
    let changed = move |c: char| match c {
        '\r' => true,
        '&' => run != Run::Cdata,
        '\n' | '\t' => run == Run::Value,
        _ => false,
    };
    if !s.contains(changed) {
        return Ok(Cow::Borrowed(s));
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find(changed) {
        let (plain, tail) = rest.split_at(at);
        out.push_str(plain);
        if let Some(after) = tail.strip_prefix('&') {
            let end = after.find(';').ok_or(XmlError::Reference)?;
            let (entity, after) = after.split_at(end);
            out.push(referenced(entity)?);
            rest = after.get(1..).unwrap_or("");
        } else if let Some(after) = tail.strip_prefix('\r') {
            // A carriage return and the line feed after it are one line
            // ending.
            out.push(if run == Run::Value { ' ' } else { '\n' });
            rest = after.strip_prefix('\n').unwrap_or(after);
        } else {
            // A line feed or a tab in an attribute's value: a byte.
            out.push(' ');
            rest = tail.get(1..).unwrap_or("");
        }
    }
    out.push_str(rest);
    Ok(Cow::Owned(out))
}

/// The character a reference to `entity` -- what is between its `&` and its
/// `;` -- stands for: one of XML's five entities, or a character reference,
/// in decimal or hexadecimal, to a character XML allows.
fn referenced(entity: &str) -> Result<char, XmlError> {
    Ok(match entity {
        "lt" => '<',
        "gt" => '>',
        "amp" => '&',
        "quot" => '"',
        "apos" => '\'',
        _ => {
            let code = if let Some(hex) = entity
                .strip_prefix("#x")
                .filter(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit()))
            {
                u32::from_str_radix(hex, 16).ok()
            } else if let Some(dec) = entity
                .strip_prefix('#')
                .filter(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
            {
                dec.parse::<u32>().ok()
            } else {
                None
            };
            code.and_then(char::from_u32)
                .filter(|&c| is_xml_char(c))
                .ok_or(XmlError::Reference)?
        }
    })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::format_collect,
        reason = "a test: a failure should be loud"
    )]

    use super::*;

    const TT: &str = "http://www.w3.org/ns/ttml";
    const TTS: &str = "http://www.w3.org/ns/ttml#styling";

    fn element<'n, 'a>(node: &'n Node<'a>) -> &'n Element<'a> {
        match node {
            Node::Element(e) => e,
            Node::Text(t) => panic!("text {t:?}, not an element"),
        }
    }

    #[test]
    fn a_document_is_its_root_names_resolved() {
        let doc = br#"<?xml version="1.0" encoding="UTF-8"?>
<!-- a comment before -->
<tt xmlns="http://www.w3.org/ns/ttml" xmlns:s="http://www.w3.org/ns/ttml#styling" xml:lang="en">
  <body><p begin="1s" s:color="red">one<br/>two</p></body>
</tt>
"#;
        let tt = parse(doc).unwrap();
        assert!(tt.name.is(TT, "tt"));
        assert_eq!(tt.attribute(XML_NAMESPACE, "lang"), Some("en"));
        let body = tt.elements().next().unwrap();
        assert!(body.name.is(TT, "body"));
        let p = body.elements().next().unwrap();
        assert!(p.name.is(TT, "p"));
        // An unprefixed attribute is in no namespace; a prefixed one in its
        // prefix's, whatever the prefix is.
        assert_eq!(p.attribute("", "begin"), Some("1s"));
        assert_eq!(p.attribute(TTS, "color"), Some("red"));
        assert_eq!(p.children.len(), 3);
        assert_eq!(p.children[0], Node::Text("one".into()));
        assert!(element(&p.children[1]).name.is(TT, "br"));
        assert_eq!(p.children[2], Node::Text("two".into()));
    }

    #[test]
    fn references_cdata_and_line_endings_are_read() {
        let tt = parse(
            b"<t a='x&#x41;\ty'>&lt;&amp;&gt;&quot;&apos;&#233;<![CDATA[<raw>&amp;]]>\r\nend\r</t>",
        )
        .unwrap();
        assert_eq!(tt.attribute("", "a"), Some("xA y"));
        assert_eq!(
            tt.children,
            [Node::Text("<&>\"'\u{e9}<raw>&amp;\nend\n".into())]
        );
    }

    /// Line endings and white space as expat reads them (answers taken from
    /// Python's `xml.etree`): in text a line ending is a line feed, in a
    /// value a space, as a tab and a line feed are; a reference to one is
    /// the character itself.
    #[test]
    fn line_endings_and_white_space_are_read_as_expat_reads_them() {
        let tt = parse(b"<a x='1&#13;&#10;2\r\n3\r4\t5\n6'>1\r\n2\r3&#13;4<![CDATA[\r\n5\r]]></a>")
            .unwrap();
        assert_eq!(tt.attribute("", "x"), Some("1\r\n2 3 4 5 6"));
        assert_eq!(tt.children, [Node::Text("1\n2\n3\r4\n5\n".into())]);
    }

    #[test]
    fn a_default_namespace_is_scoped_to_its_element() {
        let doc = parse(b"<a xmlns='urn:a'><b xmlns='urn:b'><c/></b><d/></a>").unwrap();
        let mut names = Vec::new();
        for e in doc.elements() {
            names.push((&*e.name.namespace, e.name.local));
            for inner in e.elements() {
                names.push((&*inner.name.namespace, inner.name.local));
            }
        }
        assert_eq!(names, [("urn:b", "b"), ("urn:b", "c"), ("urn:a", "d")]);
    }

    #[test]
    fn a_doctype_is_passed_over_and_its_entities_never_read() {
        let laughs = br#"<!DOCTYPE lolz [<!ENTITY lol "lol"><!ENTITY lol2 "&lol;&lol;&lol;">]><lolz>&lol2;</lolz>"#;
        assert_eq!(parse(laughs), Err(XmlError::Reference));
        let fine = parse(br#"<!DOCTYPE tt SYSTEM "tt.dtd"><tt/>"#).unwrap();
        assert_eq!(fine.name.local, "tt");
    }

    /// Each refused as expat refuses it (Python's `xml.etree` read each).
    #[test]
    fn what_is_not_well_formed_is_refused() {
        for (doc, why) in [
            (&b""[..], XmlError::Malformed("no root element")),
            (b"<a>", XmlError::Malformed("an element not ended")),
            (
                b"<a></b>",
                XmlError::Malformed("an end tag not of the element open"),
            ),
            (b"<a/><b/>", XmlError::Malformed("a second root element")),
            (b"text<a/>", XmlError::Malformed("text outside the root")),
            (
                b"<a x='1' x='2'/>",
                XmlError::Malformed("an attribute given twice"),
            ),
            // A namespace declaration twice: an attribute twice, though no
            // name of the element's resolves through it.
            (
                b"<a xmlns:p='urn:x' xmlns:p='urn:y'/>",
                XmlError::Malformed("an attribute given twice"),
            ),
            (
                b"<a x=1/>",
                XmlError::Malformed("an attribute's value not quoted"),
            ),
            (
                b"<a x='1'y='2'/>",
                XmlError::Malformed("attributes not apart"),
            ),
            (
                b"<a x='<'/>",
                XmlError::Malformed("a '<' in an attribute's value"),
            ),
            (b"<p:a/>", XmlError::UnboundPrefix),
            (b"<a>&bogus;</a>", XmlError::Reference),
            (b"<a>&#0;</a>", XmlError::Reference),
            (b"<a>&#xD800;</a>", XmlError::Reference),
            (b"<a>& no end</a>", XmlError::Reference),
            // A reference to a character XML does not allow.
            (b"<a>&#1;</a>", XmlError::Reference),
            (b"<a>&#xFFFE;</a>", XmlError::Reference),
            (b"<a>&#x110000;</a>", XmlError::Reference),
            (b"<a>\xff</a>", XmlError::NotUtf8),
            // A character XML does not allow, written: anywhere.
            (
                b"<a>x\x01y</a>",
                XmlError::Malformed("a character XML does not allow"),
            ),
            (
                b"<!-- \x01 --><a/>",
                XmlError::Malformed("a character XML does not allow"),
            ),
            (
                "<a>\u{ffff}</a>".as_bytes(),
                XmlError::Malformed("a character XML does not allow"),
            ),
            (
                b"<a><!-- never ended</a>",
                XmlError::Malformed("a comment not ended"),
            ),
            (
                b"<a xmlns:p=''/>",
                XmlError::Malformed("a prefix bound to no namespace"),
            ),
            // Two attributes whose names resolve alike.
            (
                b"<a xmlns:p='urn:x' xmlns:q='urn:x' p:k='1' q:k='2'/>",
                XmlError::Malformed("an attribute given twice"),
            ),
            // Each twice with another between, written or resolved.
            (
                b"<a xmlns:p='urn:x' y='1' xmlns:p='urn:y'/>",
                XmlError::Malformed("an attribute given twice"),
            ),
            (
                b"<a xmlns:p='urn:x' xmlns:q='urn:x' p:k='1' y='2' q:k='3'/>",
                XmlError::Malformed("an attribute given twice"),
            ),
            // The reserved prefixes and namespaces.
            (
                b"<a xmlns:xml='urn:x'/>",
                XmlError::Malformed("the xml prefix bound to another namespace"),
            ),
            (
                b"<a xmlns:p='http://www.w3.org/XML/1998/namespace'/>",
                XmlError::Malformed("a prefix bound to a reserved namespace"),
            ),
            (
                b"<a xmlns='http://www.w3.org/2000/xmlns/'/>",
                XmlError::Malformed("a prefix bound to a reserved namespace"),
            ),
            (
                b"<a xmlns:xmlns='urn:x'/>",
                XmlError::Malformed("the xmlns prefix declared"),
            ),
            (
                b"<a xmlns:='urn:x'/>",
                XmlError::Malformed("a declaration's prefix no name"),
            ),
            (
                b"<a xmlns:p:q='urn:x'/>",
                XmlError::Malformed("a declaration's prefix no name"),
            ),
        ] {
            assert_eq!(parse(doc), Err(why), "{doc:?}");
        }
    }

    /// What expat reads, read: the `xml` prefix declared as its own, the
    /// default namespace undeclared, and references to the characters at
    /// XML's edges.
    #[test]
    fn what_is_well_formed_at_the_edges_is_read() {
        let a = parse(
            b"<a xmlns:xml='http://www.w3.org/XML/1998/namespace' xml:lang='en' xmlns=''>&#x9;&#xA;&#xD;&#xFFFD;&#x10FFFF;</a>",
        )
        .unwrap();
        assert_eq!(a.attribute(XML_NAMESPACE, "lang"), Some("en"));
        assert!(a.name.is("", "a"));
        assert_eq!(a.children, [Node::Text("\t\n\r\u{fffd}\u{10ffff}".into())]);
    }

    /// Which characters XML allows, as expat answers: Python's `xml.etree`
    /// was given each of these written and referred to, and refused the
    /// same ones either way -- `refused`, its answers written as ranges.
    #[test]
    fn the_characters_xml_allows_are_expats() {
        let asked = (0u32..=0x20).chain([
            0x7F, 0x80, 0x9F, 0xD7FF, 0xE000, 0xFFFD, 0xFFFE, 0xFFFF, 0x1_0000, 0x10_FFFF,
        ]);
        let refused = [0x0..=0x8, 0xB..=0xC, 0xE..=0x1F, 0xFFFE..=0xFFFF];
        for c in asked {
            let read = !refused.iter().any(|r| r.contains(&c));
            let referred = format!("<a>&#x{c:X};</a>");
            assert_eq!(parse(referred.as_bytes()).is_ok(), read, "{referred}");
            let written = format!("<a>{}</a>", char::from_u32(c).unwrap());
            assert_eq!(parse(written.as_bytes()).is_ok(), read, "U+{c:04X} written");
        }
    }

    #[test]
    fn elements_nest_only_so_deep() {
        let deep = |n: usize| format!("{}{}", "<a>".repeat(n), "</a>".repeat(n));
        assert!(parse(deep(MAX_DEPTH).as_bytes()).is_ok());
        assert_eq!(
            parse(deep(MAX_DEPTH + 1).as_bytes()),
            Err(XmlError::TooDeep)
        );
        // Far deeper: refused at the bound, not overflowed.
        assert_eq!(parse(deep(100_000).as_bytes()), Err(XmlError::TooDeep));
    }

    #[test]
    fn a_byte_order_mark_and_a_processing_instruction_inside_are_passed_over() {
        let tt = parse("\u{feff}<a><?pi data?>x</a>".as_bytes()).unwrap();
        assert_eq!(tt.children, [Node::Text("x".into())]);
    }

    #[test]
    fn a_declaration_is_in_scope_until_its_element_ends() {
        let tt =
            parse(br#"<a xmlns:p="urn:outer"><b xmlns:p="urn:inner" p:x="1"/><c p:y="2"/></a>"#)
                .unwrap();
        let children: Vec<&Element<'_>> = tt.elements().collect();
        assert_eq!(&*children[0].attributes[0].0.namespace, "urn:inner");
        assert_eq!(&*children[1].attributes[0].0.namespace, "urn:outer");
        // Out of scope after the element declaring it.
        assert_eq!(
            parse(br#"<a><b xmlns:p="urn:x"/><c p:y="2"/></a>"#),
            Err(XmlError::UnboundPrefix)
        );
    }

    /// A namespace is one string, however many names are in it and however
    /// many declarations bind it -- a reference in one read first: each name
    /// its own copy, a namespace a megabyte long in a document of a hundred
    /// thousand elements would be a hundred gigabytes.
    #[test]
    fn a_namespace_is_one_string_however_many_names_are_in_it() {
        let tail = "x".repeat(1000);
        let body = "<p q:a='1'/><r:p/>".repeat(1000);
        let doc = format!(
            "<d xmlns='urn:{tail}' xmlns:q='urn:{tail}' xmlns:r='urn:&#x78;{}'>{body}</d>",
            &tail[1..]
        );
        let root = parse(doc.as_bytes()).unwrap();
        let mut names = vec![&root.name];
        for e in root.elements() {
            names.push(&e.name);
            names.extend(e.attributes.iter().map(|(n, _)| n));
        }
        assert_eq!(names.len(), 3001);
        assert_eq!(*root.name.namespace, *format!("urn:{tail}"));
        for n in names {
            assert!(Rc::ptr_eq(&n.namespace, &root.name.namespace));
        }
    }

    /// What is as written is not copied: a value or a run of text is a
    /// slice of the document but where a reference, a line ending or (in a
    /// value) white space makes it other.
    #[test]
    fn what_is_as_written_is_borrowed() {
        let root = parse(
            b"<a x='plain' y='a&amp;b' z='a\tb'>text<b/>a&lt;b<c/>a\r\nb<d/><![CDATA[c&amp;]]></a>",
        )
        .unwrap();
        let values: Vec<bool> = root
            .attributes
            .iter()
            .map(|(_, v)| matches!(v, Cow::Borrowed(_)))
            .collect();
        assert_eq!(values, [true, false, false]);
        let texts: Vec<bool> = root
            .children
            .iter()
            .filter_map(|n| match n {
                Node::Text(t) => Some(matches!(t, Cow::Borrowed(_))),
                Node::Element(_) => None,
            })
            .collect();
        assert_eq!(texts, [true, false, false, true]);
    }

    /// An element of a hundred thousand attributes, each in a namespace of
    /// its own declared on it, is read in its length: the attributes told
    /// apart by sorting, and each prefix resolved by a hash.
    #[test]
    fn many_attributes_and_declarations_cost_their_length() {
        let n = 100_000;
        let declarations: String = (0..n)
            .map(|i| format!(r#" xmlns:p{i}="urn:{i}""#))
            .collect();
        let attributes: String = (0..n).map(|i| format!(r#" p{i}:a="{i}""#)).collect();
        let doc = format!("<r{declarations}><e{attributes}/></r>");
        let started = std::time::Instant::now();
        let root = parse(doc.as_bytes()).unwrap();
        let e = root.elements().next().unwrap();
        assert_eq!(e.attributes.len(), n);
        assert_eq!(e.attribute("urn:99999", "a"), Some("99999"));
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        // And the same attribute twice, the last of them, is still seen.
        let twice = format!("<r{declarations}><e{attributes} p0:a='x'/></r>");
        assert_eq!(
            parse(twice.as_bytes()),
            Err(XmlError::Malformed("an attribute given twice"))
        );
    }
}
