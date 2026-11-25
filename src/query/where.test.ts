import { describe, expect, it } from "vitest";
import { obtain, type ObtainReport } from "./obtain.js";
import type { ItemTags } from "./types.js";
import { orderByEffort, pickAmmo, renderWhere, sortSummaries, summaryRank, whereSummary } from "./where.js";

const gun: ItemTags = { name: "FS-9 LMG", className: "behr_lmg_ballistic_01", id: "id-fs9", tags: [] };

const base = (extra: Partial<ObtainReport> = {}): ObtainReport => ({
  ...obtain({ items: [gun] }, gun),
  ...extra,
});

const asOf = "2026-11-14T00:00:00Z";
const day = (iso: string) => Date.parse(iso) / 1000;

describe("where", () => {
  it("puts buying first, then rewards and crafting, then loot", () => {
    const report = obtain(
      {
        items: [gun],
        shops: [
          { item_uuid: "id-fs9", id_terminal: 1, terminal_name: "Old Cheap", price_buy: 5000, date_modified: day("2026-08-01") },
          { item_uuid: "id-fs9", id_terminal: 2, terminal_name: "Fresh", price_buy: 6000, date_modified: day("2026-11-10") },
          { item_uuid: "id-fs9", id_terminal: 3, terminal_name: "Dear", price_buy: 9000, date_modified: day("2026-11-12") },
        ],
        shopsAsOf: asOf,
        blueprints: [{ name: "BP_FS9", displayName: "FS-9 LMG", produces: "behr_lmg_ballistic_01", rewardedBy: [{ mission: "Bounty" }] }],
        contractAwards: [{ generator: "G", contract: "TheCollector_FS9", item: "behr_lmg_ballistic_01" }],
      },
      gun,
    );
    const withLoot: ObtainReport = {
      ...report,
      areas: [
        {
          area: "caves/loot_caves",
          configs: 1,
          best: { area: "caves/loot_caves", spawnTags: [], entity: "crate", entityName: "Weapons Crate", slotShare: 1, kind: "loot", score: 0.05 },
        },
      ],
    };
    const s = whereSummary(withLoot, { shopsLoaded: true });
    expect(s.rows.map((r) => r.method)).toEqual(["buy", "buy", "contract", "craft", "loot"]);
    expect(s.rows[0]).toMatchObject({ where: "Old Cheap", price: 5000, stale: true });
    expect(s.rows[0].detail).toContain("cheapest of 3 shops");
    // The cheapest row is stale, so the cheapest fresh one follows it.
    expect(s.rows[1]).toMatchObject({ where: "Fresh", price: 6000, stale: false });
    expect(s.rows[3].detail).toBe("blueprint from Bounty");
    expect(s.rows[4]).toMatchObject({ where: "caves: loot caves", odds: 0.05 });

    const text = renderWhere(s);
    expect(text.split("\n")[0]).toBe("FS-9 LMG  (behr_lmg_ballistic_01)");
    expect(text).toMatch(/^ {2}buy {6}Old Cheap .* 5,000 aUEC, 2026-08-01 stale; cheapest of 3 shops$/m);
  });

  it("honours a place and says when the item is only sold elsewhere", () => {
    const datasets = {
      items: [gun],
      shops: [{ item_uuid: "id-fs9", id_terminal: 1, terminal_name: "Guns Ruin", price_buy: 7000 }],
      terminals: [{ id: 1, nickname: "Guns Ruin", displayname: "Ruin Station", star_system_name: "Pyro" }],
    };
    const there = whereSummary(obtain(datasets, gun, undefined, { at: "ruin" }), { shopsLoaded: true });
    expect(there.rows[0].detail).toContain('only shop at "ruin"');
    const elsewhere = whereSummary(obtain(datasets, gun, undefined, { at: "grimhex" }), { shopsLoaded: true });
    expect(elsewhere.rows).toEqual([]);
    expect(elsewhere.notes).toEqual(['not sold at "grimhex" as far as UEX knows; 1 shop row(s) elsewhere, cheapest 7,000 aUEC at Guns Ruin (Ruin Station, Pyro) (undated stale)']);
    // Sold, just not there: the header says so and shows the cheapest shop elsewhere.
    const text = renderWhere(elsewhere);
    expect(text).not.toContain("no known source");
    expect(text).toContain('no source at "grimhex"; sold at 1 shop row(s) elsewhere');
    expect(text).toMatch(/buy {6}Guns Ruin \(Ruin Station, Pyro\) {2}7,000 aUEC, undated stale; cheapest elsewhere/);
    expect(text).not.toContain("note: not sold");
    expect(summaryRank(elsewhere)).toEqual([3.5, 7000]);
    expect(summaryRank(whereSummary(base()))).toEqual([4, Infinity]);
  });

  it("labels ammo and orders the buyable twin first", () => {
    const sold = whereSummary(base({ shops: [{ terminal: "Dumper's GrimHEX", location: "Grim HEX", price_buy: 275, stale: true }] }));
    const unsold = whereSummary(base());
    const loot = whereSummary(base({ otherTables: [{ table: "T", archetype: "A", share: 0.1, perRoll: 0.1 }] }));
    expect(orderByEffort([unsold, loot, sold])).toEqual([2, 1, 0]);
    expect(sortSummaries([unsold, sold])[0]).toBe(sold);
    const ammo = whereSummary(base({ ammoFor: [{ className: "grin_salvage_repair_01", name: "Cambio SRT" }] }));
    expect(renderWhere(ammo).split("\n")[0]).toBe("ammo for Cambio SRT: FS-9 LMG  (behr_lmg_ballistic_01)");
  });

  it("drops sourceless sibling magazines when another one has a source", () => {
    const sold = whereSummary(base({ shops: [{ terminal: "Guns", location: "Lorville", price_buy: 50, stale: false }] }));
    const creature = whereSummary(base());
    expect(pickAmmo([creature, sold, creature])).toEqual({ keep: [1], dropped: [0, 2] });
    // Nothing has a source: keep them all, so the reply still says so.
    expect(pickAmmo([creature, creature])).toEqual({ keep: [0, 1], dropped: [] });
  });

  it("notes missing shop data", () => {
    expect(whereSummary(base(), { shopsLoaded: false }).notes).toEqual(["no shop data loaded"]);
  });
});
