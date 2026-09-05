import { describe, it, expect } from "vitest";
import { at, indexByName, normalizeRecord, num, refName, refShortName, str } from "./records.js";

describe("normalizeRecord", () => {
  const raw = {
    _RecordName_: "MineableElement.Quantainium_Raw",
    _RecordId_: "fc62043c",
    _RecordValue_: { _Type_: "MineableElement", elementInstability: 1000 },
  };

  it("unwraps the record envelope", () => {
    const record = normalizeRecord(raw, "mining/mineableelements/quantainium_raw");
    expect(record).toMatchObject({
      name: "MineableElement.Quantainium_Raw",
      shortName: "Quantainium_Raw",
      id: "fc62043c",
      type: "MineableElement",
    });
  });

  it("derives the type from the record name when _Type_ is absent", () => {
    const record = normalizeRecord(
      { _RecordName_: "Foo.Bar", _RecordValue_: { x: 1 } },
      "path/foo",
    );
    expect(record?.type).toBe("Foo");
    expect(record?.shortName).toBe("Bar");
  });

  it("leaves a name alone when it does not carry its type as a prefix", () => {
    const record = normalizeRecord(
      { _RecordName_: "Standalone", _RecordValue_: { _Type_: "Other" } },
      "p",
    );
    expect(record?.shortName).toBe("Standalone");
  });

  it("rejects files that are not record envelopes", () => {
    expect(normalizeRecord({ something: "else" }, "p")).toBeUndefined();
    expect(normalizeRecord({ _RecordName_: "OnlyName" }, "p")).toBeUndefined();
  });
});

describe("references", () => {
  const ref = { _RecordName_: "ResourceType.Raw_Gold", _RecordId_: "abc" };

  it("reads a reference's target name", () => {
    expect(refName(ref)).toBe("ResourceType.Raw_Gold");
  });

  it("strips the type prefix for display", () => {
    expect(refShortName(ref)).toBe("Raw_Gold");
  });

  it("returns nothing for values that are not references", () => {
    expect(refName(42)).toBeUndefined();
    expect(refName(null)).toBeUndefined();
    expect(refName([ref])).toBeUndefined();
    expect(refShortName({ noName: true })).toBeUndefined();
  });
});

describe("accessors", () => {
  it("walks a dotted path", () => {
    expect(at({ a: { b: { c: 7 } } }, "a.b.c")).toBe(7);
  });

  it("returns undefined instead of throwing on a missing path", () => {
    expect(at({ a: 1 }, "a.b.c")).toBeUndefined();
    expect(at(null, "a")).toBeUndefined();
  });

  it("num rejects non-finite values so NaN never reaches output", () => {
    expect(num(1.5)).toBe(1.5);
    expect(num(0)).toBe(0);
    expect(num(NaN)).toBeUndefined();
    expect(num(Infinity)).toBeUndefined();
    expect(num("3")).toBeUndefined();
    expect(num(null)).toBeUndefined();
  });

  it("str rejects empty strings", () => {
    expect(str("x")).toBe("x");
    expect(str("")).toBeUndefined();
    expect(str(5)).toBeUndefined();
  });
});

describe("indexByName", () => {
  it("keys records by full name for reference resolution", () => {
    const records = [
      { name: "A.One", shortName: "One", id: "1", type: "A", recordPath: "p1", value: {} },
      { name: "A.Two", shortName: "Two", id: "2", type: "A", recordPath: "p2", value: {} },
    ];
    const index = indexByName(records);
    expect(index.get("A.Two")?.id).toBe("2");
    expect(index.get("A.Missing")).toBeUndefined();
  });
});
