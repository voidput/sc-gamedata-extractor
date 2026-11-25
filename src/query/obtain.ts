/**
 * Where an item comes from, and where to look first.
 *
 * Joins the parsed datasets: game-item-tags (what each item is), game-loot
 * (tables, archetypes, pool filters, secondary choices), game-item-sources
 * (which areas spawn which containers with which table, fixed rack loadouts,
 * contract awards), game-blueprints, game-wikelo-trades and, when fetched,
 * uex-item-prices and uex-terminals (shop stock is server-side; UEX is
 * crowdsourced). shops.ts places and dates the shop rows.
 *
 * How a loot roll is modelled. A container rolls its table: the table picks an
 * archetype by weight, the archetype picks a primary entry by weight (a tag
 * selector such as Weapon/FPS/Stocked/LMG), then each secondary layer picks a
 * choice by weight (rarity Common 100 / Uncommon 10, or manufacturer Behring
 * 100 / Kastak 1). The item is in the pool when it carries every positive tag
 * of each pick (a tag counts when the item has it or a tag below it), none of
 * the negatives or the archetype's excluded tags, and passes the slot's pool
 * filter. `roll` is the product of those weight shares: the odds a roll lands
 * on a pool holding the item. `est.` divides by the pool's size, which assumes
 * an even pick inside the pool. The data does not say so; it is an estimate.
 *
 * Random loot is opt-in: only items tagged LootGeneration/CanGenerateAsLoot
 * roll. No loot record mentions the flag, so the check is in game code, but the
 * pattern is consistent: the unflagged items are NPC-only (`_lr01`), creature
 * weapons, contested-zone and event rewards and store skins. A
 * CannotGenerateAsLoot sub-tag says why an item is barred (PromotionalItem,
 * InGameReward/Wikelo, TwitchDrop, ...). Fixed and loose spawns ignore the flag.
 *
 * Not covered: which named locations use each area preset (object containers),
 * and how many spawn points of each kind an area has.
 */

import { locateShops, placeLabel, shopRowText, STALE_DAYS, type LocatedShop, type ShopPrice, type UexTerminal } from "./shops.js";
import type { ContractAward, ItemTags, LootArea, SpawnLoot } from "./types.js";

type Obj = Record<string, unknown>;

export interface LootData {
  tables: Obj[];
  archetypes: Obj[];
  filters?: Obj[];
  secondaryChoices?: Obj[];
}

export interface BlueprintLike {
  name: string;
  displayName?: string;
  produces?: string;
  rewardedBy?: { mission?: string; contract?: string; pool?: string; chance?: number }[];
}

export interface WikeloTradeLike {
  name: string;
  rewardInferred?: string;
}

export type { LocatedShop, ShopPrice, UexTerminal } from "./shops.js";

export interface Datasets {
  items: ItemTags[];
  loot?: LootData;
  areas?: LootArea[];
  contractAwards?: ContractAward[];
  blueprints?: BlueprintLike[];
  wikelo?: WikeloTradeLike[];
  shops?: ShopPrice[];
  /** UEX terminals, to place shop rows; rows keep their terminal name without them. */
  terminals?: UexTerminal[];
  /** When the shop rows were fetched (ISO or ms), for the stale flag. Default now. */
  shopsAsOf?: string | number;
}

// ---------------------------------------------------------------- tags

/** True when `tags` holds `wanted` or a tag below it. */
export function hasTag(tags: readonly string[], wanted: string): boolean {
  return tags.some((t) => t === wanted || t.startsWith(`${wanted}/`));
}

const CAN = "LootGeneration/CanGenerateAsLoot";
const CANNOT = "LootGeneration/CannotGenerateAsLoot";

export interface LootEligibility {
  /** false when barred, true when flagged lootable, undefined when unflagged. */
  canLoot?: boolean;
  /** The CannotGenerateAsLoot sub-tag, e.g. "PromotionalItem". */
  reason?: string;
  rarity?: string;
}

export function lootEligibility(tags: readonly string[]): LootEligibility {
  const barred = tags.find((t) => t === CANNOT || t.startsWith(`${CANNOT}/`));
  const rarity = tags.find((t) => t.startsWith("LootGeneration/LootRarity/"))?.split("/").pop();
  if (barred) return { canLoot: false, reason: barred.slice(CANNOT.length + 1) || undefined, rarity };
  return { canLoot: hasTag(tags, CAN) ? true : undefined, rarity };
}

function strings(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : [];
}

function numberOr(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

export interface Selector {
  name?: string;
  weight: number;
  positive: string[];
  negative: string[];
}

/** One weighted choice, normalised across v1 entries, V3 entries and secondary choices. */
export function entrySelector(entry: Obj): Selector {
  const v3 = (entry.selector as Obj | undefined)?.tags as Obj | undefined;
  const extra = entry.additionalTags as Obj | undefined;
  const positive = [
    ...(typeof entry.tag === "string" ? [entry.tag] : []),
    ...strings(v3?.positiveTags),
    ...strings(extra?.positiveTags),
  ];
  const negative = [...strings(v3?.negativeTags), ...strings(extra?.negativeTags)];
  return { name: typeof entry.name === "string" ? entry.name : undefined, weight: numberOr(entry.weight, 1), positive, negative };
}

/** An empty selector matches everything; that is how "Anything else" choices read. */
export function selects(selector: Pick<Selector, "positive" | "negative">, tags: readonly string[]): boolean {
  return selector.positive.every((t) => hasTag(tags, t)) && !selector.negative.some((t) => hasTag(tags, t));
}

// ---------------------------------------------------------------- loot model

/** A weighted roll among choices; the item must match the choice that comes up. */
export interface Layer {
  name?: string;
  choices: Selector[];
}

export interface Archetype {
  name: string;
  excluded: string[];
  primary: Selector[];
  layers: Layer[];
}

export function normaliseArchetype(raw: Obj, fallbackName = "inline"): Archetype {
  const primarySource = ((raw.primaryOrGroup as Obj | undefined)?.entries ?? raw.entries) as Obj[] | undefined;
  const secondarySource = (raw.secondaryOrGroups as Obj[] | undefined) ?? [];
  return {
    name: typeof raw.name === "string" ? raw.name : fallbackName,
    excluded: strings((raw.excludedTags as Obj | undefined)?.tags),
    primary: (primarySource ?? []).filter(Boolean).map(entrySelector),
    layers: secondarySource.filter(Boolean).map((g) => ({
      name: typeof g.groupName === "string" ? g.groupName : undefined,
      choices: ((g.entries as Obj[] | undefined) ?? []).filter(Boolean).map(entrySelector),
    })),
  };
}

/** Pool filter tree: cumulative parts must all pass; an additive part lets an item in on its own. */
export type PoolFilter = (tags: readonly string[]) => boolean;

export interface LootIndex {
  tables: Map<string, { name: string; version: string; slots: { weight: number; archetype?: Archetype }[] }>;
  filters: Map<string, PoolFilter>;
  /** Secondary choice record (single or multi layer) -> its layers. */
  choices: Map<string, Layer[]>;
}

function archetypeRef(slot: Obj): { name?: string; inline?: Obj } {
  const ref = slot.archetype;
  if (typeof ref === "string") return { name: ref };
  if (ref && typeof ref === "object") {
    const rec = (ref as Obj).lootArchetypeRecord;
    if (typeof rec === "string") return { name: rec };
    return { inline: ref as Obj };
  }
  return {};
}

function compileFilter(node: unknown, byName: Map<string, Obj>, depth = 0): PoolFilter {
  if (!node || typeof node !== "object" || depth > 16) return () => true;
  const obj = node as Obj;
  if (typeof obj.filterRecord === "string") {
    const target = byName.get(obj.filterRecord.toLowerCase());
    return compileFilter(target?.filter, byName, depth + 1);
  }
  if (obj.tagFilter && typeof obj.tagFilter === "object") {
    const tf = obj.tagFilter as Obj;
    const sel = { positive: strings(tf.positiveTags), negative: strings(tf.negativeTags) };
    return (tags) => selects(sel, tags);
  }
  if (Array.isArray(obj.filters)) {
    const parts = (obj.filters as Obj[]).filter(Boolean).map((f) => ({
      additive: f.mode === "Additive",
      test: compileFilter(f.filter, byName, depth + 1),
    }));
    const cumulative = parts.filter((p) => !p.additive);
    const additive = parts.filter((p) => p.additive);
    return (tags) => cumulative.every((p) => p.test(tags)) || additive.some((p) => p.test(tags));
  }
  return () => true;
}

export function buildLootIndex(loot: LootData): LootIndex {
  const byName = new Map<string, Archetype>();
  for (const raw of loot.archetypes) {
    const a = normaliseArchetype(raw);
    byName.set(a.name.toLowerCase(), a);
  }

  const tables = new Map<string, { name: string; version: string; slots: { weight: number; archetype?: Archetype }[] }>();
  for (const t of loot.tables) {
    const name = String(t.name);
    const slots = ((t.lootArchetypes as Obj[] | undefined) ?? []).filter(Boolean).map((slot) => {
      const { name: ref, inline } = archetypeRef(slot);
      const archetype = inline
        ? normaliseArchetype(inline, typeof slot.name === "string" ? `${name}:${slot.name}` : `${name}:inline`)
        : ref
          ? byName.get(ref.toLowerCase())
          : undefined;
      return { weight: numberOr(slot.weight, 1), archetype };
    });
    tables.set(name.toLowerCase(), { name, version: String(t.version ?? ""), slots });
  }

  const filterRecords = new Map<string, Obj>();
  for (const f of loot.filters ?? []) if (typeof f.name === "string") filterRecords.set(f.name.toLowerCase(), f);
  const filters = new Map<string, PoolFilter>();
  for (const [name, f] of filterRecords) filters.set(name, compileFilter(f.filter, filterRecords));

  const single = new Map<string, Layer>();
  for (const c of loot.secondaryChoices ?? []) {
    if (typeof c.name === "string" && Array.isArray(c.choices)) {
      single.set(c.name.toLowerCase(), { name: c.name, choices: (c.choices as Obj[]).filter(Boolean).map(entrySelector) });
    }
  }
  const choices = new Map<string, Layer[]>();
  for (const [name, layer] of single) choices.set(name, [layer]);
  for (const c of loot.secondaryChoices ?? []) {
    if (typeof c.name === "string" && Array.isArray(c.layers)) {
      const layers = strings(c.layers)
        .map((l) => single.get(l.toLowerCase()))
        .filter((l): l is Layer => l !== undefined);
      choices.set(c.name.toLowerCase(), layers);
    }
  }
  return { tables, filters, choices };
}

/** Odds a layer's roll comes up with a choice the item satisfies; 0 when none does. */
function layerShare(layer: Layer, tags: readonly string[]): number {
  const total = layer.choices.reduce((s, c) => s + c.weight, 0);
  if (total <= 0) return 1;
  return layer.choices.filter((c) => selects(c, tags)).reduce((s, c) => s + c.weight, 0) / total;
}

export interface RollPath {
  table: string;
  archetype: string;
  entry?: string;
  /** archetype x entry x every secondary layer, as odds per roll. */
  share: number;
  /** Lootable items the same path could produce, the item included. */
  poolSize?: number;
  /** share / poolSize: the item's odds per roll, assuming an even pick in the pool. */
  perRoll: number;
}

export interface RollContext {
  filter?: PoolFilter;
  extraLayers?: Layer[];
  /** Every item, for pool sizes. */
  pool?: readonly ItemTags[];
}

/** Every way one roll of a table can produce the item. */
export function tableRolls(index: LootIndex, tableName: string, item: ItemTags, ctx: RollContext = {}): RollPath[] {
  const table = index.tables.get(tableName.toLowerCase());
  if (!table) return [];
  if (lootEligibility(item.tags).canLoot !== true) return [];
  if (ctx.filter && !ctx.filter(item.tags)) return [];
  const total = table.slots.reduce((s, slot) => s + slot.weight, 0);
  if (total <= 0) return [];

  const paths: RollPath[] = [];
  for (const slot of table.slots) {
    const archetype = slot.archetype;
    if (!archetype || slot.weight <= 0) continue;
    if (archetype.excluded.some((t) => hasTag(item.tags, t))) continue;
    const layers = [...archetype.layers, ...(ctx.extraLayers ?? [])];
    let secondary = 1;
    for (const layer of layers) secondary *= layerShare(layer, item.tags);
    if (secondary === 0) continue;
    const primaryTotal = archetype.primary.reduce((s, e) => s + e.weight, 0);
    if (primaryTotal <= 0) continue;

    for (const entry of archetype.primary) {
      if (entry.weight <= 0 || !selects(entry, item.tags)) continue;
      const share = (slot.weight / total) * (entry.weight / primaryTotal) * secondary;
      let poolSize: number | undefined;
      if (ctx.pool) {
        // Same picks as the item: same entry, and in each layer a choice the item also satisfies.
        const itemChoices = layers.map((l) => l.choices.filter((c) => selects(c, item.tags)));
        poolSize = ctx.pool.filter(
          (c) =>
            lootEligibility(c.tags).canLoot === true &&
            selects(entry, c.tags) &&
            !archetype.excluded.some((t) => hasTag(c.tags, t)) &&
            (!ctx.filter || ctx.filter(c.tags)) &&
            itemChoices.every((choices, i) => layers[i].choices.length === 0 || choices.some((ch) => selects(ch, c.tags))),
        ).length;
      }
      paths.push({
        table: table.name,
        archetype: archetype.name,
        entry: entry.name,
        share,
        poolSize,
        perRoll: poolSize ? share / poolSize : share,
      });
    }
  }
  return paths;
}

// ---------------------------------------------------------------- spawns

export interface SpawnHit {
  area: string;
  config?: string;
  spawnTags: string[];
  /** The container, rack or loose item entity. */
  entity: string;
  entityName?: string;
  /** Share of this config's spawns that are this entity. */
  slotShare: number;
  kind: "loot" | "fixed" | "loose";
  table?: string;
  /** Item odds per roll of the container, summed over archetype paths. */
  perRoll?: number;
  maxResults?: number;
  /** Rough odds the item is in one spawned entity: slotShare x fill x (1-(1-perRoll)^rolls). */
  score: number;
  paths?: RollPath[];
}

function slotLayers(index: LootIndex, loot: SpawnLoot): Layer[] {
  return loot.secondaryChoices ? (index.choices.get(loot.secondaryChoices.toLowerCase()) ?? []) : [];
}

export function spawnHits(datasets: Datasets, index: LootIndex | undefined, item: ItemTags): SpawnHit[] {
  const hits: SpawnHit[] = [];
  const cls = item.className.toLowerCase();
  const lootable = lootEligibility(item.tags).canLoot === true;
  for (const area of datasets.areas ?? []) {
    for (const config of area.configs) {
      const total = config.slots.reduce((s, slot) => s + slot.weight, 0);
      if (total <= 0) continue;
      const fill = config.fillChance ?? 1;
      for (const slot of config.slots) {
        const slotShare = slot.weight / total;
        if (slotShare <= 0) continue;
        const base = { area: area.name, config: config.name, spawnTags: config.spawnTags, entity: slot.entity, entityName: slot.entityName, slotShare };
        if (slot.entity.toLowerCase() === cls) {
          hits.push({ ...base, kind: "loose", score: slotShare * fill });
          continue;
        }
        if (slot.contains?.some((c) => c.toLowerCase() === cls)) {
          hits.push({ ...base, kind: "fixed", score: slotShare * fill });
          continue;
        }
        if (!slot.loot || !index || !lootable) continue;
        const filter = slot.loot.poolFilter ? index.filters.get(slot.loot.poolFilter.toLowerCase()) : undefined;
        const paths = tableRolls(index, slot.loot.table, item, {
          filter,
          extraLayers: slotLayers(index, slot.loot),
          pool: datasets.items,
        });
        if (paths.length === 0) continue;
        const perRoll = Math.min(1, paths.reduce((s, p) => s + p.perRoll, 0));
        const rolls = Math.max(1, slot.loot.maxResults ?? 1);
        const chance = slot.loot.chance ?? 1;
        hits.push({
          ...base,
          kind: "loot",
          table: slot.loot.table,
          perRoll,
          maxResults: slot.loot.maxResults,
          // maxResults caps rolls; containers usually stop earlier when full, so this is an upper bound.
          score: slotShare * fill * chance * (1 - Math.pow(1 - perRoll, rolls)),
          paths,
        });
      }
    }
  }
  return hits.sort((a, b) => b.score - a.score);
}

export interface AreaSummary {
  area: string;
  /** Best single spawn in the area. */
  best: SpawnHit;
  /** Spawn kinds (configs) in the area that can produce the item. */
  configs: number;
}

/** Collapse spawn hits to one row per area, best spawn first. */
export function rankAreas(hits: readonly SpawnHit[]): AreaSummary[] {
  const byArea = new Map<string, SpawnHit[]>();
  for (const h of hits) byArea.set(h.area, [...(byArea.get(h.area) ?? []), h]);
  return [...byArea.entries()]
    .map(([area, list]) => ({
      area,
      best: list.reduce((a, b) => (b.score > a.score ? b : a)),
      configs: new Set(list.map((h) => h.config ?? "")).size,
    }))
    .sort((a, b) => b.best.score - a.best.score);
}

/** Loot tables that can roll the item but that no area spawn uses (actor corpses, events, legacy). */
export function otherTables(datasets: Datasets, index: LootIndex, item: ItemTags, used: Set<string>): RollPath[] {
  const out: RollPath[] = [];
  for (const table of index.tables.values()) {
    if (used.has(table.name.toLowerCase())) continue;
    const paths = tableRolls(index, table.name, item, { pool: datasets.items });
    if (paths.length > 0) {
      const perRoll = paths.reduce((s, p) => s + p.perRoll, 0);
      out.push({ ...paths[0], perRoll, share: paths.reduce((s, p) => s + p.share, 0) });
    }
  }
  return out.sort((a, b) => b.perRoll - a.perRoll);
}

// ---------------------------------------------------------------- lookup

/** Items matching a name or class name; exact matches win, then every word must appear. */
export function findItems(items: readonly ItemTags[], query: string): ItemTags[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const exact = items.filter((i) => i.name.toLowerCase() === q || i.className.toLowerCase() === q);
  if (exact.length > 0) return exact;
  const words = q.split(/\s+/);
  return items.filter((i) => {
    // The manufacturer tag carries the full name ("KastakArms"); the code is only "KSAR".
    const maker = i.tags.find((t) => t.startsWith("Manufacturer/"))?.slice("Manufacturer/".length) ?? "";
    const hay = `${i.name} ${i.className} ${i.manufacturer ?? ""} ${maker}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

const squash = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, "");

export interface ObtainReport {
  item: ItemTags;
  eligibility: LootEligibility;
  areas: AreaSummary[];
  spawns: SpawnHit[];
  /** Tables that roll the item but no area spawn references. */
  otherTables: RollPath[];
  blueprints: BlueprintLike[];
  contractAwards: ContractAward[];
  /** Wikelo trades whose name implies this reward, used when no award record matches. */
  wikeloInferred: WikeloTradeLike[];
  /** Shop rows for the item, placed and dated; only those at `at` when it is set. */
  shops: LocatedShop[];
  /** The place filter, when one was given. */
  at?: string;
  /** Shop rows for the item elsewhere, when `at` filtered some out. */
  shopsElsewhere?: number;
  /** With `at`: the cheapest buyable row elsewhere, so "not sold here" can say where it is. */
  cheapestElsewhere?: LocatedShop;
  /** Set when the item was looked up as ammo: the weapons that load it. */
  ammoFor?: { className: string; name: string }[];
}

export interface ObtainOptions {
  /** Keep only shops at this place (terminal, station, city, moon, planet or system). */
  at?: string;
  /** The weapons this item was looked up as ammo for; only labels the report. */
  ammoFor?: readonly ItemTags[];
}

export interface AmmoTarget {
  ammo: ItemTags;
  /** Matched weapons that load it, in match order. */
  weapons: ItemTags[];
}

/**
 * The magazines or canisters the matched weapons load (game-item-tags
 * `ammoItems`), each with the weapons that take it. Items that are not
 * weapons, or name no ammo, are skipped, so a name that matches a gun and its
 * own magazine still resolves to the magazine. Order follows the weapons'
 * `ammoItems`, the containers the weapon names first.
 */
export function ammoTargets(items: readonly ItemTags[], matches: readonly ItemTags[]): AmmoTarget[] {
  const byClass = new Map(items.map((i) => [i.className.toLowerCase(), i]));
  const out = new Map<string, AmmoTarget>();
  for (const weapon of matches) {
    for (const cls of weapon.ammoItems ?? []) {
      const ammo = byClass.get(cls.toLowerCase());
      if (!ammo) continue;
      const target = out.get(ammo.className) ?? { ammo, weapons: [] };
      if (!target.weapons.includes(weapon)) target.weapons.push(weapon);
      out.set(ammo.className, target);
    }
  }
  return [...out.values()];
}

/** "Cambio SRT" or "Cambio SRT (+2 variants)": the weapons a report is ammo for. */
export function weaponsLabel(weapons: readonly { name: string }[]): string {
  const names = [...new Set(weapons.map((w) => w.name))];
  return names.length > 1 ? `${names[0]} (+${names.length - 1} variant${names.length > 2 ? "s" : ""})` : (names[0] ?? "");
}

export function obtain(datasets: Datasets, item: ItemTags, index?: LootIndex, options: ObtainOptions = {}): ObtainReport {
  const lootIndex = index ?? (datasets.loot ? buildLootIndex(datasets.loot) : undefined);
  const cls = item.className.toLowerCase();
  const spawns = spawnHits(datasets, lootIndex, item);
  const used = new Set(
    (datasets.areas ?? []).flatMap((a) => a.configs.flatMap((c) => c.slots.map((s) => s.loot?.table.toLowerCase() ?? ""))),
  );
  const awards = (datasets.contractAwards ?? []).filter((a) => a.item.toLowerCase() === cls);
  const nameKey = squash(item.name);
  // UEX leaves item_uuid blank on some rows; fall back to the display name for those only.
  const rows = (datasets.shops ?? []).filter((p) =>
    p.item_uuid ? p.item_uuid === item.id : !item.isTemplate && p.item_name?.toLowerCase() === item.name.toLowerCase(),
  );
  const at = options.at?.trim() || undefined;
  const shops = locateShops(rows, { terminals: datasets.terminals, asOf: datasets.shopsAsOf, at });
  let cheapestElsewhere: LocatedShop | undefined;
  if (at && shops.length < rows.length) {
    const here = new Set(shops.map((s) => `${s.id_terminal ?? ""}|${s.terminal}`));
    cheapestElsewhere = locateShops(rows, { terminals: datasets.terminals, asOf: datasets.shopsAsOf })
      .filter((s) => s.price_buy && !here.has(`${s.id_terminal ?? ""}|${s.terminal}`))
      .sort((a, b) => (a.price_buy ?? 0) - (b.price_buy ?? 0))[0];
  }
  return {
    item,
    eligibility: lootEligibility(item.tags),
    areas: rankAreas(spawns),
    spawns,
    otherTables: lootIndex ? otherTables(datasets, lootIndex, item, used) : [],
    blueprints: (datasets.blueprints ?? []).filter((b) => b.produces?.toLowerCase() === cls),
    contractAwards: awards,
    wikeloInferred:
      awards.length > 0
        ? []
        : (datasets.wikelo ?? []).filter((t) => t.rewardInferred && nameKey.length > 3 && squash(t.rewardInferred) === nameKey),
    shops,
    ...(at ? { at, shopsElsewhere: rows.length - shops.length } : {}),
    ...(cheapestElsewhere ? { cheapestElsewhere } : {}),
    ...(options.ammoFor?.length ? { ammoFor: options.ammoFor.map((w) => ({ className: w.className, name: w.name })) } : {}),
  };
}

// ---------------------------------------------------------------- render

export const pct = (n: number): string =>
  n >= 0.1 ? `${(n * 100).toFixed(1)}%` : n >= 0.001 ? `${(n * 100).toFixed(2)}%` : n > 0 ? `${(n * 100).toPrecision(2)}%` : "0%";

/** "caves/loot_caves_occupied_rock_stanton" -> "caves: loot caves occupied rock stanton". */
export function areaLabel(name: string): string {
  return name.replace(/\//g, ": ").replace(/_/g, " ");
}

function tagLabel(tags: readonly string[]): string {
  const wanted = tags.filter((t) => /^(LootSize|LootRarity|LootContents)\//.test(t)).map((t) => t.split("/").slice(1).join("/"));
  return wanted.length > 0 ? wanted.join(" ") : tags.filter((t) => !/^(SpawnPoint|TrackInArea)/.test(t)).join(" ");
}

/** "; 110 shop row(s) elsewhere, cheapest 275 aUEC at Dumper's GrimHEX (...)", or "" when none. */
export function elsewhereText(report: Pick<ObtainReport, "shopsElsewhere" | "cheapestElsewhere">): string {
  if (!report.shopsElsewhere) return "";
  const c = report.cheapestElsewhere;
  const cheapest = c?.price_buy ? `, cheapest ${c.price_buy.toLocaleString("en-US")} aUEC at ${placeLabel(c)} (${c.updated ?? "undated"}${c.stale ? " stale" : ""})` : "";
  return `; ${report.shopsElsewhere} shop row(s) elsewhere${cheapest}`;
}

export function renderObtain(report: ObtainReport, options: { limit?: number; shopsLoaded?: boolean } = {}): string {
  const { item, eligibility } = report;
  const limit = options.limit ?? 15;
  const lines: string[] = [];
  const ammo = report.ammoFor?.length ? `ammo for ${weaponsLabel(report.ammoFor)}: ` : "";
  lines.push(`${ammo}${item.name}  (${item.className}${item.manufacturer ? `, ${item.manufacturer}` : ""}${item.isTemplate ? ", no display name" : ""})`);

  const loot =
    eligibility.canLoot === false
      ? `no, CannotGenerateAsLoot${eligibility.reason ? `/${eligibility.reason}` : ""}`
      : eligibility.canLoot
        ? "yes, CanGenerateAsLoot"
        : "no, not flagged CanGenerateAsLoot (random loot is opt-in)";
  lines.push(`  lootable   ${loot}${eligibility.rarity ? `, rarity ${eligibility.rarity}` : ""}`);

  if (report.areas.length > 0) {
    lines.push(`\n  best areas (${report.areas.length}${report.areas.length > limit ? `, top ${limit}` : ""}); odds per spawned box, rough:`);
    lines.push(`    ${"area".padEnd(52)} ${"spawn point".padEnd(26)} ${"box".padEnd(34)} ${"box%".padStart(6)} ${"per roll".padStart(9)} ${"odds".padStart(8)}`);
    for (const { best: h } of report.areas.slice(0, limit)) {
      const box = h.kind === "loot" ? (h.entityName ?? h.entity) : `${h.kind}: ${h.entityName ?? h.entity}`;
      lines.push(
        `    ${areaLabel(h.area).slice(0, 52).padEnd(52)} ${(h.config ?? tagLabel(h.spawnTags)).slice(0, 26).padEnd(26)} ${box.slice(0, 34).padEnd(34)} ${pct(h.slotShare).padStart(6)} ${(h.perRoll !== undefined ? pct(h.perRoll) : "fixed").padStart(9)} ${pct(h.score).padStart(8)}`,
      );
    }
    lines.push("    box% = share of that spawn point's spawns that are this box; per roll = item odds per loot roll;");
    lines.push("    odds = box% x up to maxResults rolls. Even pick inside a pool is assumed, not stated by the data.");
  } else if (eligibility.canLoot === true) {
    lines.push("  areas      no area spawn produces it");
  }

  if (report.otherTables.length > 0) {
    const names = report.otherTables.slice(0, 10).map((t) => `${t.table} (${pct(t.perRoll)})`);
    lines.push(`\n  other tables (corpses, events, legacy; not placed by an area preset): ${names.join(", ")}${report.otherTables.length > 10 ? ", ..." : ""}`);
  }

  lines.push("");
  if (report.blueprints.length > 0) {
    for (const b of report.blueprints) {
      lines.push(`  crafting   ${b.displayName ?? b.name}`);
      const missions = [...new Set((b.rewardedBy ?? []).map((r) => r.mission ?? r.contract).filter(Boolean))];
      if (missions.length > 0) lines.push(`             blueprint from missions: ${missions.slice(0, 8).join(", ")}${missions.length > 8 ? `, +${missions.length - 8}` : ""}`);
    }
  } else {
    lines.push("  crafting   no blueprint");
  }

  if (report.contractAwards.length > 0) {
    const byContract = [...new Set(report.contractAwards.map((a) => a.contract))];
    lines.push(`  contracts  awarded by ${byContract.join(", ")}`);
  } else if (report.wikeloInferred.length > 0) {
    lines.push(`  wikelo     ${report.wikeloInferred.map((t) => t.name).join(", ")} (reward inferred from contract name)`);
  } else {
    lines.push("  contracts  no mission or Wikelo award");
  }

  const at = report.at ? ` at "${report.at}"` : "";
  const buyable = report.shops.filter((s) => s.price_buy).sort((a, b) => (a.price_buy ?? 0) - (b.price_buy ?? 0));
  if (buyable.length > 0) {
    const shown = buyable.slice(0, Math.max(limit, 10));
    lines.push(`  shops${at}   (UEX, player-reported; date = last confirmed, stale = over ${STALE_DAYS} days)`);
    for (const s of shown) lines.push(`    ${shopRowText(s)}`);
    if (buyable.length > shown.length) lines.push(`    ... ${buyable.length - shown.length} more`);
  } else if (report.shops.length > 0) {
    lines.push(`  shops      listed${at}, no buy price: ${[...new Set(report.shops.map(placeLabel))].join("; ")} (UEX)`);
  } else if (options.shopsLoaded === false) {
    lines.push("  shops      no shop data (run: sc-gamedata fetch uex-item-prices)");
  } else if (report.at) {
    lines.push(`  shops      not sold${at} as far as UEX knows${elsewhereText(report)}`);
  } else {
    lines.push("  shops      not sold anywhere UEX knows of");
  }
  return lines.join("\n");
}
