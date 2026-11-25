import zlib from "node:zlib";

import { describe, expect, it } from "vitest";

import { parseContainerXml, readContainerXml, readSpin, readZip, rotate, systemLevelPlacements } from "./socpak.js";

/** A minimal zip: local headers, central directory, end record. */
function zip(files: { name: string; localName?: string; data: string; deflate?: boolean }[]): Buffer {
  const locals: Buffer[] = [];
  const central: Buffer[] = [];
  let offset = 0;

  for (const file of files) {
    const raw = Buffer.from(file.data);
    const body = file.deflate ? zlib.deflateRawSync(raw) : raw;
    const localName = Buffer.from(file.localName ?? file.name);
    const name = Buffer.from(file.name);

    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(file.deflate ? 8 : 0, 8);
    local.writeUInt32LE(body.length, 18);
    local.writeUInt32LE(raw.length, 22);
    local.writeUInt16LE(localName.length, 26);
    locals.push(local, localName, body);

    const entry = Buffer.alloc(46);
    entry.writeUInt32LE(0x02014b50, 0);
    entry.writeUInt16LE(file.deflate ? 8 : 0, 10);
    entry.writeUInt32LE(body.length, 20);
    entry.writeUInt32LE(raw.length, 24);
    entry.writeUInt16LE(name.length, 28);
    entry.writeUInt32LE(offset, 42);
    central.push(entry, name);

    offset += 30 + localName.length + body.length;
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

describe("socpak zip", () => {
  it("reads stored and deflated entries", () => {
    const entries = readZip(
      zip([
        { name: "a.xml", data: "<a/>" },
        { name: "b.xml", data: "<b/>".repeat(50), deflate: true },
      ]),
    );
    expect(entries.get("a.xml")?.toString()).toBe("<a/>");
    expect(entries.get("b.xml")?.toString()).toBe("<b/>".repeat(50));
  });

  it("picks the root container document, not the editor copy or nested ones", () => {
    const xml = readContainerXml(
      zip([
        { name: "sys_editor.xml", data: "<Editor/>" },
        { name: "sys/hcd/x.xml", localName: "sys\\hcd\\x.xml", data: "<Nested/>" },
        { name: "sys.xml", data: "<ObjectContainer/>", deflate: true },
      ]),
    );
    expect(xml).toBe("<ObjectContainer/>");
  });
});

describe("container placement", () => {
  it("rotates by a quaternion written w,x,y,z", () => {
    // 90 degrees about z takes +x to +y.
    const [x, y, z] = rotate([Math.SQRT1_2, 0, 0, Math.SQRT1_2], [1, 0, 0]);
    expect(x).toBeCloseTo(0);
    expect(y).toBeCloseTo(1);
    expect(z).toBeCloseTo(0);
  });

  it("composes child positions down the chain and reads the join id", () => {
    const placed = parseContainerXml(`
      <ObjectContainer>
        <ChildObjectContainers>
          <Child external="1" name="star.socpak" entityName="Star" pos="0,0,0" rot="1,0,0,0" starMapRecord="star-id"/>
          <Child external="1" name="planet.socpak" entityName="Planet" pos="1000,0,0" rot="${Math.SQRT1_2},0,0,${Math.SQRT1_2}" starMapRecord="planet-id">
            <InstanceMetadata/>
            <ChildObjectContainers>
              <Child entityName="Moon" pos="10,0,0" rot="1,0,0,0" starMapRecord="moon-id">
                <ChildObjectContainers>
                  <Child entityName="Wreck" pos="0,1,0"/>
                </ChildObjectContainers>
              </Child>
            </ChildObjectContainers>
          </Child>
          <Child entityName="After" pos="5,5,5"/>
        </ChildObjectContainers>
      </ObjectContainer>`);

    const by = new Map(placed.map((p) => [p.entityName, p]));
    expect(placed).toHaveLength(5);
    expect(by.get("Planet")).toMatchObject({ depth: 0, starMapRecord: "planet-id", container: "planet.socpak" });
    // The planet's rotation turns the moon's +x offset into +y.
    expect(by.get("Moon")?.position[0]).toBeCloseTo(1000);
    expect(by.get("Moon")?.position[1]).toBeCloseTo(10);
    expect(by.get("Moon")).toMatchObject({ depth: 1, parent: "Planet" });
    expect(by.get("Wreck")?.position[0]).toBeCloseTo(999);
    expect(by.get("Wreck")?.starMapRecord).toBeUndefined();
    // A sibling after a nested child is back at the root.
    expect(by.get("After")).toMatchObject({ depth: 0, position: [5, 5, 5] });
    expect(by.get("After")?.parent).toBeUndefined();
  });

  it("folds bodies held by a plain container into the system level", () => {
    const system = `
      <ObjectContainer><ChildObjectContainers>
        <Child external="1" name="Data/Ring/Segment.socpak" entityName="Segment" pos="1000,0,0" rot="${Math.SQRT1_2},0,0,${Math.SQRT1_2}"/>
        <Child external="1" name="Data/Ring/Missing.socpak" entityName="Gone" pos="0,0,0"/>
      </ChildObjectContainers></ObjectContainer>`;
    const docs: Record<string, string> = {
      "data/ring/segment.socpak": `<ObjectContainer><ChildObjectContainers>
        <Child external="1" name="moon.socpak" entityName="Moon" pos="10,0,0" starMapRecord="moon-id"/>
        <Child external="1" name="Data/Ring/Segment.socpak" entityName="Loop" pos="0,0,0"/>
      </ChildObjectContainers></ObjectContainer>`,
    };
    const missing: string[] = [];
    const placed = systemLevelPlacements(system, (key) => docs[key], (key) => missing.push(key));
    const moon = placed.find((p) => p.starMapRecord === "moon-id");
    // The segment's rotation turns the moon's +x offset into +y.
    expect(moon?.position[0]).toBeCloseTo(1000);
    expect(moon?.position[1]).toBeCloseTo(10);
    expect(moon?.parentRecord).toBeUndefined();
    expect(missing).toEqual(["data/ring/missing.socpak"]);
    // A container that holds itself is not followed again.
    expect(placed.filter((p) => p.entityName === "Moon")).toHaveLength(1);
  });
});

describe("readSpin", () => {
  it("reads a body's rotation period and rotating zone, and ignores a document without one", () => {
    const body = '<ObjectContainer name="stanton1" gridRadius="2000000" radius="1200000"><Entity label="stanton1" planetRadius="1000000" planetRotationSpeed="2.48" planetAxis="0,0,1"/></ObjectContainer>';
    expect(readSpin(body)).toEqual({ rotationHours: 2.48, gridRadius: 2_000_000 });
    expect(readSpin('<Entity planetRotationSpeed="5.1"/>')).toEqual({ rotationHours: 5.1 });
    expect(readSpin('<ObjectContainer gridRadius="500"><Entity label="station"/></ObjectContainer>')).toBeUndefined();
    expect(readSpin('<Entity planetRotationSpeed="0"/>')).toBeUndefined();
  });
});
