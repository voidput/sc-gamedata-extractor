//! CryXmlB, the binary XML the engine ships entity data in (port of
//! `sc-gamedata/src/cryxml.ts`).
//!
//! Little-endian: the 8-byte magic `CryXmlB\0`, then nine u32s (file length, then offset
//! and count of the node, attribute and child tables, then offset and size of the string
//! data). A node is 28 bytes: name offset, content offset, attribute count (u16), child
//! count (u16), parent index, first attribute index, first child index, reserved. An
//! attribute is two string offsets, key then value. The child table is a flat list of node
//! indices. Strings are NUL-terminated.
//!
//! Unlike the TypeScript, malformed input is an error rather than an exception or a panic.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct XmlNode {
    pub tag: String,
    /// Attributes in document order.
    pub attrs: Vec<(String, String)>,
    pub children: Vec<XmlNode>,
}

impl XmlNode {
    /// An attribute's value; with a repeated key the last one, as an object assignment.
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CryXmlError {
    #[error("not a CryXmlB document")]
    NotCryXml,
    #[error("CryXmlB document is truncated or corrupt")]
    Corrupt,
}

pub const MAGIC: &[u8] = b"CryXmlB\0";

/// Offset of the first CryXmlB document inside a larger file.
pub fn find_cryxml(buffer: &[u8]) -> Option<usize> {
    buffer.windows(MAGIC.len()).position(|w| w == MAGIC)
}

fn u32_at(b: &[u8], at: usize) -> Result<usize, CryXmlError> {
    let s = b.get(at..at + 4).ok_or(CryXmlError::Corrupt)?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
}

fn u16_at(b: &[u8], at: usize) -> Result<usize, CryXmlError> {
    let s = b.get(at..at + 2).ok_or(CryXmlError::Corrupt)?;
    Ok(u16::from_le_bytes([s[0], s[1]]) as usize)
}

/// Parse the document starting at `start`; returns its root node.
pub fn parse_cryxml(buffer: &[u8], start: usize) -> Result<XmlNode, CryXmlError> {
    if buffer.get(start..start + MAGIC.len()) != Some(MAGIC) {
        return Err(CryXmlError::NotCryXml);
    }
    let b = &buffer[start..];
    let node_table = u32_at(b, 12)?;
    let node_count = u32_at(b, 16)?;
    let attr_table = u32_at(b, 20)?;
    let child_table = u32_at(b, 28)?;
    let strings = u32_at(b, 36)?;
    let strings_end = strings
        .checked_add(u32_at(b, 40)?)
        .ok_or(CryXmlError::Corrupt)?
        .min(b.len());
    if node_count > b.len() / 28 + 1 {
        return Err(CryXmlError::Corrupt);
    }

    let mut cache: HashMap<usize, String> = HashMap::new();
    let mut text = |offset: usize| -> String {
        cache
            .entry(offset)
            .or_insert_with(|| {
                let from = strings.saturating_add(offset).min(strings_end);
                let end = b[from..strings_end]
                    .iter()
                    .position(|&c| c == 0)
                    .map_or(strings_end, |p| from + p);
                String::from_utf8_lossy(&b[from..end]).into_owned()
            })
            .clone()
    };

    let mut flat: Vec<XmlNode> = Vec::with_capacity(node_count);
    let mut first_child = Vec::with_capacity(node_count);
    let mut child_count = Vec::with_capacity(node_count);
    for i in 0..node_count {
        let at = node_table + i * 28;
        let attr_count = u16_at(b, at + 8)?;
        let first_attr = u32_at(b, at + 16)?;
        let mut attrs = Vec::with_capacity(attr_count);
        for a in 0..attr_count {
            let entry = attr_table + (first_attr + a) * 8;
            attrs.push((text(u32_at(b, entry)?), text(u32_at(b, entry + 4)?)));
        }
        flat.push(XmlNode {
            tag: text(u32_at(b, at)?),
            attrs,
            children: Vec::new(),
        });
        child_count.push(u16_at(b, at + 10)?);
        first_child.push(u32_at(b, at + 20)?);
    }

    // The TypeScript shares child objects by reference; here the tree is built bottom-up
    // from child index lists. A node listed under several parents is copied; a cycle is
    // corrupt input.
    let mut kids: Vec<Vec<usize>> = Vec::with_capacity(node_count);
    for i in 0..node_count {
        let mut list = Vec::with_capacity(child_count[i]);
        for c in 0..child_count[i] {
            let index = u32_at(b, child_table + (first_child[i] + c) * 4)?;
            if index < node_count {
                list.push(index);
            }
        }
        kids.push(list);
    }
    if node_count == 0 {
        return Ok(XmlNode::default());
    }
    build(0, &flat, &kids, &mut vec![false; node_count])
}

fn build(
    i: usize,
    flat: &[XmlNode],
    kids: &[Vec<usize>],
    on_path: &mut [bool],
) -> Result<XmlNode, CryXmlError> {
    if on_path[i] {
        return Err(CryXmlError::Corrupt);
    }
    on_path[i] = true;
    let mut node = flat[i].clone();
    for &k in &kids[i] {
        node.children.push(build(k, flat, kids, on_path)?);
    }
    on_path[i] = false;
    Ok(node)
}

/// Every node with the given tag, depth first, document order.
pub fn nodes_by_tag<'a>(root: &'a XmlNode, tag: &str) -> Vec<&'a XmlNode> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.tag == tag {
            out.push(node);
        }
        for child in node.children.iter().rev() {
            stack.push(child);
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Serialise a tree as CryXmlB, as the engine lays it out (port of the TS test helper).
    pub fn cryxml(root: &XmlNode) -> Vec<u8> {
        let mut strings: Vec<u8> = Vec::new();
        let mut offsets: HashMap<String, u32> = HashMap::new();
        let mut intern = |v: &str| -> u32 {
            if let Some(&o) = offsets.get(v) {
                return o;
            }
            let o = strings.len() as u32;
            strings.extend_from_slice(v.as_bytes());
            strings.push(0);
            offsets.insert(v.to_string(), o);
            o
        };
        let mut flat: Vec<(&XmlNode, i32)> = Vec::new();
        fn walk<'a>(n: &'a XmlNode, parent: i32, flat: &mut Vec<(&'a XmlNode, i32)>) -> usize {
            let me = flat.len();
            flat.push((n, parent));
            for c in &n.children {
                walk(c, me as i32, flat);
            }
            me
        }
        walk(root, -1, &mut flat);
        // Child indices: recompute by walking again in the same order.
        let mut child_index: Vec<Vec<u32>> = vec![Vec::new(); flat.len()];
        for (i, (_, parent)) in flat.iter().enumerate() {
            if *parent >= 0 {
                child_index[*parent as usize].push(i as u32);
            }
        }
        let mut nodes = Vec::new();
        let mut attrs: Vec<u32> = Vec::new();
        let mut children: Vec<u32> = Vec::new();
        for (i, (n, parent)) in flat.iter().enumerate() {
            nodes.extend_from_slice(&intern(&n.tag).to_le_bytes());
            nodes.extend_from_slice(&intern("").to_le_bytes());
            nodes.extend_from_slice(&(n.attrs.len() as u16).to_le_bytes());
            nodes.extend_from_slice(&(n.children.len() as u16).to_le_bytes());
            nodes.extend_from_slice(&parent.to_le_bytes());
            nodes.extend_from_slice(&((attrs.len() / 2) as u32).to_le_bytes());
            nodes.extend_from_slice(&(children.len() as u32).to_le_bytes());
            nodes.extend_from_slice(&0u32.to_le_bytes());
            for (k, v) in &n.attrs {
                attrs.push(intern(k));
                attrs.push(intern(v));
            }
            children.extend_from_slice(&child_index[i]);
        }
        let node_off = 44u32;
        let attr_off = node_off + nodes.len() as u32;
        let child_off = attr_off + attrs.len() as u32 * 4;
        let string_off = child_off + children.len() as u32 * 4;
        let mut out = MAGIC.to_vec();
        for v in [
            string_off + strings.len() as u32,
            node_off,
            flat.len() as u32,
            attr_off,
            attrs.len() as u32 / 2,
            child_off,
            children.len() as u32,
            string_off,
            strings.len() as u32,
        ] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend(nodes);
        attrs
            .iter()
            .for_each(|a| out.extend_from_slice(&a.to_le_bytes()));
        children
            .iter()
            .for_each(|c| out.extend_from_slice(&c.to_le_bytes()));
        out.extend(strings);
        out
    }

    pub fn node(tag: &str, attrs: &[(&str, &str)], children: Vec<XmlNode>) -> XmlNode {
        XmlNode {
            tag: tag.into(),
            attrs: attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            children,
        }
    }

    fn tree() -> XmlNode {
        node(
            "Entities",
            &[],
            vec![
                node("Entity", &[("Name", "a"), ("EntityClass", "Light")], vec![]),
                node(
                    "Entity",
                    &[
                        ("Name", "gz"),
                        ("EntityClass", "GreenZone"),
                        ("Radius", "12000"),
                    ],
                    vec![node(
                        "PropertiesDataCore",
                        &[],
                        vec![node(
                            "EntityComponentGreenZone",
                            &[("radius", "12000")],
                            vec![],
                        )],
                    )],
                ),
                node("Entity", &[("Name", "b"), ("EntityClass", "Light")], vec![]),
            ],
        )
    }

    // Cases from sc-gamedata src/cryxml.test.ts.
    #[test]
    fn round_trips() {
        assert_eq!(parse_cryxml(&cryxml(&tree()), 0).unwrap(), tree());
    }

    #[test]
    fn found_inside_a_larger_file() {
        let mut wrapped = b"CrChF junk before the document".to_vec();
        wrapped.extend(cryxml(&tree()));
        let start = find_cryxml(&wrapped).unwrap();
        assert_eq!(start, 30);
        let root = parse_cryxml(&wrapped, start).unwrap();
        let names: Vec<_> = nodes_by_tag(&root, "Entity")
            .iter()
            .map(|e| e.attr("Name").unwrap())
            .collect();
        assert_eq!(names, vec!["a", "gz", "b"]);
    }

    #[test]
    fn rejects_anything_else() {
        assert_eq!(
            parse_cryxml(b"<xml/>          ", 0),
            Err(CryXmlError::NotCryXml)
        );
        assert_eq!(find_cryxml(b"nothing here"), None);
        let mut cut = cryxml(&tree());
        cut.truncate(60);
        assert_eq!(parse_cryxml(&cut, 0), Err(CryXmlError::Corrupt));
    }
}
