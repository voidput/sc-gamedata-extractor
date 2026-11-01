#!/usr/bin/env node
/**
 * Flatten the combat datasets into CSV tables an infographic or spreadsheet
 * tool can import: one row per item, one column per stat, no nesting.
 *
 *   node scripts/export-infographic.mjs [--data ./data] [--out ./exports]
 *
 * Writes to <out>/<build>/. Reads the parsed datasets, so run `parse` first.
 * Ships are filtered to player hulls: AI, template, display and event variants
 * are dropped, and same-name variants collapse to the base record.
 */

import fs from "node:fs";
import path from "node:path";
import { deflectionCheck } from "../dist/deflection.js";
import { G, damageAt, envelope, falloffRange, rangeForChance, rangeTable, targetOf } from "../dist/engagement.js";

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const dataDir = path.resolve(flag("data", "data"));
const load = (file) => JSON.parse(fs.readFileSync(path.join(dataDir, file), "utf8"));

const ships = load("game-ships.json");
const weapons = load("game-weapons.json");
const components = load("game-ship-components.json");
const build = weapons._build ?? ships._build ?? "unknown";
const outDir = path.resolve(flag("out", "exports"), build);
fs.mkdirSync(outDir, { recursive: true });

// ---------- CSV ----------

function cell(value) {
  if (value === undefined || value === null) return "";
  if (typeof value === "number") return Number.isFinite(value) ? String(Math.round(value * 1000) / 1000) : "";
  if (typeof value === "boolean") return value ? "yes" : "no";
  const text = String(value);
  return /[",\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

/** Write rows using the column order given; columns empty in every row are dropped. */
function writeCsv(name, columns, rows) {
  const used = columns.filter(([, get]) => rows.some((r) => {
    const v = get(r);
    return v !== undefined && v !== null && v !== "";
  }));
  const lines = [used.map(([header]) => header).join(",")];
  for (const row of rows) lines.push(used.map(([, get]) => cell(get(row))).join(","));
  fs.writeFileSync(path.join(outDir, `${name}.csv`), lines.join("\n") + "\n");
  return { name, rows: rows.length, columns: used.length };
}

const sum = (o) => (o ? Object.values(o).reduce((a, b) => a + (typeof b === "number" ? b : 0), 0) : undefined);

// ---------- filters ----------

const NOT_PLAYER =
  /(_ai_|_ai$|_pu_|_ea_|template|_pir|_crim|unmanned|_test|_s42|_tutorial|_npc|_derelict|_wreck|_boarded|_drone|_hijacked|_qig|_fw_|_mission|_dummy|_prop|_showdown|_cinematic|_nointerior|_display|_temp|_indestructible|_fleetweek|_drug|_tier_\d|_piano|_swarm|_advocacy|_fw\d|_plat$)/i;

function playerShips() {
  const byName = new Map();
  for (const v of ships.vehicles) {
    if (v.isTemplate || !v.flight || NOT_PLAYER.test(v.recordName)) continue;
    const seen = byName.get(v.name);
    // Same name, several records: keep the base, which has the shortest record name.
    if (!seen || v.recordName.length < seen.recordName.length) byName.set(v.name, v);
  }
  return [...byName.values()].sort((a, b) => a.name.localeCompare(b.name));
}

const named = (list) => list.filter((x) => !x.isTemplate);
const playerShipList = playerShips();
const shipWeapons = named(weapons.shipWeapons);
const shortRecord = (r) => r.replace(/^EntityClassDefinition\./, "");

/**
 * Display names are not unique: capital turrets reuse player gun names at
 * other sizes, and low-poly or edition copies repeat a gun stat for stat. So
 * every weapon gets a `key` (name plus size) for humans and `record` for
 * scripts, and copies with an identical stat line point at the original
 * through `variant_of`. Match on `record`, or `key`, never on name alone.
 */
const signature = (w) => [w.name, w.size, w.alpha, w.dps, w.damageType, w.fireRate].join("|");
const firstBySignature = new Map();
for (const w of [...shipWeapons].sort((a, b) => a.recordName.length - b.recordName.length || a.recordName.localeCompare(b.recordName))) {
  if (!firstBySignature.has(signature(w))) firstBySignature.set(signature(w), w);
}
const variantOf = (w) => {
  const base = firstBySignature.get(signature(w));
  return base && base !== w ? shortRecord(base.recordName) : undefined;
};
const keyOf = (w) => `${w.name} S${w.size ?? "?"}`;
/** Hull damage one projectile carries, distortion excluded. */
const perHit = (w) => {
  const d = w.ammo?.damage;
  if (!d) return undefined;
  const total = Object.entries(d).filter(([t]) => t !== "distortion").reduce((a, [, v]) => a + v, 0);
  return total || undefined;
};
const pelletsOf = (w) => (w.modes.find((m) => m.dps === w.dps) ?? w.modes[0])?.pellets;

const written = [];

// ---------- ships ----------

written.push(
  writeCsv("ships", [
    ["name", (v) => v.name],
    ["record", (v) => v.recordName.replace(/^EntityClassDefinition\./, "")],
    ["manufacturer", (v) => v.manufacturer?.toUpperCase()],
    ["career", (v) => v.career],
    ["role", (v) => v.role],
    ["size", (v) => v.size],
    ["crew", (v) => v.crewSize],
    ["length_m", (v) => v.boundingBox?.y],
    ["width_m", (v) => v.boundingBox?.x],
    ["height_m", (v) => v.boundingBox?.z],
    ["scm_speed_ms", (v) => v.flight?.scmSpeed],
    ["max_speed_ms", (v) => v.flight?.maxSpeed],
    ["boost_forward_ms", (v) => v.flight?.boostSpeedForward],
    ["boost_backward_ms", (v) => v.flight?.boostSpeedBackward],
    ["pitch_dps", (v) => v.flight?.pitch],
    ["yaw_dps", (v) => v.flight?.yaw],
    ["roll_dps", (v) => v.flight?.roll],
    ["shield_hp", (v) => v.totals?.shieldHealth],
    ["shield_regen", (v) => v.totals?.shieldRegen],
    ["armor_hp", (v) => v.totals?.armorHealth],
    ["armor_deflect_physical", (v) => v.armor?.deflection?.physical],
    ["armor_deflect_energy", (v) => v.armor?.deflection?.energy],
    ["armor_mult_physical", (v) => v.armor?.damageMultiplier?.physical],
    ["armor_mult_energy", (v) => v.armor?.damageMultiplier?.energy],
    ["hull_damage_normalization", (v) => v.hull?.damageNormalization],
    ["stock_weapon_dps", (v) => v.totals?.weaponDps],
    ["stock_weapons", (v) => v.totals?.weapons],
    ["stock_missiles", (v) => v.totals?.missiles],
    ["stock_missile_damage", (v) => v.totals?.missileDamage],
    ["weapon_power_pips", (v) => v.powerPools?.WeaponGun],
    ["power_output", (v) => v.totals?.powerOutput],
    ["fuel_scu", (v) => v.totals?.fuel],
    ["quantum_fuel_scu", (v) => v.totals?.quantumFuel],
    ["cargo_scu", (v) => v.totals?.cargoScu],
    ["thrust_main_n", (v) => v.totals?.thrustMain],
    ["thrust_retro_n", (v) => v.totals?.thrustRetro],
    ["thrust_maneuver_n", (v) => v.totals?.thrustManeuver],
    ["flares", (v) => v.totals?.countermeasures?.flare],
    ["chaff", (v) => v.totals?.countermeasures?.chaff],
    ["cross_section_x", (v) => v.crossSection?.x],
    ["cross_section_y", (v) => v.crossSection?.y],
    ["cross_section_z", (v) => v.crossSection?.z],
  ], playerShipList),
);

// Stock loadout, one row per item kind per ship: good for "what does it ship with" charts.
const LOADOUT_KINDS = ["weapons", "missiles", "shields", "powerPlants", "coolers", "quantumDrives", "radars", "countermeasures"];
const loadoutRows = [];
for (const v of playerShipList) {
  for (const kind of LOADOUT_KINDS) {
    for (const item of v.loadout?.[kind] ?? []) {
      loadoutRows.push({ ship: v.name, kind, ...item });
    }
  }
}
written.push(
  writeCsv("ship_loadouts", [
    ["ship", (r) => r.ship],
    ["slot", (r) => r.kind],
    ["item", (r) => r.name ?? r.item],
    ["record", (r) => r.item],
    ["size", (r) => r.size],
    ["count", (r) => r.count ?? 1],
    ["mount_or_rack", (r) => r.mount ?? r.rack],
    ["dps_each", (r) => r.dps],
    ["damage_each", (r) => r.damage],
  ], loadoutRows),
);

// ---------- weapons ----------

const damageOf = (w, type) => w.ammo?.damage?.[type];
written.push(
  writeCsv("ship_weapons", [
    ["name", (w) => w.name],
    ["key", keyOf],
    ["record", (w) => shortRecord(w.recordName)],
    ["variant_of", variantOf],
    ["manufacturer", (w) => w.manufacturer],
    ["type", (w) => w.type],
    ["sub_type", (w) => w.subType],
    ["size", (w) => w.size],
    ["grade", (w) => w.grade],
    ["damage_type", (w) => w.damageType],
    ["dps", (w) => w.dps],
    ["sustained_dps", (w) => w.sustainedDps],
    ["alpha", (w) => w.alpha],
    ["fire_rate_rpm", (w) => w.fireRate],
    ["range_m", (w) => w.range],
    ["projectile_speed_ms", (w) => w.speed],
    ["spread", (w) => w.spread],
    ["penetration_m", (w) => w.penetration],
    ["per_hit_damage", perHit],
    ["pellets", pelletsOf],
    ["damage_physical", (w) => damageOf(w, "physical")],
    ["damage_energy", (w) => damageOf(w, "energy")],
    ["damage_distortion", (w) => damageOf(w, "distortion")],
    ["damage_thermal", (w) => damageOf(w, "thermal")],
    ["ammo_or_pool", (w) => w.ammoCapacity],
    ["capacitor_regen_shots_s", (w) => w.capacitor?.regenPerSecond],
    ["capacitor_regen_delay_s", (w) => w.capacitor?.regenCooldown],
    ["burst_shots", (w) => w.burst?.shots],
    ["burst_seconds", (w) => w.burst?.seconds],
    ["burst_damage", (w) => w.burst?.damage],
    ["burst_limit", (w) => w.burst?.limit],
    ["overheat_seconds", (w) => w.overheatSeconds],
    ["power_draw", (w) => w.power],
    ["em_signature", (w) => w.em],
    ["ir_signature", (w) => w.ir],
    ["health", (w) => w.health],
    ["mass_kg", (w) => w.mass],
    ["volume_microscu", (w) => w.volume],
  ], shipWeapons),
);

written.push(
  writeCsv("fps_weapons", [
    ["name", (w) => w.name],
    ["manufacturer", (w) => w.manufacturer],
    ["type", (w) => w.type],
    ["sub_type", (w) => w.subType],
    ["size", (w) => w.size],
    ["damage_type", (w) => w.damageType],
    ["dps", (w) => w.dps],
    ["alpha", (w) => w.alpha],
    ["fire_rate_rpm", (w) => w.fireRate],
    ["range_m", (w) => w.range],
    ["projectile_speed_ms", (w) => w.speed],
    ["magazine", (w) => w.ammoCapacity],
    ["burst_seconds", (w) => w.burst?.seconds],
    ["burst_damage", (w) => w.burst?.damage],
    ["mass_kg", (w) => w.mass],
  ], named(weapons.fpsWeapons)),
);

written.push(
  writeCsv("missiles", [
    ["name", (m) => m.name],
    ["manufacturer", (m) => m.manufacturer],
    ["size", (m) => m.size],
    ["damage_type", (m) => m.damageType],
    ["damage", (m) => m.damageTotal ?? sum(m.damage)],
    ["tracking", (m) => m.trackingSignal],
    ["lock_time_s", (m) => m.lockTime],
    ["lock_range_min_m", (m) => m.lockRangeMin],
    ["lock_range_max_m", (m) => m.lockRangeMax],
    ["lock_angle_deg", (m) => m.lockingAngle],
    ["speed_ms", (m) => m.speed],
    ["boost_s", (m) => m.boostDuration],
    ["lifetime_s", (m) => m.maxLifetime],
    ["explosion_radius_m", (m) => m.explosionRadius],
  ], named(weapons.missiles)),
);

// ---------- components ----------

const COMMON = [
  ["name", (c) => c.name],
  ["record", (c) => c.recordName.replace(/^EntityClassDefinition\./, "")],
  ["manufacturer", (c) => c.manufacturer],
  ["size", (c) => c.size],
  ["grade", (c) => c.grade],
  ["sub_type", (c) => c.subType],
  ["power_draw", (c) => c.power],
  ["em_signature", (c) => c.em],
  ["ir_signature", (c) => c.ir],
  ["health", (c) => c.health],
  ["mass_kg", (c) => c.mass],
  ["volume_microscu", (c) => c.volume],
  ["distortion_pool", (c) => c.distortion?.max],
  ["distortion_recover_s", (c) => c.distortion?.secondsToRecover],
  ["self_repair_s", (c) => c.durability?.selfRepair?.seconds],
  ["resist_physical", (c) => c.durability?.resistances?.physical],
  ["resist_energy", (c) => c.durability?.resistances?.energy],
];

const snake = (key) => key.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase();

function componentTable(category, file, extra = []) {
  const list = named(components[category] ?? []);
  const statKeys = [...new Set(list.flatMap((c) => Object.keys(c.stats ?? {})))];
  const stats = statKeys.map((k) => [snake(k), (c) => c.stats?.[k]]);
  written.push(writeCsv(file, [...COMMON, ...stats, ...extra], list));
}

const range = (type, side) => (c) => c.detail?.[side]?.[type]?.max;
componentTable("shields", "shields", [
  ["absorb_physical_max", range("physical", "absorption")],
  ["absorb_energy_max", range("energy", "absorption")],
  ["absorb_distortion_max", range("distortion", "absorption")],
  ["resist_physical_max", range("physical", "resistance")],
  ["resist_energy_max", range("energy", "resistance")],
  ["resist_distortion_max", range("distortion", "resistance")],
]);
componentTable("powerPlants", "power_plants");
componentTable("coolers", "coolers");
componentTable("quantumDrives", "quantum_drives");
componentTable("radars", "radars");
componentTable("armor", "armor", [
  ["deflect_physical", (c) => c.detail?.deflection?.physical],
  ["deflect_energy", (c) => c.detail?.deflection?.energy],
  ["mult_physical", (c) => c.detail?.damageMultiplier?.physical],
  ["mult_energy", (c) => c.detail?.damageMultiplier?.energy],
]);
componentTable("lifeSupport", "life_support");
componentTable("jumpDrives", "jump_drives");

// ---------- what damages what ----------

// Per player ship: how many named guns of each size get through its armour.
const sizes = [...new Set(shipWeapons.map((w) => w.size).filter((s) => s !== undefined))].sort((a, b) => a - b);
const guns = shipWeapons.filter((w) => w.type === "WeaponGun" && w.ammo?.damage);
const deflectionRows = playerShipList
  .filter((v) => v.armor)
  .map((v) => {
    const rows = deflectionCheck(v.armor, guns.filter((w) => !variantOf(w)));
    const bySize = {};
    for (const size of sizes) {
      const ofSize = rows.filter((r) => r.size === size);
      if (ofSize.length === 0) continue;
      bySize[size] = { through: ofSize.filter((r) => r.penetrates).length, total: ofSize.length };
    }
    const smallest = rows.filter((r) => r.penetrates).map((r) => r.size).sort((a, b) => a - b)[0];
    return { ship: v, bySize, smallest, through: rows.filter((r) => r.penetrates).length, total: rows.length };
  });
written.push(
  writeCsv("deflection_summary", [
    ["ship", (r) => r.ship.name],
    ["armor_hp", (r) => r.ship.armor.health],
    ["deflect_physical", (r) => r.ship.armor.deflection?.physical],
    ["deflect_energy", (r) => r.ship.armor.deflection?.energy],
    ["guns_through", (r) => r.through],
    ["guns_total", (r) => r.total],
    ["pct_through", (r) => (r.total ? (100 * r.through) / r.total : undefined)],
    ["smallest_size_through", (r) => r.smallest],
    ...sizes.map((s) => [`s${s}_through_pct`, (r) => (r.bySize[s] ? (100 * r.bySize[s].through) / r.bySize[s].total : undefined)]),
  ], deflectionRows),
);

// Weapon by ship: every distinct gun against every player ship's armour.
const matrixGuns = guns.filter((w) => !variantOf(w));
const armored = playerShipList.filter((v) => v.armor);
const matrixRows = [];
for (const v of armored) {
  for (const row of deflectionCheck(v.armor, matrixGuns)) matrixRows.push({ ship: v, row });
}
const byRecord = new Map(matrixGuns.map((w) => [w.recordName, w]));
written.push(
  writeCsv("deflection_matrix", [
    ["ship", (r) => r.ship.name],
    ["ship_record", (r) => shortRecord(r.ship.recordName)],
    ["weapon_key", (r) => keyOf(byRecord.get(r.row.recordName))],
    ["weapon_record", (r) => shortRecord(r.row.recordName)],
    ["weapon_size", (r) => r.row.size],
    ["penetrates", (r) => r.row.penetrates],
    ["deflected_types", (r) => r.row.deflected.join(" ")],
    ["applied_per_hit", (r) => r.row.appliedPerHit],
    ["applied_dps", (r) => r.row.appliedDps],
    ["listed_dps", (r) => r.row.dps],
  ], matrixRows),
);

// Penetration: how deep a round travels into a hull and what it can pierce.
written.push(
  writeCsv("weapon_penetration", [
    ["key", keyOf],
    ["record", (w) => shortRecord(w.recordName)],
    ["size", (w) => w.size],
    ["damage_type", (w) => w.damageType],
    ["per_hit_damage", perHit],
    ["penetration_depth_m", (w) => w.ammo?.penetration],
    ["penetration_radius_near_m", (w) => w.ammo?.penetrationRadius?.near],
    ["penetration_radius_far_m", (w) => w.ammo?.penetrationRadius?.far],
    ["pierce_max_thickness_m", (w) => w.ammo?.pierce?.maxThickness],
    ["pierce_damage_level1", (w) => w.ammo?.pierce?.level1],
    ["pierce_damage_level2", (w) => w.ammo?.pierce?.level2],
    ["pierce_damage_level3", (w) => w.ammo?.pierce?.level3],
  ], shipWeapons.filter((w) => w.type === "WeaponGun" && !variantOf(w))),
);

// Time of flight per distance band, for every distinct ship gun.
const TOF_BANDS = [500, 1000, 1500, 2000, 3000, 4000, 6000, 8000];
written.push(
  writeCsv("weapon_flight_time", [
    ["key", keyOf],
    ["record", (w) => shortRecord(w.recordName)],
    ["size", (w) => w.size],
    ["projectile_speed_ms", (w) => w.speed],
    ["max_range_m", (w) => w.range],
    ["flight_time_max_range_s", (w) => (w.range && w.speed ? w.range / w.speed : undefined)],
    ...TOF_BANDS.map((d) => [`flight_s_at_${d}m`, (w) => (w.speed && (w.range ?? 0) >= d ? d / w.speed : undefined)]),
  ], shipWeapons.filter((w) => w.type === "WeaponGun" && !variantOf(w))),
);

// Personal weapons: damage per round at distance, and where it bottoms out.
const FPS_BANDS = [0, 10, 25, 50, 75, 100, 150, 200, 300];
const perRound = (w, d) => {
  const hit = damageAt(w.ammo?.damage, w.ammo?.falloff, d);
  const total = Object.values(hit).reduce((a, b) => a + b, 0);
  return total || undefined;
};
written.push(
  writeCsv("fps_falloff", [
    ["name", (w) => w.name],
    ["record", (w) => shortRecord(w.recordName)],
    ["sub_type", (w) => w.subType],
    ["max_range_m", (w) => w.range],
    ["falloff_floor_at_m", (w) => falloffRange(w.ammo?.damage, w.ammo?.falloff)],
    ...FPS_BANDS.map((d) => [`damage_at_${d}m`, (w) => perRound(w, d)]),
  ], named(weapons.fpsWeapons).filter((w) => w.ammo?.damage)),
);

// Stock loadout of every player ship against every player ship: hull DPS and
// the ranges it holds out to. "What can my ship actually hurt, and from where."
const stockGuns = (v) =>
  (v.loadout?.weapons ?? []).flatMap((entry) => {
    const weapon = shipWeapons.find((w) => shortRecord(w.recordName) === entry.item);
    return weapon && weapon.type === "WeaponGun" ? [{ weapon, count: entry.count ?? 1, mount: entry.mount }] : [];
  });
const attackers = playerShipList.map((v) => ({ v, guns: stockGuns(v) })).filter((a) => a.guns.length > 0);
const engagementRows = [];
for (const { v: attacker, guns: loadout } of attackers) {
  const listed = loadout.reduce((sum, g) => sum + (g.weapon.dps ?? 0) * g.count, 0);
  for (const targetShip of armored) {
    const target = targetOf(targetShip);
    const env = envelope(loadout, target, { bands: [0] });
    const table = env.appliedDps > 0 ? rangeTable(loadout, target, [0, 6, 13], { accuracy: [0.75, 0.5] }) : [];
    engagementRows.push({ attacker, targetShip, env, listed, table });
  }
}
written.push(
  writeCsv("loadout_vs_ship", [
    ["attacker", (r) => r.attacker.name],
    ["target", (r) => r.targetShip.name],
    ["stock_listed_dps", (r) => r.listed],
    ["stock_hull_dps", (r) => r.env.appliedDps],
    ["pct_of_listed", (r) => (r.listed ? (100 * r.env.appliedDps) / r.listed : undefined)],
    ["guns_that_hurt", (r) => r.env.guns.filter((g) => g.penetrates).reduce((n, g) => n + g.count, 0)],
    ["guns_total", (r) => r.env.guns.reduce((n, g) => n + g.count, 0)],
    ["eff_range_50pct_steady_m", (r) => r.table[0]?.ranges["50"]],
    ["eff_range_75pct_steady_m", (r) => r.table[0]?.ranges["75"]],
    ["eff_range_50pct_6g_m", (r) => r.table[1]?.ranges["50"]],
    ["eff_range_75pct_6g_m", (r) => r.table[1]?.ranges["75"]],
    ["eff_range_50pct_13g_m", (r) => r.table[2]?.ranges["50"]],
    ["eff_range_75pct_13g_m", (r) => r.table[2]?.ranges["75"]],
    ["all_guns_range_m", (r) => r.env.allGunsRange],
    ["max_damaging_range_m", (r) => r.env.maxRange],
    ["target_armor_hp", (r) => r.targetShip.armor?.health],
    ["target_shield_hp", (r) => r.targetShip.totals?.shieldHealth],
    ["seconds_to_strip_armor", (r) => (r.env.appliedDps ? (r.targetShip.armor?.health ?? 0) / r.env.appliedDps : undefined)],
  ], engagementRows),
);

// Every distinct gun's effective range against reference targets: how far out
// a share of rounds still lands, steady and jinking.
const REFERENCE_TARGETS = ["Aegis Gladius", "Drake Cutlass Black", "Aegis Hammerhead", "Aegis Idris-M"];
const references = REFERENCE_TARGETS.map((name) => playerShipList.find((v) => v.name === name)).filter(Boolean);
const slug = (name) => name.replace(/^(Aegis|Drake|RSI|Anvil|Origin|Crusader)\s+/, "").toLowerCase().replace(/[^a-z0-9]+/g, "_");
const rangeColumns = references.flatMap((ship) => {
  const target = targetOf(ship);
  return [
    [`${slug(ship.name)}_50pct_steady_m`, (w) => rangeForChance(w, target, 0.5)],
    [`${slug(ship.name)}_50pct_6g_m`, (w) => rangeForChance(w, target, 0.5, { accel: 6 * G })],
    [`${slug(ship.name)}_90pct_6g_m`, (w) => rangeForChance(w, target, 0.9, { accel: 6 * G })],
    [`${slug(ship.name)}_50pct_13g_m`, (w) => rangeForChance(w, target, 0.5, { accel: 13 * G })],
    [`${slug(ship.name)}_90pct_13g_m`, (w) => rangeForChance(w, target, 0.9, { accel: 13 * G })],
  ];
});
written.push(
  writeCsv("weapon_effective_range", [
    ["key", keyOf],
    ["record", (w) => shortRecord(w.recordName)],
    ["size", (w) => w.size],
    ["projectile_speed_ms", (w) => w.speed],
    ["spread_deg", (w) => w.spread],
    ["max_range_m", (w) => w.range],
    ...rangeColumns,
  ], shipWeapons.filter((w) => w.type === "WeaponGun" && !variantOf(w) && w.speed)),
);

// One JSON bundle for web pages: weapons and targets with the inputs a page
// needs to redo the deflection maths itself, plus the precomputed matrix.
const bundle = {
  build,
  generated: new Date().toISOString(),
  note: "Every number is read from the game's DataCore records. Match weapons by record, not name.",
  weapons: shipWeapons.map((w) => ({
    key: keyOf(w),
    record: shortRecord(w.recordName),
    variantOf: variantOf(w),
    name: w.name,
    manufacturer: w.manufacturer,
    type: w.type,
    subType: w.subType,
    size: w.size,
    damageType: w.damageType,
    alpha: w.alpha,
    perHit: perHit(w),
    pellets: pelletsOf(w),
    damagePerProjectile: w.ammo?.damage,
    dps: w.dps,
    sustainedDps: w.sustainedDps,
    fireRate: w.fireRate,
    speed: w.speed,
    range: w.range,
    ammoOrPool: w.ammoCapacity,
    burst: w.burst,
    power: w.power,
    em: w.em,
  })),
  targets: armored.map((v) => ({
    name: v.name,
    record: shortRecord(v.recordName),
    role: v.role,
    size: v.size,
    shieldHp: v.totals?.shieldHealth,
    armorHp: v.armor.health,
    deflection: v.armor.deflection,
    damageMultiplier: v.armor.damageMultiplier,
  })),
  // ship record -> weapon record -> applied damage per hit; 0 means deflected.
  matrix: Object.fromEntries(
    armored.map((v) => [
      shortRecord(v.recordName),
      Object.fromEntries(
        matrixRows.filter((m) => m.ship === v).map((m) => [shortRecord(m.row.recordName), m.row.appliedPerHit]),
      ),
    ]),
  ),
};
fs.writeFileSync(path.join(outDir, "combat.json"), JSON.stringify(bundle));
written.push({ name: "combat.json", rows: bundle.weapons.length, columns: 0, json: true });

// ---------- readme ----------

const notes = `Star Citizen combat data export, build ${build}
Generated ${new Date().toISOString()} from game files by sc-gamedata.

Every value is read from the game's DataCore records. Derived columns:
- sustained_dps: energy guns cycle empty pool, regen delay, refill (pool only
  refills when not firing). Ballistics fire until overheat then lock out,
  assuming cooling runs while firing; magazine-limited guns keep their dps.
- burst_*: one trigger hold until pool, magazine or heat runs out.
- deflection_summary: a hit is deflected when its per-projectile damage of a
  type is at or below the armour's deflection for that type; full-health
  armour. Distortion is not counted as hull damage.
- range_m: projectile speed times lifetime.
- per_hit_damage: hull damage one projectile carries (distortion excluded).
  Deflection is tested against this, not alpha: a scattergun's alpha is
  per_hit_damage times pellets.
- key / record / variant_of: display names repeat across sizes (capital
  turrets reuse player gun names) and across cosmetic copies. Match on record,
  or key (name plus size). variant_of names the original a copy duplicates.
- deflection_matrix: every distinct gun against every player ship.
- weapon_penetration: how deep a round travels into a hull
  (penetration_depth_m) and the cone it does it in; pierce columns are the
  thickest geometry it crosses and the damage kept at each level.
- weapon_flight_time: seconds for a round to reach each distance. Ship ammo
  has no damage falloff in the game data: full damage to max range.
- fps_falloff: personal weapons' damage per round by distance, from the
  ammo's damage drop parameters, and where it reaches its floor.
- loadout_vs_ship: every player ship's stock guns against every player ship.
  Hull DPS after deflection and multipliers, the range all damaging guns
  still reach, and seconds to strip the armour HP at that DPS (shields
  ignored, armour at full health). Target acceleration is not in the game
  data. eff_range columns: farthest distance at which 75% or 50% of the
  point-blank hull DPS still lands, against the target at its SCM speed,
  holding course ("steady") or jinking at 6 g or 13 g.
- weapon_effective_range: each distinct gun against reference targets
  (light, medium, large, capital): farthest distance for 50% hits steady and
  at 6 g and 13 g, and 90% hits at 6 g and 13 g.
- Hit chance model: rounds aimed at the lead pip, scattered by spread (degrees)
  plus how far the target slides from the pip during flight time,
  0.5 * accel * t^2 capped by SCM speed * t. Chance = (target radius / miss
  radius)^2, target radius from its average projected bounding box.
  Acceleration is not in the game data (it needs hull mass), so steady and
  6 g and 13 g are shown rather than a single guessed figure.
  combat.json carries the same plus the inputs to redo it client side.

Units: speeds m/s, rotation degrees/s, mass kg, volume micro-SCU (1 SCU =
1,000,000), fuel and cargo SCU, thrust newtons, fire rate rounds/minute.

Not in the game data: hull HP and ship mass (vehicle XML), prices (see UEX).
Ships: player hulls only; AI, template, display and event variants dropped,
same-name variants collapsed to the base record.

Files:
${written.map((w) => (w.json ? `  ${w.name}  ${w.rows} weapons, targets and matrix` : `  ${w.name}.csv  ${w.rows} rows, ${w.columns} columns`)).join("\n")}
`;
fs.writeFileSync(path.join(outDir, "README.txt"), notes);

console.log(`wrote ${written.length} tables to ${outDir}`);
for (const w of written) console.log(`  ${w.name.padEnd(20)} ${String(w.rows).padStart(6)} ${w.json ? "weapons" : `rows  ${w.columns} cols`}`);
