/**
 * "Where do I get X", one compact table: every source of an item from an
 * obtain() report, one row each, easiest first.
 *
 * Effort order: buy (1) beats a contract, mission or Wikelo reward or a crafting
 * blueprint (2), which beats loot (3). Inside a tier: cheapest shop first,
 * likeliest loot area first. Shop rows are UEX data (player-reported), not game
 * files; loot odds are the rough per-box estimates of obtain.ts.
 */

import { areaLabel, elsewhereText, pct, weaponsLabel, type ObtainReport } from "./obtain.js";
import { placeLabel, STALE_DAYS, type LocatedShop } from "./shops.js";

export type WhereMethod = "buy" | "contract" | "wikelo" | "craft" | "loot";

export interface WhereRow {
  method: WhereMethod;
  /** 1 buy, 2 reward or crafting, 3 loot. */
  effort: 1 | 2 | 3;
  where: string;
  detail: string;
  /** buy: aUEC. */
  price?: number;
  /** buy: last confirmed, YYYY-MM-DD. */
  updated?: string;
  stale?: boolean;
  /** loot: rough odds per spawned box. */
  odds?: number;
}

export interface WhereSummary {
  item: { className: string; name: string };
  ammoFor?: { className: string; name: string }[];
  at?: string;
  /** With `at`: shop rows for the item at other places. */
  shopsElsewhere?: number;
  /** With `at`: the cheapest of those. */
  cheapestElsewhere?: WhereRow;
  rows: WhereRow[];
  /** Lines worth saying that are not a source: no shop data, sold only elsewhere. */
  notes: string[];
}

const list = (names: readonly string[], max = 3) =>
  `${names.slice(0, max).join(", ")}${names.length > max ? ` +${names.length - max}` : ""}`;

function buyRow(s: LocatedShop, note: string): WhereRow {
  return {
    method: "buy",
    effort: 1,
    where: placeLabel(s),
    detail: `${(s.price_buy ?? 0).toLocaleString("en-US")} aUEC, ${s.updated ?? "undated"}${s.stale ? " stale" : ""}${note ? `; ${note}` : ""}`,
    price: s.price_buy,
    updated: s.updated,
    stale: s.stale,
  };
}

export function whereSummary(report: ObtainReport, options: { shopsLoaded?: boolean } = {}): WhereSummary {
  const rows: WhereRow[] = [];
  const notes: string[] = [];

  const buyable = report.shops.filter((s) => s.price_buy).sort((a, b) => (a.price_buy ?? 0) - (b.price_buy ?? 0));
  if (buyable.length > 0) {
    const at = report.at ? ` at "${report.at}"` : "";
    const count = buyable.length > 1 ? `cheapest of ${buyable.length} shops${at}` : `only shop${at}`;
    rows.push(buyRow(buyable[0], count));
    // The cheapest row is often months old; also show the cheapest one confirmed lately.
    const fresh = buyable.find((s) => !s.stale);
    if (buyable[0].stale && fresh) rows.push(buyRow(fresh, `cheapest confirmed in ${STALE_DAYS} days`));
  } else if (options.shopsLoaded === false) {
    notes.push("no shop data loaded");
  } else if (report.at) {
    notes.push(`not sold at "${report.at}" as far as UEX knows${elsewhereText(report)}`);
  }

  const byContract = new Map<string, number>();
  for (const a of report.contractAwards) byContract.set(a.contract, (byContract.get(a.contract) ?? 0) + (a.amount ?? 1));
  const contracts = [...byContract.keys()];
  if (contracts.length > 0) {
    rows.push({ method: "contract", effort: 2, where: list(contracts), detail: `contract award${contracts.length > 1 ? `, ${contracts.length} contracts` : ""}` });
  } else if (report.wikeloInferred.length > 0) {
    rows.push({ method: "wikelo", effort: 2, where: list(report.wikeloInferred.map((t) => t.name)), detail: "Wikelo trade, reward inferred from its name" });
  }

  for (const b of report.blueprints) {
    const missions = [...new Set((b.rewardedBy ?? []).map((r) => r.mission ?? r.contract).filter((m): m is string => !!m))];
    rows.push({
      method: "craft",
      effort: 2,
      where: `blueprint ${b.displayName ?? b.name}`,
      detail: missions.length > 0 ? `blueprint from ${list(missions)}` : "no mission in the data awards the blueprint",
    });
  }

  const areas = report.areas.slice(0, 2);
  for (const [i, { area, best }] of areas.entries()) {
    const box = best.kind === "loot" ? (best.entityName ?? best.entity) : `${best.kind}: ${best.entityName ?? best.entity}`;
    rows.push({
      method: "loot",
      effort: 3,
      where: areaLabel(area),
      detail: `${box}, ${pct(best.score)} per box${i === 0 && report.areas.length > 1 ? `; best of ${report.areas.length} areas` : ""}`,
      odds: best.score,
    });
  }
  if (areas.length === 0 && report.otherTables.length > 0) {
    const t = report.otherTables[0];
    rows.push({
      method: "loot",
      effort: 3,
      where: `table ${t.table}`,
      detail: `${pct(t.perRoll)} per roll; not placed by an area preset (corpses, events)${report.otherTables.length > 1 ? `; +${report.otherTables.length - 1} tables` : ""}`,
      odds: t.perRoll,
    });
  }

  rows.sort((a, b) => a.effort - b.effort);
  return {
    item: { className: report.item.className, name: report.item.name },
    ...(report.ammoFor ? { ammoFor: report.ammoFor } : {}),
    ...(report.at ? { at: report.at } : {}),
    ...(report.at && report.shopsElsewhere ? { shopsElsewhere: report.shopsElsewhere } : {}),
    ...(report.at && report.cheapestElsewhere ? { cheapestElsewhere: buyRow(report.cheapestElsewhere, "cheapest elsewhere") } : {}),
    rows,
    notes,
  };
}

/**
 * Best (lowest) effort of a summary, then price. An item with no source at the
 * place but shops elsewhere ranks 3.5; one with no source at all, 4.
 */
export function summaryRank(s: WhereSummary): [number, number] {
  const top = s.rows[0];
  if (top) return [top.effort, top.price ?? Infinity];
  return s.shopsElsewhere ? [3.5, s.cheapestElsewhere?.price ?? Infinity] : [4, Infinity];
}

/**
 * Ammo summaries worth showing, easiest first. A magazine that shares the
 * weapon's port tag but has no source at all (a creature's or NPC's twin of a
 * player magazine) is dropped when another one has a source; `dropped` names
 * them. Indexes into `list`.
 */
export function pickAmmo(list: readonly WhereSummary[]): { keep: number[]; dropped: number[] } {
  const order = orderByEffort(list);
  const sourced = order.filter((i) => summaryRank(list[i])[0] < 4);
  if (sourced.length === 0) return { keep: order, dropped: [] };
  return { keep: sourced, dropped: order.filter((i) => summaryRank(list[i])[0] >= 4) };
}

/** Indexes of `list`, easiest first, so the canister you can buy leads its unsold twin. Stable. */
export function orderByEffort(list: readonly WhereSummary[]): number[] {
  const ranks = list.map(summaryRank);
  return list.map((_, i) => i).sort((a, b) => ranks[a][0] - ranks[b][0] || ranks[a][1] - ranks[b][1]);
}

/** Summaries easiest first. Stable. */
export function sortSummaries(list: readonly WhereSummary[]): WhereSummary[] {
  return orderByEffort(list).map((i) => list[i]);
}

export function renderWhere(s: WhereSummary): string {
  const ammo = s.ammoFor?.length ? `ammo for ${weaponsLabel(s.ammoFor)}: ` : "";
  const lines = [`${ammo}${s.item.name}  (${s.item.className})${s.at ? `  at "${s.at}"` : ""}`];
  if (s.rows.length === 0 && s.at && s.shopsElsewhere) {
    lines.push(`  no source at "${s.at}"; sold at ${s.shopsElsewhere} shop row(s) elsewhere`);
    const c = s.cheapestElsewhere;
    if (c) lines.push(`  ${"buy".padEnd(8)} ${c.where}  ${c.detail}`);
  } else if (s.rows.length === 0) {
    lines.push("  no known source: not sold (UEX), awarded, craftable or rolled by any loot table");
  } else {
    const width = Math.min(56, Math.max(...s.rows.map((r) => r.where.length)));
    for (const r of s.rows) {
      const where = r.where.length > width ? `${r.where.slice(0, width - 1)}…` : r.where.padEnd(width);
      lines.push(`  ${r.method.padEnd(8)} ${where}  ${r.detail}`);
    }
  }
  // With no row the header above already says where it is sold.
  for (const n of s.rows.length === 0 && s.shopsElsewhere ? s.notes.filter((x) => !x.startsWith("not sold at")) : s.notes) lines.push(`  note: ${n}`);
  return lines.join("\n");
}
