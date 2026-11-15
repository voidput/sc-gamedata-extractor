/**
 * CryXmlB: the binary XML the engine ships entity data in.
 *
 * Layout, little-endian: an 8-byte magic `CryXmlB\0`, then nine u32s (file
 * length, then offset and count for the node, attribute and child tables, then
 * offset and size of the string data). A node is 28 bytes: name offset, content
 * offset, attribute count (u16), child count (u16), parent index, first
 * attribute index, first child index, reserved. An attribute is two string
 * offsets, key then value. The child table is a flat list of node indices.
 * Strings are NUL-terminated in the string data.
 */

export interface XmlNode {
  tag: string;
  attrs: Record<string, string>;
  children: XmlNode[];
}

const MAGIC = Buffer.from("CryXmlB\0", "latin1");

/** Offset of the first CryXmlB document inside a larger file, or -1. */
export function findCryXml(buffer: Buffer): number {
  return buffer.indexOf(MAGIC);
}

/** Parse the CryXmlB document starting at `start`. Returns its root node. */
export function parseCryXml(buffer: Buffer, start = 0): XmlNode {
  if (buffer.compare(MAGIC, 0, MAGIC.length, start, start + MAGIC.length) !== 0) {
    throw new Error("not a CryXmlB document");
  }
  const u32 = (offset: number) => buffer.readUInt32LE(start + offset);
  const nodeTable = u32(12);
  const nodeCount = u32(16);
  const attrTable = u32(20);
  const childTable = u32(28);
  const strings = start + u32(36);
  const stringsEnd = strings + u32(40);

  const cache = new Map<number, string>();
  const text = (offset: number): string => {
    let value = cache.get(offset);
    if (value === undefined) {
      const from = strings + offset;
      let end = from;
      while (end < stringsEnd && buffer[end] !== 0) end++;
      value = buffer.toString("utf-8", from, end);
      cache.set(offset, value);
    }
    return value;
  };

  const nodes: XmlNode[] = new Array(nodeCount);
  const firstChild: number[] = new Array(nodeCount);
  const childCount: number[] = new Array(nodeCount);

  for (let i = 0; i < nodeCount; i++) {
    const at = nodeTable + i * 28;
    const attrCount = buffer.readUInt16LE(start + at + 8);
    const firstAttr = u32(at + 16);
    const attrs: Record<string, string> = {};
    for (let a = 0; a < attrCount; a++) {
      const entry = attrTable + (firstAttr + a) * 8;
      attrs[text(u32(entry))] = text(u32(entry + 4));
    }
    nodes[i] = { tag: text(u32(at)), attrs, children: [] };
    childCount[i] = buffer.readUInt16LE(start + at + 10);
    firstChild[i] = u32(at + 20);
  }

  for (let i = 0; i < nodeCount; i++) {
    for (let c = 0; c < childCount[i]; c++) {
      const index = u32(childTable + (firstChild[i] + c) * 4);
      if (index < nodeCount) nodes[i].children.push(nodes[index]);
    }
  }

  return nodes[0] ?? { tag: "", attrs: {}, children: [] };
}

/** Every node with the given tag, depth first. */
export function* nodesByTag(root: XmlNode, tag: string): Generator<XmlNode> {
  const stack = [root];
  while (stack.length > 0) {
    const node = stack.pop() as XmlNode;
    if (node.tag === tag) yield node;
    for (let i = node.children.length - 1; i >= 0; i--) stack.push(node.children[i]);
  }
}
