import { describe, it, expect } from "vitest";
import {
  isUnresolved,
  localize,
  locKey,
  locRefs,
  nameStatusField,
  lookupString,
  nameStatusOf,
  parseGlobalIni,
  searchStrings,
} from "./localization.js";

describe("parseGlobalIni", () => {
  it("reads key=value lines", () => {
    const table = parseGlobalIni("manufacturer_NameAEGS=Aegis Dynamics\nfoo=bar");
    expect(table.get("manufacturer_NameAEGS")).toBe("Aegis Dynamics");
  });

  it("strips the BOM so the first key is not corrupted", () => {
    // The real global.ini is UTF-8 with a BOM; without stripping it the very
    // first key silently never matches.
    const table = parseGlobalIni("﻿first_key=value");
    expect(table.get("first_key")).toBe("value");
  });

  it("handles CRLF line endings", () => {
    const table = parseGlobalIni("a=1\r\nb=2\r\n");
    expect(table.get("a")).toBe("1");
    expect(table.get("b")).toBe("2");
  });

  it("keeps '=' inside a value", () => {
    const table = parseGlobalIni("equation=x=y+1");
    expect(table.get("equation")).toBe("x=y+1");
  });

  it("skips comments, sections and blank lines", () => {
    const table = parseGlobalIni("; comment\n# other\n[Section]\n\nreal=yes");
    expect(table.size).toBe(1);
    expect(table.get("real")).toBe("yes");
  });
});

describe("localize", () => {
  const table = parseGlobalIni("manufacturer_NameAEGS=Aegis Dynamics\nempty_value=");

  it("resolves an @-prefixed key", () => {
    expect(localize(table, "@manufacturer_NameAEGS")).toBe("Aegis Dynamics");
  });

  it("passes through a string that is already plain text", () => {
    expect(localize(table, "Just A Name")).toBe("Just A Name");
  });

  it("treats @LOC_EMPTY as absent", () => {
    expect(localize(table, "@LOC_EMPTY")).toBeUndefined();
  });

  it("treats an empty resolved value as absent", () => {
    expect(localize(table, "@empty_value")).toBeUndefined();
  });

  it("matches case-insensitively", () => {
    expect(localize(table, "@MANUFACTURER_NAMEAEGS")).toBe("Aegis Dynamics");
  });

  it("returns the token when the key is missing, so gaps are visible", () => {
    const result = localize(table, "@manufacturer_NameNOPE");
    expect(result).toBe("@manufacturer_NameNOPE");
    expect(isUnresolved(result)).toBe(true);
  });

  it("returns undefined for non-strings and empty strings", () => {
    expect(localize(table, undefined)).toBeUndefined();
    expect(localize(table, 42)).toBeUndefined();
    expect(localize(table, "")).toBeUndefined();
  });
});

describe("searchStrings", () => {
  const table = parseGlobalIni(
    [
      "manufacturer_NameAEGS=Aegis Dynamics",
      "manufacturer_NameDRAK=Drake Interplanetary",
      "Stanton1=Hurston",
      "Stanton1c=Magda",
      "ATC_WikeloMods=Wikelo Emporium",
    ].join("\n"),
  );

  it("matches on the value, for finding a key from in-game text", () => {
    const { hits } = searchStrings(table, "Wikelo");
    expect(hits).toEqual([{ key: "ATC_WikeloMods", value: "Wikelo Emporium" }]);
  });

  it("matches on the key, for finding what a token resolves to", () => {
    const { total } = searchStrings(table, "manufacturer_Name");
    expect(total).toBe(2);
  });

  it("can be restricted to keys only", () => {
    // "Hurston" appears as a value; keysOnly should not match it.
    expect(searchStrings(table, "Hurston", { keysOnly: true }).total).toBe(0);
    expect(searchStrings(table, "Hurston").total).toBe(1);
  });

  it("caps hits while still reporting the true total", () => {
    const { total, hits } = searchStrings(table, "manufacturer", { limit: 1 });
    expect(total).toBe(2);
    expect(hits).toHaveLength(1);
  });

  it("is case-insensitive", () => {
    expect(searchStrings(table, "aegis").total).toBe(1);
  });
});

describe("lookupString", () => {
  const table = parseGlobalIni("Stanton1c=Magda\nmanufacturer_NameAEGS=Aegis Dynamics");

  it("accepts a key with or without the leading @", () => {
    expect(lookupString(table, "@manufacturer_NameAEGS")).toBe("Aegis Dynamics");
    expect(lookupString(table, "manufacturer_NameAEGS")).toBe("Aegis Dynamics");
  });

  it("resolves a bare location key, which is how body names are named", () => {
    expect(lookupString(table, "Stanton1c")).toBe("Magda");
  });

  it("returns undefined rather than a token when the key is absent", () => {
    expect(lookupString(table, "Nope")).toBeUndefined();
  });
});

describe("locKey / nameStatusOf", () => {
  const table = parseGlobalIni(
    [
      "item_NameReal=Arrowhead",
      "item_DescReal=A sniper rifle.",
      "item_NameP,P=Pembroke",
      "item_NameStub=<= PLACEHOLDER =>",
      "LOC_PLACEHOLDER=<= PLACEHOLDER =>",
      "item_NameBlank=",
      "Item_NameMixed=Mixed",
    ].join("\r\n"),
  );

  it("keeps a real key and calls it real", () => {
    expect(locKey(table, "@item_NameReal")).toBe("item_NameReal");
    expect(nameStatusOf(table, "@item_NameReal")).toBe("real");
  });

  it("localize resolves a key that exists only with ,P", () => {
    expect(localize(table, "@item_NameP")).toBe("Pembroke");
    expect(localize(table, "@ITEM_NAMEP")).toBe("Pembroke");
  });

  it("finds a key that exists only with ,P and returns the table's spelling", () => {
    expect(locKey(table, "@item_NameP")).toBe("item_NameP,P");
    expect(nameStatusOf(table, "@item_NameP")).toBe("real");
  });

  it("matches case-insensitively and returns the table's spelling", () => {
    expect(locKey(table, "@item_namemixed")).toBe("Item_NameMixed");
  });

  it("flags a placeholder value and the placeholder key", () => {
    expect(nameStatusOf(table, "@item_NameStub")).toBe("placeholder");
    expect(nameStatusOf(table, "@LOC_PLACEHOLDER")).toBe("placeholder");
    expect(locKey(table, "@item_NameStub")).toBe("item_NameStub");
  });

  it("keeps an unresolved key as referenced", () => {
    expect(locKey(table, "@item_NameGone")).toBe("item_NameGone");
    expect(nameStatusOf(table, "@item_NameGone")).toBe("unresolved");
  });

  it("treats unset values and empty markers as empty, with no key", () => {
    for (const value of [undefined, "", "@LOC_EMPTY", "@loc_uninitialized", "@"]) {
      expect(locKey(table, value)).toBeUndefined();
      expect(nameStatusOf(table, value)).toBe("empty");
    }
    expect(nameStatusOf(table, "@item_NameBlank")).toBe("empty");
  });

  it("gives literal text no key and classifies it by content", () => {
    expect(locKey(table, "Plain Name")).toBeUndefined();
    expect(nameStatusOf(table, "Plain Name")).toBe("real");
    expect(nameStatusOf(table, "< = TEST NAME = >")).toBe("placeholder");
    expect(nameStatusOf(table, "PH - 987_shoes_01")).toBe("placeholder");
    expect(nameStatusOf(table, "Phoenix")).toBe("real");
  });

  it("locRefs bundles name and description keys", () => {
    expect(locRefs(table, "@item_NameReal", "@item_DescReal")).toEqual({
      nameKey: "item_NameReal",
      descriptionKey: "item_DescReal",
    });
    expect(locRefs(table, "@item_NameReal").nameStatus).toBeUndefined();
    expect(locRefs(table, "@item_NameGone").nameStatus).toBe("unresolved");
  });

  it("nameStatusField omits only real", () => {
    expect(nameStatusField(table, "@item_NameReal")).toBeUndefined();
    expect(nameStatusField(table, "@item_NameStub")).toBe("placeholder");
  });
});
