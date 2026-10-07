//! Just enough XML to read what a UPnP router sends: its device description
//! and its SOAP answers.
//!
//! Both are small documents of elements and text. What is read: elements,
//! with their namespace prefix dropped (`u:AddPortMappingResponse` is
//! `AddPortMappingResponse`); text, with the five named entities and numeric
//! character references decoded; CDATA sections, kept as they are; and
//! attributes, checked for form and otherwise skipped -- nothing here needs
//! them. Comments and processing instructions (`<?xml ...?>`) are skipped.
//!
//! What is refused: a document type declaration (`<!DOCTYPE ...>`), which no
//! UPnP document has and through which entity tricks arrive; an entity other
//! than the five; a tag closed by another's name; text or a second element
//! after the root; nesting deeper than [`MAX_DEPTH`]; more than
//! [`MAX_ELEMENTS`] elements. A router's answer is not trusted: it is read
//! by rules that bound what it can cost.

/// The deepest nesting read. A UPnP description nests five or six deep.
pub const MAX_DEPTH: usize = 32;

/// The most elements read. A router's description has a few hundred.
pub const MAX_ELEMENTS: usize = 10_000;

/// An element.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Element {
    /// Its name as written, prefix and all.
    qname: String,
    /// The text directly inside it, its pieces joined.
    text: String,
    /// The elements directly inside it, in order.
    pub children: Vec<Element>,
}

impl Element {
    /// Its name without a namespace prefix.
    #[must_use]
    pub fn name(&self) -> &str {
        self.qname
            .rsplit_once(':')
            .map_or(self.qname.as_str(), |(_, local)| local)
    }

    /// The text directly inside it, trimmed.
    #[must_use]
    pub fn text(&self) -> &str {
        self.text.trim()
    }

    /// Its first child named `name`.
    #[must_use]
    pub fn child(&self, name: &str) -> Option<&Self> {
        self.children.iter().find(|c| c.name() == name)
    }

    /// The text of its first child named `name`.
    #[must_use]
    pub fn child_text(&self, name: &str) -> Option<&str> {
        self.child(name).map(Self::text)
    }

    /// Every element named `name` at any depth below it, in document order.
    #[must_use]
    pub fn find_all(&self, name: &str) -> Vec<&Self> {
        let mut found = Vec::new();
        let mut pending: Vec<&Self> = self.children.iter().rev().collect();
        while let Some(e) = pending.pop() {
            if e.name() == name {
                found.push(e);
            }
            pending.extend(e.children.iter().rev());
        }
        found
    }
}

/// Read `doc`'s root element.
///
/// # Errors
///
/// What is wrong with it, in a phrase, when it is not a document these rules
/// read.
pub fn parse(doc: &str) -> Result<Element, String> {
    let mut rest = doc.strip_prefix('\u{feff}').unwrap_or(doc);
    let mut open: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;
    let mut elements = 0usize;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("<?") {
            rest = after
                .split_once("?>")
                .ok_or("a processing instruction is not closed")?
                .1;
        } else if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.split_once("-->").ok_or("a comment is not closed")?.1;
        } else if let Some(after) = rest.strip_prefix("<![CDATA[") {
            let (data, tail) = after
                .split_once("]]>")
                .ok_or("a CDATA section is not closed")?;
            open.last_mut()
                .ok_or("a CDATA section outside the root element")?
                .text
                .push_str(data);
            rest = tail;
        } else if rest.starts_with("<!") {
            return Err("a declaration (<!DOCTYPE ...>), which no UPnP document has".to_owned());
        } else if let Some(after) = rest.strip_prefix("</") {
            let (name, tail) = after.split_once('>').ok_or("an end tag is not closed")?;
            let name = name.trim_end();
            let element = open
                .pop()
                .ok_or_else(|| format!("</{name}> closes nothing"))?;
            if element.qname != name {
                return Err(format!("<{}> is closed by </{name}>", element.qname));
            }
            close(element, &mut open, &mut root)?;
            rest = tail;
        } else if let Some(after) = rest.strip_prefix('<') {
            let (element, empty, tail) = start_tag(after)?;
            elements = elements.saturating_add(1);
            if elements > MAX_ELEMENTS {
                return Err(format!("more than {MAX_ELEMENTS} elements"));
            }
            if open.is_empty() && root.is_some() {
                return Err(format!("<{}> after the root element", element.qname));
            }
            if empty {
                close(element, &mut open, &mut root)?;
            } else {
                if open.len() >= MAX_DEPTH {
                    return Err(format!("elements nested more than {MAX_DEPTH} deep"));
                }
                open.push(element);
            }
            rest = tail;
        } else {
            let (raw, tail) = rest.split_at(rest.find('<').unwrap_or(rest.len()));
            let text = decode(raw)?;
            match open.last_mut() {
                Some(e) => e.text.push_str(&text),
                None if text.trim().is_empty() => {}
                None => return Err("text outside the root element".to_owned()),
            }
            rest = tail;
        }
    }
    if let Some(e) = open.last() {
        return Err(format!("<{}> is never closed", e.qname));
    }
    root.ok_or_else(|| "no element at all".to_owned())
}

/// Put a finished element in its parent, or make it the root.
fn close(element: Element, open: &mut [Element], root: &mut Option<Element>) -> Result<(), String> {
    match open.last_mut() {
        Some(parent) => parent.children.push(element),
        None if root.is_none() => *root = Some(element),
        None => return Err(format!("<{}> after the root element", element.qname)),
    }
    Ok(())
}

/// Read a start tag from just after its `<`: the element, whether it is
/// empty (`<a/>`), and what follows the tag.
fn start_tag(after: &str) -> Result<(Element, bool, &str), String> {
    let end = after
        .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .ok_or("a tag is not closed")?;
    let (qname, mut rest) = after.split_at(end);
    if qname.is_empty() || qname.contains(['<', '&', '"', '\'', '=']) {
        return Err("a tag with no name, or a malformed one".to_owned());
    }
    let element = Element {
        qname: qname.to_owned(),
        ..Element::default()
    };
    loop {
        rest = rest.trim_start();
        if let Some(tail) = rest.strip_prefix("/>") {
            return Ok((element, true, tail));
        }
        if let Some(tail) = rest.strip_prefix('>') {
            return Ok((element, false, tail));
        }
        // An attribute: name = "value", or 'value'.
        let (name, value) = rest
            .split_once('=')
            .ok_or_else(|| format!("<{qname}> is not closed"))?;
        let name = name.trim_end();
        if name.is_empty()
            || name.contains(['<', '>', '/', '"', '\''])
            || name.contains(char::is_whitespace)
        {
            return Err(format!("a malformed attribute in <{qname}>"));
        }
        let value = value.trim_start();
        let quote = value
            .chars()
            .next()
            .filter(|&c| c == '"' || c == '\'')
            .ok_or_else(|| format!("an unquoted attribute in <{qname}>"))?;
        let (body, tail) = value
            .get(1..)
            .and_then(|v| v.split_once(quote))
            .ok_or_else(|| format!("an attribute of <{qname}> is not closed"))?;
        if body.contains('<') {
            return Err(format!("a '<' in an attribute of <{qname}>"));
        }
        decode(body)?;
        rest = tail;
    }
}

/// `raw` with its entity and character references decoded.
fn decode(raw: &str) -> Result<String, String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some((before, after)) = rest.split_once('&') {
        out.push_str(before);
        let (name, tail) = after
            .split_once(';')
            .filter(|(name, _)| name.len() <= 10)
            .ok_or("an '&' that begins no reference")?;
        out.push(entity(name).ok_or_else(|| format!("an unknown reference &{name};"))?);
        rest = tail;
    }
    out.push_str(rest);
    Ok(out)
}

/// The character `&name;` stands for: one of the five XML names, or a
/// character number in decimal (`#38`) or hexadecimal (`#x26`).
fn entity(name: &str) -> Option<char> {
    match name {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse::<u32>().ok()?,
            };
            char::from_u32(code).filter(|&c| c != '\0')
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_description_is_read() {
        let doc = "<?xml version=\"1.0\"?>\n\
            <root xmlns=\"urn:schemas-upnp-org:device-1-0\">\n\
              <specVersion><major>1</major><minor>0</minor></specVersion>\n\
              <device>\n\
                <deviceType>urn:schemas-upnp-org:device:InternetGatewayDevice:1</deviceType>\n\
                <friendlyName>Router &amp; Modem</friendlyName>\n\
                <serviceList><service><serviceType>a</serviceType></service></serviceList>\n\
                <deviceList><device><serviceList>\n\
                  <service><serviceType>b</serviceType></service>\n\
                </serviceList></device></deviceList>\n\
              </device>\n\
            </root>\n";
        let root = parse(doc).unwrap_or_default();
        assert_eq!(root.name(), "root");
        let device = root.child("device");
        assert_eq!(
            device.and_then(|d| d.child_text("friendlyName")),
            Some("Router & Modem")
        );
        let types: Vec<&str> = root
            .find_all("service")
            .iter()
            .filter_map(|s| s.child_text("serviceType"))
            .collect();
        assert_eq!(types, ["a", "b"], "every service, in document order");
        assert_eq!(
            root.child("specVersion")
                .and_then(|v| v.child_text("major")),
            Some("1")
        );
    }

    #[test]
    fn prefixes_are_dropped_and_must_match_when_closing() {
        let doc = "<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\">\
            <s:Body><u:R xmlns:u='x'><NewExternalIPAddress>203.0.113.7</NewExternalIPAddress></u:R>\
            </s:Body></s:Envelope>";
        let root = parse(doc).unwrap_or_default();
        assert_eq!(root.name(), "Envelope");
        let r = root.child("Body").and_then(|b| b.child("R"));
        assert_eq!(
            r.and_then(|r| r.child_text("NewExternalIPAddress")),
            Some("203.0.113.7")
        );
        assert!(parse("<s:a></a>").is_err());
        assert!(parse("<a></s:a>").is_err());
    }

    #[test]
    fn references_cdata_comments_and_empty_elements() {
        let doc = "<a>x &lt;&gt;&amp;&quot;&apos; &#65;&#x42;&#X43; <!-- no --><![CDATA[<raw&>]]><b/><c x=\"1\" y='a>b'/></a>";
        let root = parse(doc).unwrap_or_default();
        assert_eq!(root.text(), "x <>&\"' ABC <raw&>");
        let names: Vec<&str> = root.children.iter().map(Element::name).collect();
        assert_eq!(names, ["b", "c"]);
    }

    #[test]
    fn text_is_trimmed_but_kept_whole_inside() {
        let root = parse("<a>\n  two  words \n</a>").unwrap_or_default();
        assert_eq!(root.text(), "two  words");
    }

    #[test]
    fn what_is_refused() {
        for doc in [
            "",
            "   ",
            "text",
            "<a>",
            "<a></b>",
            "</a>",
            "<a></a><b></b>",
            "<a></a>tail",
            "<a>&unknown;</a>",
            "<a>&#0;</a>",
            "<a>&#xD800;</a>",
            "<a>& </a>",
            "<!DOCTYPE a [<!ENTITY e \"x\">]><a>&e;</a>",
            "<a x></a>",
            "<a x=1></a>",
            "<a x=\"1></a>",
            "<a x=\"<\"></a>",
            "<a><!-- </a>",
            "<a><![CDATA[ </a>",
            "<?xml version=\"1.0\"",
            "<>",
            "<a",
        ] {
            assert!(parse(doc).is_err(), "{doc:?} was read");
        }
    }

    #[test]
    fn depth_and_size_are_bounded() {
        let deep = |n: usize| format!("{}{}", "<a>".repeat(n), "</a>".repeat(n));
        assert!(parse(&deep(MAX_DEPTH)).is_ok());
        assert!(parse(&deep(MAX_DEPTH + 1)).is_err());
        let wide = |n: usize| format!("<r>{}</r>", "<e/>".repeat(n));
        assert!(parse(&wide(MAX_ELEMENTS - 1)).is_ok());
        assert!(parse(&wide(MAX_ELEMENTS)).is_err());
    }

    #[test]
    fn a_byte_order_mark_and_the_prolog_are_skipped() {
        let root =
            parse("\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<!-- c --><a/>\r\n")
                .unwrap_or_default();
        assert_eq!(root.name(), "a");
    }
}
