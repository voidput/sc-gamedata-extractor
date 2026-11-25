//! A minimal reader for the game's XML config files (port of `sc-gamedata/src/xml.ts`).
//!
//! The TypeScript had StarBreaker convert CryXmlB to text, then read the text with this
//! reader. Here [`read_document`] decodes CryXmlB directly ([`crate::cryxml`]) and keeps
//! the text reader for files that ship as plain XML (and for tests).
//!
//! Converted CryXml is plain element/attribute XML: no text content, no namespaces, no
//! CDATA. That subset is all this handles. Comments, processing instructions and doctypes
//! are skipped; text between elements is ignored; the five predefined entities and numeric
//! character references are decoded. Child order is preserved, which matters: the game
//! lists action maps and actions in the order its controls UI shows them.

use crate::cryxml::{MAGIC, XmlNode, parse_cryxml};
use crate::helpers_world::is_js_space;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("XML parse error at offset {offset}: {message}")]
pub struct XmlError {
    pub offset: usize,
    pub message: String,
}

/// An archive file as a tree: CryXmlB when it carries the magic, else UTF-8 text XML.
pub fn read_document(bytes: &[u8]) -> Result<XmlNode, String> {
    if bytes.starts_with(MAGIC) {
        return parse_cryxml(bytes, 0).map_err(|e| e.to_string());
    }
    parse_xml(&String::from_utf8_lossy(bytes)).map_err(|e| e.to_string())
}

/// `[A-Za-z_:]`
fn name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b':'
}

/// `[-A-Za-z0-9_:.]`
fn name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.')
}

/// End of the name starting at `at` (the sticky `NAME` regex), or `None`.
fn name_end(text: &str, at: usize) -> Option<usize> {
    let b = text.as_bytes();
    if !b.get(at).copied().is_some_and(name_start) {
        return None;
    }
    let mut i = at + 1;
    while b.get(i).copied().is_some_and(name_char) {
        i += 1;
    }
    Some(i)
}

/// Past any `\s` at `at`.
fn skip_space(text: &str, at: usize) -> usize {
    let rest = &text[at..];
    at + rest.len() - rest.trim_start_matches(is_js_space).len()
}

/// The sticky `ATTRIBUTE` regex at `at`: `(name, raw value, end)`.
fn attribute_at(text: &str, at: usize) -> Option<(&str, &str, usize)> {
    let start = skip_space(text, at);
    let end = name_end(text, start)?;
    let name = &text[start..end];
    let eq = skip_space(text, end);
    if text.as_bytes().get(eq) != Some(&b'=') {
        return None;
    }
    let open = skip_space(text, eq + 1);
    let quote = *text.as_bytes().get(open)?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let close = open + 1 + text[open + 1..].find(quote as char)?;
    Some((name, &text[open + 1..close], close + 1))
}

/// `&(#x[0-9a-f]+|#\d+|[a-z]+);` (case-insensitive), as `decodeEntities`.
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        match decode_one(after) {
            Some((decoded, used)) => {
                out.push_str(&decoded);
                rest = &after[used..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// One reference after its `&`: the replacement and the bytes consumed (`;` included).
fn decode_one(after: &str) -> Option<(String, usize)> {
    let b = after.as_bytes();
    let (body_len, decoded) = if b.first() == Some(&b'#') {
        let hex = matches!(b.get(1), Some(b'x' | b'X'));
        let digits_at = if hex { 2 } else { 1 };
        let n = b[digits_at..]
            .iter()
            .take_while(|c| {
                if hex {
                    c.is_ascii_hexdigit()
                } else {
                    c.is_ascii_digit()
                }
            })
            .count();
        if n == 0 {
            return None;
        }
        let digits = &after[digits_at..digits_at + n];
        let code = u32::from_str_radix(digits, if hex { 16 } else { 10 }).ok();
        // `String.fromCodePoint` of an invalid code point throws; keep the text instead.
        let ch = code.and_then(char::from_u32).map(String::from);
        (digits_at + n, ch)
    } else {
        let n = b.iter().take_while(|c| c.is_ascii_alphabetic()).count();
        if n == 0 {
            return None;
        }
        let ch = match &after[..n] {
            "amp" => Some("&"),
            "lt" => Some("<"),
            "gt" => Some(">"),
            "quot" => Some("\""),
            "apos" => Some("'"),
            _ => None,
        };
        (n, ch.map(String::from))
    };
    if b.get(body_len) != Some(&b';') {
        return None;
    }
    let used = body_len + 1;
    Some((
        decoded.unwrap_or_else(|| format!("&{}", &after[..used])),
        used,
    ))
}

/// `parseXml`: the root element of a document. Malformed input is an error.
pub fn parse_xml(text: &str) -> Result<XmlNode, XmlError> {
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root: Option<XmlNode> = None;
    let mut i = 0;
    let fail = |offset: usize, message: String| XmlError { offset, message };

    while i < text.len() {
        let Some(open) = text[i..].find('<').map(|o| o + i) else {
            break;
        };
        i = open;
        let rest = &text[i..];

        if rest.starts_with("<!--") {
            let end = text[i + 4..]
                .find("-->")
                .ok_or_else(|| fail(i, "unterminated comment".into()))?;
            i = i + 4 + end + 3;
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            let end = rest
                .find('>')
                .ok_or_else(|| fail(i, "unterminated declaration".into()))?;
            i += end + 1;
            continue;
        }

        if rest.as_bytes().get(1) == Some(&b'/') {
            let end = name_end(text, i + 2).ok_or_else(|| fail(i, "bad closing tag".into()))?;
            let name = &text[i + 2..end];
            match stack.pop() {
                Some(current) if current.tag == name => {
                    let close = text[end..]
                        .find('>')
                        .ok_or_else(|| fail(i, "unterminated closing tag".into()))?;
                    i = end + close + 1;
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(current),
                        None => root = Some(current),
                    }
                }
                _ => return Err(fail(i, format!("unexpected </{name}>"))),
            }
            continue;
        }

        let end = name_end(text, i + 1).ok_or_else(|| fail(i, "bad tag name".into()))?;
        let mut element = XmlNode {
            tag: text[i + 1..end].to_string(),
            ..XmlNode::default()
        };
        i = end;
        while let Some((name, value, next)) = attribute_at(text, i) {
            let value = decode_entities(value);
            // An object assignment: a repeated key keeps its place and takes the last value.
            match element.attrs.iter_mut().find(|(k, _)| k == name) {
                Some(slot) => slot.1 = value,
                None => element.attrs.push((name.to_string(), value)),
            }
            i = next;
        }

        i = skip_space(text, i);
        let self_closing = text[i..].starts_with("/>");
        if !self_closing && text.as_bytes().get(i) != Some(&b'>') {
            return Err(fail(i, format!("malformed tag <{}>", element.tag)));
        }
        i += if self_closing { 2 } else { 1 };

        if stack.is_empty() && root.is_some() {
            return Err(fail(i, "multiple root elements".into()));
        }
        if self_closing {
            match stack.last_mut() {
                Some(parent) => parent.children.push(element),
                None => root = Some(element),
            }
        } else {
            stack.push(element);
        }
    }

    if let Some(open) = stack.last() {
        return Err(fail(i, format!("unclosed <{}>", open.tag)));
    }
    root.ok_or_else(|| fail(i, "no root element".into()))
}

/// `childrenNamed`: direct children with the tag, case-sensitive.
pub fn children_named<'a>(node: &'a XmlNode, tag: &'a str) -> impl Iterator<Item = &'a XmlNode> {
    node.children.iter().filter(move |c| c.tag == tag)
}

/// `childNamed`: the first direct child with the tag.
pub fn child_named<'a>(node: &'a XmlNode, tag: &str) -> Option<&'a XmlNode> {
    node.children.iter().find(|c| c.tag == tag)
}

#[cfg(test)]
mod tests {
    //! Cases from `src/xml.test.ts`.
    use super::*;

    #[test]
    fn reads_nested_elements_attributes_and_entities() {
        let root = parse_xml(
            "<?xml version=\"1.0\"?>\n<!-- c --><root a=\"1\" b='x &amp; y &#65;&#x42;'>\n  <kid n=\"one\"/>\n  <kid n=\"two\"><inner/></kid>\n</root>",
        )
        .unwrap();
        assert_eq!(root.tag, "root");
        assert_eq!(root.attr("a"), Some("1"));
        assert_eq!(root.attr("b"), Some("x & y AB"));
        let kids: Vec<_> = children_named(&root, "kid").collect();
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[1].attr("n"), Some("two"));
        assert!(child_named(kids[1], "inner").is_some());
        assert!(child_named(&root, "none").is_none());
    }

    #[test]
    fn parses_nested_elements_and_attributes_in_order() {
        let root = parse_xml(
            "<?xml version=\"1.0\"?>\n<!-- a comment -->\n<a x=\"1\" y='two'><b/><c z=\"&amp;&lt;&#65;&#x42;\"><d /></c></a>",
        )
        .unwrap();
        assert_eq!(root.tag, "a");
        assert_eq!(
            root.attrs,
            vec![("x".into(), "1".into()), ("y".into(), "two".into())]
        );
        let tags: Vec<&str> = root.children.iter().map(|c| c.tag.as_str()).collect();
        assert_eq!(tags, ["b", "c"]);
        assert_eq!(root.children[1].attr("z"), Some("&<AB"));
        assert_eq!(root.children[1].children[0].tag, "d");
    }

    #[test]
    fn keeps_a_lone_space_attribute_value_intact() {
        assert_eq!(parse_xml("<a k=\" \" />").unwrap().attr("k"), Some(" "));
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(
            parse_xml("<a><b></a>")
                .unwrap_err()
                .message
                .contains("unexpected")
        );
        assert!(parse_xml("<a>").unwrap_err().message.contains("unclosed"));
        assert!(parse_xml("").is_err());
        assert!(parse_xml("<a/><b/>").is_err());
    }

    #[test]
    fn unknown_entities_stay_and_repeated_attributes_take_the_last_value() {
        let root = parse_xml("<a x=\"&bogus; &AMP; &\" x=\"2\" y=\"&#xZZ;\"/>").unwrap();
        assert_eq!(
            root.attrs,
            vec![("x".into(), "2".into()), ("y".into(), "&#xZZ;".into())]
        );
        let root = parse_xml("<a x=\"&bogus; &AMP; & q\"/>").unwrap();
        assert_eq!(root.attr("x"), Some("&bogus; &AMP; & q"));
    }

    #[test]
    fn reads_cryxml_or_text() {
        let tree = parse_xml("<profile v=\"1\"><a/></profile>").unwrap();
        let bin = crate::cryxml::tests::cryxml(&tree);
        assert_eq!(read_document(&bin).unwrap(), tree);
        assert_eq!(
            read_document(b"<profile v=\"1\"><a/></profile>").unwrap(),
            tree
        );
        assert!(read_document(b"not xml").is_err());
    }
}
