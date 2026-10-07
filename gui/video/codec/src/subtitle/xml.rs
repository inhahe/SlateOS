//! What TTML needs of XML: XML 1.0 documents in UTF-8, read into a tree
//! with every element's and attribute's name resolved against its
//! namespace (Namespaces in XML 1.0).
//!
//! A subtitle sample is a stranger's document, so this reader is strict
//! where XML is -- a document that is not well-formed is refused whole --
//! and bounded where XML is not: elements nest at most [`MAX_DEPTH`] deep,
//! and no entity expands to more than a character. A DOCTYPE's declarations
//! are passed over unread, and an entity other than XML's five and the
//! character references is an error, so no document can make the reader
//! expand one entity into many (the "billion laughs").
//!
//! What it does not do, as TTML does not need it: other encodings than
//! UTF-8, validation, default attributes from a DTD.
//!
//! What a document costs to read is its length: an element's attributes are
//! told apart by a hash, not each against every other, and a prefix is
//! resolved by a hash of the bindings in scope, not a walk of every
//! declaration -- either of which a document of a hundred thousand
//! attributes or declarations would make quadratic.

use std::collections::{HashMap, HashSet};

/// The namespace the `xml` prefix is bound to, always.
pub(crate) const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// How deep elements may nest. TTML's documents are a handful deep -- `tt`,
/// `body`, `div`, `p`, `span` -- and a document nesting more is made to
/// exhaust whoever walks it.
pub(crate) const MAX_DEPTH: usize = 64;

/// A name as XML resolves it: its namespace (empty for none) and its local
/// part.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Name {
    pub namespace: String,
    pub local: String,
}

impl Name {
    /// Whether this is `local` in `namespace`.
    pub(crate) fn is(&self, namespace: &str, local: &str) -> bool {
        self.namespace == namespace && self.local == local
    }
}

/// An element: its name, its attributes in the order written (the
/// namespace declarations not among them), and what it holds.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Element {
    pub name: Name,
    pub attributes: Vec<(Name, String)>,
    pub children: Vec<Node>,
}

impl Element {
    /// The value of the attribute named `local` in `namespace`.
    pub(crate) fn attribute(&self, namespace: &str, local: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(name, _)| name.is(namespace, local))
            .map(|(_, value)| value.as_str())
    }

    /// The elements it holds, in order.
    pub(crate) fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }
}

/// What an element holds: elements, and text -- character data, CDATA
/// sections and references, run together between elements.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Node {
    Element(Element),
    Text(String),
}

/// Why a document is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum XmlError {
    /// The bytes are not UTF-8.
    NotUtf8,
    /// Not well-formed: what was wrong.
    Malformed(&'static str),
    /// An entity other than XML's five, or a character reference to no
    /// character.
    Reference,
    /// A prefix no namespace declaration in scope binds.
    UnboundPrefix,
    /// Elements nested past [`MAX_DEPTH`].
    TooDeep,
}

/// `input` as an XML document: its root element.
///
/// # Errors
///
/// [`XmlError`] for a document not in UTF-8, not well-formed, using a
/// prefix it does not bind, or nested too deep.
pub(crate) fn parse(input: &[u8]) -> Result<Element, XmlError> {
    let text = core::str::from_utf8(input).map_err(|_| XmlError::NotUtf8)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // XML reads every line ending as a line feed.
    let normalized;
    let text = if text.contains('\r') {
        normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        normalized.as_str()
    } else {
        text
    };
    Parser {
        rest: text,
        open: Vec::new(),
        scopes: Vec::new(),
        bindings: HashMap::new(),
        root: None,
    }
    .document()
}

/// An element begun and not yet ended: its name as written, for its end
/// tag, and the element so far.
struct Open {
    written: String,
    element: Element,
}

struct Parser<'a> {
    rest: &'a str,
    /// The elements begun and not yet ended, outermost first.
    open: Vec<Open>,
    /// The prefixes each open element declares, outermost first: the empty
    /// prefix the default namespace.
    scopes: Vec<Vec<String>>,
    /// Each prefix's namespaces in scope, the innermost declaration's last.
    bindings: HashMap<String, Vec<String>>,
    root: Option<Element>,
}

impl Parser<'_> {
    fn document(mut self) -> Result<Element, XmlError> {
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
                self.text(data.to_owned());
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
                    let value = references(chars)?;
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
    fn text(&mut self, value: String) {
        if value.is_empty() {
            return;
        }
        let Some(open) = self.open.last_mut() else {
            return;
        };
        if let Some(Node::Text(before)) = open.element.children.last_mut() {
            before.push_str(&value);
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
        let mut raw: Vec<(&str, String)> = Vec::new();
        let mut written_names: HashSet<&str> = HashSet::new();
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
            if !written_names.insert(attribute) {
                return Err(XmlError::Malformed("an attribute given twice"));
            }
            // Attribute-value normalization: each white-space character a
            // space, before references are read.
            let value = references(&value.replace(['\t', '\n'], " "))?;
            raw.push((attribute, value));
            tail = t.get(1..).unwrap_or("");
        };
        self.rest = tail;
        // The declarations first: they are in scope on the element itself.
        let mut declared = Vec::new();
        let mut attributes = Vec::new();
        for (attribute, value) in raw {
            let prefix = if attribute == "xmlns" {
                ""
            } else if let Some(prefix) = attribute.strip_prefix("xmlns:") {
                if value.is_empty() {
                    return Err(XmlError::Malformed("a prefix bound to no namespace"));
                }
                prefix
            } else {
                attributes.push((attribute, value));
                continue;
            };
            self.bindings
                .entry(prefix.to_owned())
                .or_default()
                .push(value);
            declared.push(prefix.to_owned());
        }
        self.scopes.push(declared);
        let element_name = self.resolve(written, true)?;
        let mut resolved: Vec<(Name, String)> = Vec::with_capacity(attributes.len());
        let mut names: HashSet<Name> = HashSet::with_capacity(attributes.len());
        for (attribute, value) in attributes {
            let n = self.resolve(attribute, false)?;
            if !names.insert(n.clone()) {
                return Err(XmlError::Malformed("an attribute given twice"));
            }
            resolved.push((n, value));
        }
        self.open.push(Open {
            written: written.to_owned(),
            element: Element {
                name: element_name,
                attributes: resolved,
                children: Vec::new(),
            },
        });
        if empty {
            self.close();
        }
        Ok(())
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
            if let Some(namespaces) = self.bindings.get_mut(&prefix) {
                namespaces.pop();
                if namespaces.is_empty() {
                    self.bindings.remove(&prefix);
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
    fn resolve(&self, written: &str, element: bool) -> Result<Name, XmlError> {
        let (prefix, local) = match written.split_once(':') {
            Some((p, l)) if !p.is_empty() && !l.is_empty() && !l.contains(':') => (p, l),
            Some(_) => return Err(XmlError::Malformed("a name of more than one prefix")),
            None => ("", written),
        };
        let namespace = if prefix == "xml" {
            XML_NAMESPACE.to_owned()
        } else if prefix.is_empty() && !element {
            String::new()
        } else {
            match self
                .bindings
                .get(prefix)
                .and_then(|namespaces| namespaces.last())
            {
                Some(ns) => ns.clone(),
                None if prefix.is_empty() => String::new(),
                None => return Err(XmlError::UnboundPrefix),
            }
        };
        Ok(Name {
            namespace,
            local: local.to_owned(),
        })
    }
}

/// XML's white space.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
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

/// Character data with its references read: XML's five entities, and
/// character references in decimal and hexadecimal.
fn references(s: &str) -> Result<String, XmlError> {
    if !s.contains('&') {
        return Ok(s.to_owned());
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        let (before, tail) = rest.split_at(at);
        out.push_str(before);
        let end = tail.find(';').ok_or(XmlError::Reference)?;
        let entity = tail.get(1..end).ok_or(XmlError::Reference)?;
        let c = match entity {
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
                    .filter(|&c| c != '\0')
                    .ok_or(XmlError::Reference)?
            }
        };
        out.push(c);
        rest = tail.get(end.saturating_add(1)..).unwrap_or("");
    }
    out.push_str(rest);
    Ok(out)
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

    fn element(node: &Node) -> &Element {
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

    #[test]
    fn a_default_namespace_is_scoped_to_its_element() {
        let doc = parse(b"<a xmlns='urn:a'><b xmlns='urn:b'><c/></b><d/></a>").unwrap();
        let mut names = Vec::new();
        for e in doc.elements() {
            names.push((e.name.namespace.clone(), e.name.local.clone()));
            for inner in e.elements() {
                names.push((inner.name.namespace.clone(), inner.name.local.clone()));
            }
        }
        assert_eq!(
            names,
            [
                ("urn:b".to_owned(), "b".to_owned()),
                ("urn:b".to_owned(), "c".to_owned()),
                ("urn:a".to_owned(), "d".to_owned()),
            ]
        );
    }

    #[test]
    fn a_doctype_is_passed_over_and_its_entities_never_read() {
        let laughs = br#"<!DOCTYPE lolz [<!ENTITY lol "lol"><!ENTITY lol2 "&lol;&lol;&lol;">]><lolz>&lol2;</lolz>"#;
        assert_eq!(parse(laughs), Err(XmlError::Reference));
        let fine = parse(br#"<!DOCTYPE tt SYSTEM "tt.dtd"><tt/>"#).unwrap();
        assert_eq!(fine.name.local, "tt");
    }

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
            (b"<a>\xff</a>", XmlError::NotUtf8),
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
        ] {
            assert_eq!(parse(doc), Err(why), "{doc:?}");
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
        let children: Vec<&Element> = tt.elements().collect();
        assert_eq!(children[0].attributes[0].0.namespace, "urn:inner");
        assert_eq!(children[1].attributes[0].0.namespace, "urn:outer");
        // Out of scope after the element declaring it.
        assert_eq!(
            parse(br#"<a><b xmlns:p="urn:x"/><c p:y="2"/></a>"#),
            Err(XmlError::UnboundPrefix)
        );
    }

    /// An element of a hundred thousand attributes, each in a namespace of
    /// its own declared on it, is read in its length: each told from the
    /// others, and each prefix resolved, by a hash.
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
