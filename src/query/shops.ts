/**
 * Shop rows with a place and an age: UEX item prices joined to UEX terminals.
 *
 * Shop stock is server-side and in no game file, so both inputs are UEX data,
 * reported by players. A price row names its terminal by id and nickname only
 * ("Dumper's GrimHEX"); the terminal row adds the station, moon, planet and
 * system. A row whose terminal is missing keeps its nickname as the place.
 *
 * Each row carries the day UEX last had it confirmed (date_modified) and a
 * stale flag when that is more than `staleDays` (30) before the reference time:
 * the price file's fetch time, or now.
 *
 * Place matching works on whole words and takes the best tier that hits any
 * terminal: (1) the query is one of a terminal's names (system, planet, moon,
 * station, city, outpost, terminal), (2) every query word is a word, or the
 * start of a word, of some name, (3) for 4+ letters, the query sits inside a
 * name. Case, spaces and punctuation are ignored ("grimhex" = "Grim HEX"), and
 * filler words ("station", "at", "shop") are dropped. So "pyro" means the Pyro
 * system, not the Pyro Gateway on the Stanton side, and "l1" does not hit
 * Lorville.
 */

import type { ItemTags } from "./types.js";

export interface ShopPrice {
  item_name?: string;
  item_uuid?: string;
  id_terminal?: number;
  terminal_name?: string;
  price_buy?: number;
  price_sell?: number;
  /** Unix seconds: when UEX last had the row confirmed. */
  date_modified?: number;
}

/** A UEX terminal, trimmed to what places it. */
export interface UexTerminal {
  id: number;
  name?: string;
  fullname?: string;
  nickname?: string;
  displayname?: string;
  code?: string;
  type?: string;
  star_system_name?: string;
  planet_name?: string;
  orbit_name?: string;
  moon_name?: string;
  space_station_name?: string;
  city_name?: string;
  outpost_name?: string;
  is_available?: number;
  is_available_live?: number;
}

export interface ShopLocation {
  /** The terminal as price rows name it, e.g. "Dumper's GrimHEX". */
  terminal: string;
  terminalId?: number;
  /** Place, moon or orbit, planet, system, e.g. "Grim HEX, Yela, Crusader, Stanton". */
  label: string;
  system?: string;
  /** false when UEX marks the terminal unavailable on LIVE. */
  live?: boolean;
  /** Normalised names to match a place against. */
  tokens: string[];
  /** Lower-case words of every name. */
  words?: string[];
}

export interface LocatedShop extends ShopPrice {
  terminal: string;
  location: string;
  system?: string;
  live?: boolean;
  /** date_modified as YYYY-MM-DD (UTC). */
  updated?: string;
  /** Not confirmed within staleDays of the reference time, or never dated. */
  stale: boolean;
}

export const STALE_DAYS = 30;
const DAY_MS = 86_400_000;

/** Lower case, letters and digits only: "Grim HEX" -> "grimhex". */
export const normalisePlace = (s: string): string => s.toLowerCase().replace(/[^a-z0-9]/g, "");

const text = (v: unknown): string | undefined => (typeof v === "string" && v.trim() ? v.trim() : undefined);

const words = (s: string): string[] => s.toLowerCase().split(/[^a-z0-9]+/).filter(Boolean);

/** Same place when equal, or one name's words start the other's ("Terra Gateway" / "Terra Gateway (Stanton system)"). */
function sameName(a: string, b: string): boolean {
  if (normalisePlace(a) === normalisePlace(b)) return true;
  const [x, y] = [words(a), words(b)].sort((p, q) => p.length - q.length);
  return x.length > 0 && x.every((w, i) => y[i] === w);
}

const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** Where a terminal is: place, moon, orbit, planet, then the star system unless a part already is it. */
export function terminalLocation(t: UexTerminal | undefined, fallbackName?: string): ShopLocation {
  const terminal = text(t?.nickname) ?? text(fallbackName) ?? text(t?.name) ?? (t ? `terminal ${t.id}` : "unknown terminal");
  if (!t) {
    return { terminal, label: terminal, tokens: [normalisePlace(terminal)].filter(Boolean), words: words(terminal) };
  }
  const system = text(t.star_system_name);
  // "Pyro Gateway (Stanton)" in Stanton: the parenthesised system adds nothing next to ", Stanton".
  const clean = (c: string | undefined) =>
    c && system ? c.replace(new RegExp(`\\s*\\(${escapeRe(system)}( system)?\\)$`, "i"), "") || c : c;
  const fixed = text(t.space_station_name) ?? text(t.city_name) ?? text(t.outpost_name);
  const display = text(t.displayname);
  // A displayname is sometimes the place's common name (Grim HEX for Green Imperial Housing Exchange,
  // named at the end of "Dumper's Depot - GrimHEX"), sometimes the terminal's own ("Personal Weapons",
  // "AE Hub", "Arkanis - Everus Harbor"), which would hide the city or station.
  const name = text(t.name);
  const namesPlace = (d: string) => !!name && normalisePlace(name) !== normalisePlace(d) && normalisePlace(name).endsWith(normalisePlace(d));
  const longer = (a: string, b: string) => ((clean(b) ?? b).length > (clean(a) ?? a).length ? b : a);
  const place =
    display && fixed && sameName(display, fixed)
      ? longer(display, fixed)
      : display && (!fixed || namesPlace(display))
        ? display
        : (fixed ?? display);
  const parts: string[] = [];
  for (const c of [place, text(t.moon_name), text(t.orbit_name), text(t.planet_name)].map(clean)) {
    if (!c || parts.some((p) => sameName(p, c))) continue;
    parts.push(c);
  }
  if (system && !parts.some((p) => normalisePlace(p) === normalisePlace(system))) parts.push(system);
  const label = parts.join(", ");
  // Every name is searchable, the station's long name (Green Imperial Housing Exchange) included.
  const names = [terminal, t.name, t.fullname, t.nickname, t.displayname, t.code, t.space_station_name, t.city_name, t.outpost_name, t.moon_name, t.orbit_name, t.planet_name, t.star_system_name, fallbackName]
    .map(text)
    .filter((n): n is string => !!n);
  const tokens = [...new Set(names.map(normalisePlace).filter(Boolean))];
  return {
    terminal,
    terminalId: t.id,
    label: label || terminal,
    system,
    live: t.is_available_live === 0 || t.is_available === 0 ? false : undefined,
    tokens,
    words: [...new Set(names.flatMap(words))],
  };
}

/** Resolves price rows to places, one lookup per terminal; one spelling per place ("Grim HEX" over "GrimHEX"). */
export function locator(terminals: readonly UexTerminal[] = []): (row: ShopPrice) => ShopLocation {
  const byId = new Map<number, UexTerminal>();
  for (const t of terminals) if (typeof t?.id === "number") byId.set(t.id, t);
  // The spelling most terminals at a place use wins; ties go to the first seen.
  const counts = new Map<string, Map<string, number>>();
  for (const t of byId.values()) {
    const label = terminalLocation(t).label;
    const key = normalisePlace(label);
    const forKey = counts.get(key) ?? new Map<string, number>();
    forKey.set(label, (forKey.get(label) ?? 0) + 1);
    counts.set(key, forKey);
  }
  const spelling = new Map<string, string>();
  for (const [key, forKey] of counts) spelling.set(key, [...forKey].reduce((a, b) => (b[1] > a[1] ? b : a))[0]);
  const canonical = (loc: ShopLocation): ShopLocation => {
    const label = spelling.get(normalisePlace(loc.label));
    return label === undefined || label === loc.label ? loc : { ...loc, label };
  };
  const memo = new Map<string, ShopLocation>();
  return (row) => {
    const key = `${row.id_terminal ?? ""}|${row.terminal_name ?? ""}`;
    let loc = memo.get(key);
    if (!loc) {
      loc = canonical(terminalLocation(row.id_terminal !== undefined ? byId.get(row.id_terminal) : undefined, row.terminal_name));
      memo.set(key, loc);
    }
    return loc;
  };
}

/** Words a place query can carry that name no place. */
const FILLER = new Set(["station", "at", "in", "on", "the", "of", "shop", "shops", "store", "terminal", "terminals", "near", "buy"]);

/** Shortest place query, after normalising, that is matched at all. */
export const MIN_PLACE_LENGTH = 3;

export interface PlaceQuery {
  whole: string;
  words: string[];
}

/** The query's words without filler; undefined when too short to match. */
export function placeQuery(place: string): PlaceQuery | undefined {
  const all = words(place);
  const kept = all.filter((w) => !FILLER.has(w));
  const used = kept.length > 0 ? kept : all;
  const whole = used.join("");
  return whole.length >= MIN_PLACE_LENGTH ? { whole, words: used } : undefined;
}

/** 1 exact name, 2 whole words or word starts, 3 substring (4+ letters); undefined when no match. */
export function placeTier(location: Pick<ShopLocation, "tokens" | "words">, query: PlaceQuery): 1 | 2 | 3 | undefined {
  if (location.tokens.includes(query.whole)) return 1;
  const ws = location.words ?? [];
  const wordHit = (q: string) => ws.some((w) => w === q || (q.length >= 3 && w.startsWith(q))) || location.tokens.some((t) => t.startsWith(q) && q.length >= 3);
  if (query.words.every(wordHit)) return 2;
  if (query.whole.length >= 4 && location.tokens.some((t) => t.includes(query.whole))) return 3;
  return undefined;
}

/**
 * A filter for the locations matching `place` at the best tier any of
 * `candidates` reaches, so a name match is not drowned by word or substring hits.
 */
export function placeFilter(candidates: readonly ShopLocation[], place: string): (location: ShopLocation) => boolean {
  const query = placeQuery(place);
  if (!query) return () => false;
  let best: number | undefined;
  for (const c of candidates) {
    const tier = placeTier(c, query);
    if (tier !== undefined && (best === undefined || tier < best)) best = tier;
    if (best === 1) break;
  }
  return (location) => best !== undefined && placeTier(location, query) === best;
}

/** True when the place query names this location on its own (any tier); an empty or too short query matches nothing. */
export function placeMatches(location: Pick<ShopLocation, "tokens" | "words">, place: string): boolean {
  const query = placeQuery(place);
  return !!query && placeTier(location, query) !== undefined;
}

/** Reference time in ms from an ISO string or ms number; now when absent or unreadable. */
export function referenceTime(asOf?: string | number): number {
  const ms = typeof asOf === "number" ? asOf : asOf ? Date.parse(asOf) : NaN;
  return Number.isFinite(ms) ? ms : Date.now();
}

export function priceAge(dateModified: number | undefined, refMs: number, staleDays = STALE_DAYS): { updated?: string; stale: boolean } {
  if (typeof dateModified !== "number" || !Number.isFinite(dateModified) || dateModified <= 0) return { stale: true };
  const ms = dateModified * 1000;
  return { updated: new Date(ms).toISOString().slice(0, 10), stale: refMs - ms > staleDays * DAY_MS };
}

export interface LocateOptions {
  terminals?: readonly UexTerminal[];
  /** ISO string or ms: the price file's fetch time. Default now. */
  asOf?: string | number;
  staleDays?: number;
  /** Keep only rows at this place. */
  at?: string;
}

export function locateShops(rows: readonly ShopPrice[], options: LocateOptions = {}): LocatedShop[] {
  const where = locator(options.terminals);
  const ref = referenceTime(options.asOf);
  // The tier is chosen over every terminal, not just these rows, so "pyro" stays the system for any item.
  const matches =
    options.at === undefined
      ? undefined
      : placeFilter([...(options.terminals ?? []).map((t) => terminalLocation(t)), ...rows.map(where)], options.at);
  const out: LocatedShop[] = [];
  for (const row of rows) {
    const loc = where(row);
    if (matches && !matches(loc)) continue;
    out.push({
      ...row,
      terminal: loc.terminal,
      location: loc.label,
      ...(loc.system ? { system: loc.system } : {}),
      ...(loc.live === false ? { live: false } : {}),
      ...priceAge(row.date_modified, ref, options.staleDays),
    });
  }
  return out;
}

/** "Dumper's GrimHEX  Grim HEX, Yela, Crusader, Stanton", without repeating a place the terminal already names. */
export function placeLabel(shop: Pick<LocatedShop, "terminal" | "location">): string {
  // "Platinum Pyro Gateway (Stanton)" already says "Pyro Gateway, Stanton".
  if (normalisePlace(shop.terminal).includes(normalisePlace(shop.location))) return shop.terminal;
  return `${shop.terminal} (${shop.location})`;
}

export function shopRowText(shop: LocatedShop): string {
  const price = shop.price_buy ? `${shop.price_buy.toLocaleString("en-US")} aUEC` : shop.price_sell ? `sells for ${shop.price_sell.toLocaleString("en-US")} aUEC` : "no price";
  const age = `${shop.updated ?? "undated"}${shop.stale ? " stale" : ""}`;
  return `${price.padStart(12)}  ${age.padEnd(16)} ${placeLabel(shop)}${shop.live === false ? " [not on LIVE]" : ""}`;
}

// ---------------------------------------------------------------- shop inventory

export interface ShopItem {
  name: string;
  /** Item-tags class name when the row's item_uuid is a known record. */
  key?: string;
  /** UEX's name, when it differs from the game's. */
  uexName?: string;
  price_buy?: number;
  price_sell?: number;
  updated?: string;
  stale: boolean;
}

export interface ShopTerminal {
  terminal: string;
  terminalId?: number;
  location: string;
  live?: boolean;
  items: ShopItem[];
  /** Items at the terminal before a limit cut `items` short. */
  itemCount?: number;
}

export interface ShopReport {
  place: string;
  terminals: ShopTerminal[];
  /** Price rows that matched, over every terminal. */
  rows: number;
  /** Set by limitShop: items kept per terminal, and whether the terminals were cut to a place list. */
  itemLimit?: number;
  terminalLimit?: number;
  /** More terminals matched than terminalLimit: `terminals` is emptied and `places` lists where they are. */
  listOnly?: boolean;
  /** With listOnly: matched places, most items first, at most PLACE_LIST_LIMIT. */
  places?: ShopPlace[];
  /** With listOnly: places before that cut, and terminals matched. */
  placeCount?: number;
  terminalCount?: number;
}

export interface ShopPlace {
  location: string;
  terminals: number;
  items: number;
}

/** Places listed when a query matches too many terminals. */
export const PLACE_LIST_LIMIT = 40;

/** Items per terminal and terminals per report shown by default; a broad place gets a list of terminals instead. */
export const SHOP_ITEM_LIMIT = 25;
export const SHOP_TERMINAL_LIMIT = 15;

export interface ShopInputs {
  shops: readonly ShopPrice[];
  terminals?: readonly UexTerminal[];
  /** Game records, for display names by item_uuid. */
  items?: readonly ItemTags[];
  asOf?: string | number;
  staleDays?: number;
}

/** Every item UEX lists at the terminals matching `place`, grouped by terminal. */
export function shopInventory(inputs: ShopInputs, place: string): ShopReport {
  const byId = new Map<string, ItemTags>();
  for (const i of inputs.items ?? []) byId.set(i.id, i);
  const rows = locateShops(inputs.shops, { terminals: inputs.terminals, asOf: inputs.asOf, staleDays: inputs.staleDays, at: place });
  const groups = new Map<string, ShopTerminal>();
  for (const r of rows) {
    if (!r.price_buy && !r.price_sell) continue;
    const id = `${r.id_terminal ?? r.terminal}`;
    let g = groups.get(id);
    if (!g) {
      g = { terminal: r.terminal, terminalId: r.id_terminal, location: r.location, ...(r.live === false ? { live: false } : {}), items: [] };
      groups.set(id, g);
    }
    const record = r.item_uuid ? byId.get(r.item_uuid) : undefined;
    const gameName = record && !record.isTemplate ? record.name : undefined;
    const name = gameName ?? r.item_name ?? record?.name ?? r.item_uuid ?? "unknown item";
    g.items.push({
      name,
      ...(record ? { key: record.className } : {}),
      ...(gameName && r.item_name && r.item_name !== gameName ? { uexName: r.item_name } : {}),
      ...(r.price_buy ? { price_buy: r.price_buy } : {}),
      ...(r.price_sell ? { price_sell: r.price_sell } : {}),
      ...(r.updated ? { updated: r.updated } : {}),
      stale: r.stale,
    });
  }
  const terminals = [...groups.values()].sort(
    (a, b) => normalisePlace(a.location).localeCompare(normalisePlace(b.location)) || a.terminal.localeCompare(b.terminal),
  );
  for (const g of terminals) g.items.sort((a, b) => a.name.localeCompare(b.name));
  return { place, terminals, rows: terminals.reduce((s, g) => s + g.items.length, 0) };
}

/**
 * Cuts a report to the first `limit` items per terminal (by name) and, past
 * `maxTerminals` terminals, to terminals with item counts but no items. Totals
 * stay, so the cut is visible. Infinity turns either cut off.
 */
export function limitShop(report: ShopReport, options: { limit?: number; maxTerminals?: number } = {}): ShopReport {
  const limit = options.limit ?? SHOP_ITEM_LIMIT;
  const maxTerminals = options.maxTerminals ?? SHOP_TERMINAL_LIMIT;
  const caps = {
    ...(Number.isFinite(limit) ? { itemLimit: limit } : {}),
    ...(Number.isFinite(maxTerminals) ? { terminalLimit: maxTerminals } : {}),
  };
  if (report.terminals.length > maxTerminals) {
    const places = new Map<string, ShopPlace>();
    for (const g of report.terminals) {
      const p = places.get(g.location) ?? { location: g.location, terminals: 0, items: 0 };
      p.terminals += 1;
      p.items += g.items.length;
      places.set(g.location, p);
    }
    const all = [...places.values()].sort((a, b) => b.items - a.items || a.location.localeCompare(b.location));
    return {
      place: report.place,
      rows: report.rows,
      terminals: [],
      ...caps,
      listOnly: true,
      places: all.slice(0, PLACE_LIST_LIMIT),
      placeCount: all.length,
      terminalCount: report.terminals.length,
    };
  }
  return { ...report, terminals: report.terminals.map((g) => ({ ...g, itemCount: g.items.length, items: g.items.slice(0, limit) })), ...caps };
}

export function renderShop(report: ShopReport, options: { limit?: number; maxTerminals?: number } = {}): string {
  if (report.terminals.length === 0) {
    const short = placeQuery(report.place) ? "" : ` (a place needs ${MIN_PLACE_LENGTH}+ letters)`;
    return `No UEX terminal at "${report.place}" lists items${short}.`;
  }
  const cut = limitShop(report, options);
  const lines = [`${report.terminals.length} terminal(s) at "${cut.place}", ${cut.rows} item row(s) (UEX, player-reported; stale = not confirmed in ${STALE_DAYS} days)`];
  if (cut.listOnly) {
    lines.push(`More than ${cut.terminalLimit} terminals match; name one place (station, city, outpost or terminal). ${cut.placeCount} place(s):`);
    for (const p of cut.places ?? []) {
      lines.push(`  ${p.location.slice(0, 64).padEnd(64)} ${String(p.terminals).padStart(3)} terminal(s) ${String(p.items).padStart(5)} item(s)`);
    }
    const more = (cut.placeCount ?? 0) - (cut.places?.length ?? 0);
    if (more > 0) lines.push(`  ... ${more} more places`);
    return lines.join("\n");
  }
  for (const g of cut.terminals) {
    lines.push("", `${placeLabel(g)}${g.live === false ? " [not on LIVE]" : ""}  ${g.itemCount ?? g.items.length} item(s)`);
    for (const i of g.items) {
      const price = i.price_buy ? `${i.price_buy.toLocaleString("en-US")}` : `sell ${i.price_sell?.toLocaleString("en-US")}`;
      lines.push(`  ${i.name.slice(0, 48).padEnd(48)} ${price.padStart(11)}  ${i.updated ?? "undated"}${i.stale ? " stale" : ""}`);
    }
    const more = (g.itemCount ?? g.items.length) - g.items.length;
    if (more > 0) lines.push(`  ... ${more} more (raise the limit)`);
  }
  return lines.join("\n");
}
