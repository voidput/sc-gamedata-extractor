import { describe, expect, it } from "vitest";

import { findCryXml, nodesByTag, parseCryXml, type XmlNode } from "./cryxml.js";
import { readZones } from "./socpak.js";

/** Serialise a node tree as CryXmlB, the way the engine lays it out. */
export function cryXml(root: XmlNode): Buffer {
  const strings: Buffer[] = [];
  const offsets = new Map<string, number>();
  let size = 0;
  const intern = (value: string) => {
    let offset = offsets.get(value);
    if (offset === undefined) {
      offset = size;
      const bytes = Buffer.from(`${value}\0`, "utf-8");
      strings.push(bytes);
      size += bytes.length;
      offsets.set(value, offset);
    }
    return offset;
  };

  const flat: { node: XmlNode; parent: number }[] = [];
  const index = new Map<XmlNode, number>();
  const walk = (node: XmlNode, parent: number) => {
    index.set(node, flat.length);
    flat.push({ node, parent });
    const self = flat.length - 1;
    for (const child of node.children) walk(child, self);
  };
  walk(root, -1);

  const nodes = Buffer.alloc(flat.length * 28);
  const attrs: number[] = [];
  const children: number[] = [];
  flat.forEach(({ node, parent }, i) => {
    const entries = Object.entries(node.attrs);
    nodes.writeUInt32LE(intern(node.tag), i * 28);
    nodes.writeUInt32LE(intern(""), i * 28 + 4);
    nodes.writeUInt16LE(entries.length, i * 28 + 8);
    nodes.writeUInt16LE(node.children.length, i * 28 + 10);
    nodes.writeInt32LE(parent, i * 28 + 12);
    nodes.writeUInt32LE(attrs.length / 2, i * 28 + 16);
    nodes.writeUInt32LE(children.length, i * 28 + 20);
    for (const [key, value] of entries) attrs.push(intern(key), intern(value));
    for (const child of node.children) children.push(index.get(child) as number);
  });

  const attrTable = Buffer.alloc(attrs.length * 4);
  attrs.forEach((v, i) => attrTable.writeUInt32LE(v, i * 4));
  const childTable = Buffer.alloc(children.length * 4);
  children.forEach((v, i) => childTable.writeUInt32LE(v, i * 4));
  const stringData = Buffer.concat(strings);

  const header = Buffer.alloc(44);
  header.write("CryXmlB\0", 0, "latin1");
  const nodeOffset = 44;
  const attrOffset = nodeOffset + nodes.length;
  const childOffset = attrOffset + attrTable.length;
  const stringOffset = childOffset + childTable.length;
  [stringOffset + stringData.length, nodeOffset, flat.length, attrOffset, attrs.length / 2, childOffset, children.length, stringOffset, stringData.length].forEach(
    (v, i) => header.writeUInt32LE(v, 8 + i * 4),
  );
  return Buffer.concat([header, nodes, attrTable, childTable, stringData]);
}

const node = (tag: string, attrs: Record<string, string> = {}, children: XmlNode[] = []): XmlNode => ({ tag, attrs, children });

/** A stored-only zip, enough for a socpak in a test. */
function zip(files: { name: string; data: Buffer }[]): Buffer {
  const locals: Buffer[] = [];
  const central: Buffer[] = [];
  let offset = 0;
  for (const file of files) {
    const name = Buffer.from(file.name);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt32LE(file.data.length, 18);
    local.writeUInt32LE(file.data.length, 22);
    local.writeUInt16LE(name.length, 26);
    locals.push(local, name, file.data);
    const entry = Buffer.alloc(46);
    entry.writeUInt32LE(0x02014b50, 0);
    entry.writeUInt32LE(file.data.length, 20);
    entry.writeUInt32LE(file.data.length, 24);
    entry.writeUInt16LE(name.length, 28);
    entry.writeUInt32LE(offset, 42);
    central.push(entry, name);
    offset += 30 + name.length + file.data.length;
  }
  const directory = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(files.length, 8);
  end.writeUInt16LE(files.length, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, directory, end]);
}

describe("CryXmlB", () => {
  const tree = node("Entities", {}, [
    node("Entity", { Name: "a", EntityClass: "Light" }),
    node("Entity", { Name: "gz", EntityClass: "GreenZone", Radius: "12000" }, [
      node("PropertiesDataCore", {}, [node("EntityComponentGreenZone", { radius: "12000" })]),
    ]),
    node("Entity", { Name: "b", EntityClass: "Light" }),
  ]);

  it("round-trips tags, attributes and nesting", () => {
    const parsed = parseCryXml(cryXml(tree));
    expect(parsed).toEqual(tree);
  });

  it("is found inside a larger file and parsed from there", () => {
    const wrapped = Buffer.concat([Buffer.from("CrChF junk before the document"), cryXml(tree)]);
    const start = findCryXml(wrapped);
    expect(start).toBe(30);
    expect([...nodesByTag(parseCryXml(wrapped, start), "Entity")].map((e) => e.attrs.Name)).toEqual(["a", "gz", "b"]);
  });

  it("rejects anything else", () => {
    expect(() => parseCryXml(Buffer.from("<xml/>          "))).toThrow();
    expect(findCryXml(Buffer.from("nothing here"))).toBe(-1);
  });
});

describe("container zones", () => {
  const soc = (entities: XmlNode[]) => Buffer.concat([Buffer.from("CrChF\0\0\0"), cryXml(node("Entities", {}, entities))]);

  it("reads the largest armistice radius and a monitored zone", () => {
    const zones = readZones(
      zip([
        {
          name: "station.soc",
          data: soc([
            node("Entity", { EntityClass: "GreenZone", Radius: "7500" }),
            node("Entity", { EntityClass: "GreenZone", Radius: "1" }, [node("EntityComponentGreenZone", { radius: "12000" })]),
            node("Entity", { EntityClass: "AreaSphere", Radius: "90000" }),
          ]),
        },
        { name: "station_editor.xml", data: Buffer.from('<Layer><Object name="MonitoredZone-001" type="MonitoredZone" guid="x"/></Layer>') },
      ]),
    );
    expect(zones).toEqual({ armisticeRadius: 12000, monitored: true });
  });

  it("reports nothing for a container with neither", () => {
    const zones = readZones(
      zip([
        { name: "rock.soc", data: soc([node("Entity", { EntityClass: "Light" })]) },
        { name: "rock_editor.xml", data: Buffer.from('<Layer><Object name="x" type="Light"/></Layer>') },
        // Nested documents belong to sub-objects, not this container.
        { name: "rock/sub_editor.xml", data: Buffer.from('<Object type="MonitoredZone"/>') },
      ]),
    );
    expect(zones).toEqual({});
  });
});
