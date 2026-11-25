import { describe, expect, it } from "vitest";

import {
  limitShop,
  locateShops,
  locator,
  normalisePlace,
  placeFilter,
  placeLabel,
  placeMatches,
  priceAge,
  renderShop,
  shopInventory,
  terminalLocation,
  type ShopPrice,
  type UexTerminal,
} from "./shops.js";
import type { ItemTags } from "./types.js";

const grimhex: UexTerminal = {
  id: 115,
  name: "Dumper's Depot - GrimHEX",
  nickname: "Dumper's GrimHEX",
  displayname: "Grim HEX",
  star_system_name: "Stanton",
  planet_name: "Crusader",
  orbit_name: "Crusader",
  moon_name: "Yela",
  space_station_name: "Green Imperial Housing Exchange",
  is_available: 1,
  is_available_live: 1,
};
const skutters: UexTerminal = { ...grimhex, id: 143, name: "Skutters - GrimHEX", nickname: "Skutters Grim" };
const arcL2: UexTerminal = {
  id: 93,
  nickname: "Clothing ARC-L2",
  displayname: "ARC-L2 Lively Pathway Station",
  star_system_name: "Stanton",
  planet_name: "ArcCorp",
  orbit_name: "ArcCorp Lagrange Point 2",
  space_station_name: "ARC-L2 Lively Pathway Station",
};
const gateway: UexTerminal = {
  id: 296,
  nickname: "Platinum Bay Terra (Stanton)",
  displayname: "Terra Gateway",
  star_system_name: "Stanton",
  orbit_name: "Terra Gateway (Stanton system)",
  space_station_name: "Terra Gateway (Stanton)",
  is_available_live: 0,
};
const terminals = [grimhex, skutters, arcL2, gateway];

// 2026-11-14T00:00:00Z
const NOW = Date.UTC(2026, 10, 14);
const daysAgo = (d: number) => Math.floor((NOW - d * 86_400_000) / 1000);

const canister: ShopPrice = { item_name: "Cambio SRT Canister", item_uuid: "can", id_terminal: 115, terminal_name: "Dumper's GrimHEX", price_buy: 275, date_modified: daysAgo(70) };
const tool: ShopPrice = { item_name: "Cambio SRT", item_uuid: "srt", id_terminal: 143, terminal_name: "Skutters Grim", price_buy: 21200, date_modified: daysAgo(2) };
const shirt: ShopPrice = { item_name: "Shirt", item_uuid: "shirt", id_terminal: 93, terminal_name: "Clothing ARC-L2", price_buy: 90, date_modified: daysAgo(1) };
const orphan: ShopPrice = { item_name: "Lost Thing", item_uuid: "lost", id_terminal: 9999, terminal_name: "Ghost Kiosk", price_buy: 5 };

describe("terminalLocation", () => {
  it("labels the place, moon, planet and system, skipping restated parts", () => {
    expect(terminalLocation(grimhex).label).toBe("Grim HEX, Yela, Crusader, Stanton");
    expect(terminalLocation(arcL2).label).toBe("ARC-L2 Lively Pathway Station, ArcCorp Lagrange Point 2, Stanton");
    expect(terminalLocation(gateway).label).toBe("Terra Gateway, Stanton");
    expect(terminalLocation(gateway).live).toBe(false);
    expect(terminalLocation(grimhex).terminal).toBe("Dumper's GrimHEX");
  });

  it("keeps every name searchable, the station's long name included", () => {
    const { tokens } = terminalLocation(grimhex);
    for (const t of ["grimhex", "yela", "crusader", "stanton", "greenimperialhousingexchange", "dumpersgrimhex"]) {
      expect(tokens).toContain(t);
    }
  });

  it("falls back to the price row's terminal name", () => {
    const loc = terminalLocation(undefined, "Ghost Kiosk");
    expect(loc).toMatchObject({ terminal: "Ghost Kiosk", label: "Ghost Kiosk", tokens: ["ghostkiosk"] });
  });
});

describe("placeMatches", () => {
  const loc = terminalLocation(grimhex);
  it("ignores case, spaces and punctuation", () => {
    expect(normalisePlace("Grim HEX")).toBe("grimhex");
    for (const q of ["grimhex", "grim hex", "GrimHEX", "Grim-Hex", "Yela", "crusader", "Stanton", "dumper's", "dumpers grimhex"]) {
      expect(placeMatches(loc, q), q).toBe(true);
    }
  });

  it("rejects other places and empty queries", () => {
    expect(placeMatches(loc, "orison")).toBe(false);
    expect(placeMatches(loc, "grim orison")).toBe(false);
    expect(placeMatches(loc, "  ")).toBe(false);
  });
});

describe("priceAge", () => {
  it("dates the row and flags it past 30 days", () => {
    expect(priceAge(daysAgo(2), NOW)).toEqual({ updated: "2026-11-12", stale: false });
    expect(priceAge(daysAgo(31), NOW).stale).toBe(true);
    expect(priceAge(daysAgo(10), NOW, 7).stale).toBe(true);
    expect(priceAge(undefined, NOW)).toEqual({ stale: true });
  });
});

describe("locateShops", () => {
  const rows = [canister, tool, shirt, orphan];

  it("joins on id_terminal and keeps unknown terminals by name", () => {
    const located = locateShops(rows, { terminals, asOf: new Date(NOW).toISOString() });
    expect(located.map((r) => [r.terminal, r.location, r.stale])).toEqual([
      ["Dumper's GrimHEX", "Grim HEX, Yela, Crusader, Stanton", true],
      ["Skutters Grim", "Grim HEX, Yela, Crusader, Stanton", false],
      ["Clothing ARC-L2", "ARC-L2 Lively Pathway Station, ArcCorp Lagrange Point 2, Stanton", false],
      ["Ghost Kiosk", "Ghost Kiosk", true],
    ]);
    expect(located[0]).toMatchObject({ updated: "2026-09-05", system: "Stanton", price_buy: 275 });
  });

  it("filters by place", () => {
    expect(locateShops(rows, { terminals, asOf: NOW, at: "grim hex" }).map((r) => r.item_uuid)).toEqual(["can", "srt"]);
    expect(locateShops(rows, { terminals, asOf: NOW, at: "ghost" }).map((r) => r.item_uuid)).toEqual(["lost"]);
    // Without terminals only the terminal name can match.
    expect(locateShops(rows, { asOf: NOW, at: "yela" })).toEqual([]);
    expect(locateShops(rows, { asOf: NOW, at: "skutters" }).map((r) => r.item_uuid)).toEqual(["srt"]);
  });
});

describe("shopInventory", () => {
  const items: ItemTags[] = [
    { name: "Cambio SRT Canister", className: "grin_tool_resource_salvage_repair_01", id: "can", tags: [] },
    { name: "Cambio SRT", className: "grin_salvage_repair_01", id: "srt", tags: [] },
    { name: "grin_unnamed", className: "grin_unnamed", id: "tpl", isTemplate: true, tags: [] },
  ];
  const extra: ShopPrice = { item_name: "UEX Name", item_uuid: "tpl", id_terminal: 115, price_buy: 10, date_modified: daysAgo(1) };

  it("groups every item at a place by terminal, with game names", () => {
    const report = shopInventory({ shops: [canister, tool, shirt, orphan, extra], terminals, items, asOf: NOW }, "GrimHEX");
    expect(report.rows).toBe(3);
    expect(report.terminals.map((t) => t.terminal)).toEqual(["Dumper's GrimHEX", "Skutters Grim"]);
    expect(report.terminals[0].items).toEqual([
      { name: "Cambio SRT Canister", key: "grin_tool_resource_salvage_repair_01", price_buy: 275, updated: "2026-09-05", stale: true },
      // A template record has no display name, so UEX's wins.
      { name: "UEX Name", key: "grin_unnamed", price_buy: 10, updated: "2026-11-13", stale: false },
    ]);
    const text = renderShop(report);
    expect(text).toContain("2 terminal(s)");
    expect(text).toContain("Skutters Grim (Grim HEX, Yela, Crusader, Stanton)");
    expect(text).toMatch(/Cambio SRT Canister\s+275 {2}2026-09-05 stale/);
  });

  it("says when nothing matches", () => {
    expect(renderShop(shopInventory({ shops: [tool], terminals }, "orison"))).toContain('No UEX terminal at "orison"');
  });
});

describe("place labels and matching on real-shaped terminals", () => {
  const sys = (star_system_name: string) => ({ star_system_name });
  const personal: UexTerminal = { id: 732, name: "Personal Weapons - Security Tower", nickname: "Personal Weapons", displayname: "Personal Weapons", city_name: "Lorville", orbit_name: "Hurston", planet_name: "Hurston", ...sys("Stanton") };
  const noodle: UexTerminal = { id: 829, name: "Noodle Bar - Lorville", nickname: "Noodle Bar - Lorville", displayname: "Noodle Bar - Lorville", city_name: "Lorville", orbit_name: "Hurston", planet_name: "Hurston", ...sys("Stanton") };
  const aeHub: UexTerminal = { id: 731, name: "AydoExpress Hub", nickname: "AE Hub", displayname: "AE Hub", space_station_name: "Seraphim Station", orbit_name: "Crusader", planet_name: "Crusader", ...sys("Stanton") };
  const arcL1: UexTerminal = { id: 131, name: "Platinum Bay - ARC-L1", nickname: "Platinum ARC-L1", displayname: "ARC-L1 Wide Forest Station", space_station_name: "ARC-L1 Wide Forest Station", orbit_name: "ArcCorp Lagrange Point 1", planet_name: "ArcCorp", ...sys("Stanton") };
  const pyroGatewayStanton: UexTerminal = { id: 307, name: "Platinum Bay - Pyro Gateway (Stanton)", nickname: "Platinum Pyro Gateway (Stanton)", displayname: "Pyro Gateway", space_station_name: "Pyro Gateway (Stanton)", orbit_name: "Pyro Gateway (Stanton system)", ...sys("Stanton") };
  const rustville: UexTerminal = { id: 600, name: "Admin - Rustville", nickname: "Rustville", displayname: "Rustville", outpost_name: "Rustville", orbit_name: "Pyro I", planet_name: "Pyro I", ...sys("Pyro") };
  const kc: UexTerminal = { ...grimhex, id: 341, name: "KC Trending - GrimHEX", nickname: "KC Trending GrimHEX", displayname: "GrimHEX" };
  const admin: UexTerminal = { ...grimhex, id: 27, name: "Admin - GrimHEX", nickname: "GrimHEX" };
  const all = [personal, noodle, aeHub, arcL1, pyroGatewayStanton, rustville, kc, admin, ...terminals];
  const where = locator(all);
  const locs = all.map((t) => where({ id_terminal: t.id }));
  const hits = (q: string) => locs.filter(placeFilter(locs, q)).map((l) => l.terminal).sort();

  it("names the city or station behind a per-terminal displayname, and always the system", () => {
    expect(terminalLocation(personal).label).toBe("Lorville, Hurston, Stanton");
    expect(terminalLocation(aeHub).label).toBe("Seraphim Station, Crusader, Stanton");
    expect(terminalLocation(rustville).label).toBe("Rustville, Pyro I, Pyro");
    expect(terminalLocation(pyroGatewayStanton).label).toBe("Pyro Gateway, Stanton");
    // Grim HEX is the place, named at the end of the terminal name; the nickname alone does not hide it.
    expect(terminalLocation(admin).label).toBe("Grim HEX, Yela, Crusader, Stanton");
  });

  it("spells one place one way and skips a place the terminal name repeats", () => {
    expect(where({ id_terminal: 341 }).label).toBe("Grim HEX, Yela, Crusader, Stanton");
    expect(placeLabel({ terminal: "Platinum Pyro Gateway (Stanton)", location: "Pyro Gateway, Stanton" })).toBe("Platinum Pyro Gateway (Stanton)");
  });

  it("drops filler words and matches whole words", () => {
    expect(hits("grim hex station")).toEqual(["Dumper's GrimHEX", "GrimHEX", "KC Trending GrimHEX", "Skutters Grim"]);
    expect(hits("arc-l1")).toEqual(["Platinum ARC-L1"]);
    expect(hits("lorville")).toEqual(["Noodle Bar - Lorville", "Personal Weapons"]);
  });

  it("prefers a system, city or station name over a word in a terminal name", () => {
    expect(hits("pyro")).toEqual(["Rustville"]);
    expect(hits("stanton")).not.toContain("Rustville");
    expect(hits("seraphim")).toEqual(["AE Hub"]);
  });

  it("matches nothing for queries under three letters", () => {
    expect(hits("l1")).toEqual([]);
    expect(hits("in")).toEqual([]);
    expect(placeMatches(terminalLocation(arcL1), "nb")).toBe(false);
    expect(renderShop(shopInventory({ shops: [shirt], terminals }, "nb"))).toContain("3+ letters");
  });

  it("falls back to a substring for four or more letters when no word matches", () => {
    expect(hits("ville")).toEqual(["Noodle Bar - Lorville", "Personal Weapons", "Rustville"]);
  });
});

describe("limitShop", () => {
  const many: UexTerminal[] = Array.from({ length: 20 }, (_, i) => ({ id: 1000 + i, nickname: `Kiosk ${i}`, displayname: "Big Place", star_system_name: "Stanton" }));
  const rows: ShopPrice[] = many.flatMap((t) => Array.from({ length: 30 }, (_, j) => ({ item_name: `Item ${String(j).padStart(2, "0")}`, id_terminal: t.id, price_buy: 10 + j, date_modified: daysAgo(1) })));

  it("caps items per terminal and reports the total", () => {
    const report = shopInventory({ shops: rows.slice(0, 30), terminals: many, asOf: NOW }, "big place");
    const cut = limitShop(report);
    expect(cut.terminals[0].items).toHaveLength(25);
    expect(cut.terminals[0].itemCount).toBe(30);
    expect(renderShop(report)).toContain("... 5 more");
    expect(limitShop(report, { limit: Infinity }).terminals[0].items).toHaveLength(30);
  });

  it("lists places instead of items when too many terminals match", () => {
    const report = shopInventory({ shops: rows, terminals: many, asOf: NOW }, "stanton");
    const cut = limitShop(report);
    expect(cut).toMatchObject({ listOnly: true, terminals: [], terminalCount: 20, placeCount: 1, rows: 600 });
    expect(cut.places).toEqual([{ location: "Big Place, Stanton", terminals: 20, items: 600 }]);
    const text = renderShop(report);
    expect(text).toContain("More than 15 terminals match");
    expect(text).toMatch(/Big Place, Stanton\s+20 terminal\(s\)\s+600 item\(s\)/);
    expect(text.length).toBeLessThan(1000);
  });
});
