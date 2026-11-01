# sc-gamedata

Extract Star Citizen game data into structured JSON, straight from the shipped
game files.

The game ships its own database. [StarBreaker][sb] reads the `Data.p4k` archive
and the DataCore records inside it; this tool drives StarBreaker, parses the raw
records, and writes one JSON dataset per domain. No API, no scraping, no waiting
for a third party to update after patch day.

[sb]: https://github.com/diogotr7/StarBreaker

## Where the output goes

Parsed datasets are not committed here. `scripts/publish-dump.mjs` publishes a
build to [sc-gamedata-dumps][dumps] — one commit per game build, tagged with the
build string, which is what makes patch-over-patch diffs possible.
[SCMCP][scmcp] reads that repo to answer questions like "what changed for the
Gladius between two patches".

```bash
npm run all                         # extract + parse into data/
npm run publish-dump -- --dry-run   # what would be published, and how big
npm run publish-dump                # commit, tag, push
```

Each tag also gets a GitHub release titled with the patch and channel, such as
`4.10.2 LIVE (4.10.197.2878)`. The channel is recorded at extract time:
PTU, EPTU and TECH-PREVIEW builds become prereleases, HOTFIX builds are
releases that are never marked latest, and only LIVE is marked latest.
Re-running against a tag that has no release creates the release, and
`--channel` overrides the recorded channel. With make:
`make ptu && make publish-dry && make publish`.

Localisation is redacted before publishing: name keys only, no descriptive text,
dialogue or mission prose. No game assets are ever published.

## Comparing builds

Any two builds can be diffed locally, without SCMCP, as long as each is either
a directory of `game-*.json` or a tag in the dumps repo:

```bash
sc-gamedata builds                                   # local parse, snapshots, published tags
sc-gamedata diff 1.0.191.28374 local                 # every dataset, counts only
sc-gamedata diff 1.0.191.28374 local weapons         # one dataset, most-changed first
sc-gamedata diff 4.10 local weapons --item "M7A"     # every changed field of one gun
sc-gamedata diff 4.10 local weapons --fields dps,alpha,fireRate --detail
sc-gamedata diff ./data-old ./data --json            # two directories, machine output
```

`local` is the parse output (`--out`, default `./data`). A tag is read with
`git show <tag>:data/<file>` from the dumps checkout in `.publish-repo`, which is
cloned or fetched on demand (`builds --fetch` updates it), so the build about
to be published can be compared against the last one first. A tag prefix such
as `4.10` picks the newest matching build.

Records are matched by `recordName`, never by display name, which the game
renames between patches; a rename shows as a changed field, not as one item
removed and another added. Nested changes are reported on the flattened path,
so a damage change reads `ammo.damage.energy: 55 -> 60`. Derived `by*` indexes
and `summary` are skipped, since they repeat the collections they index.

### Every patch, kept locally

Each `parse` saves a gzipped copy of what it wrote to `snapshots/<build>/`
(about 1 MB per build), so every patch the tool has parsed is a diff target by
build number with no publish step and no git. `sc-gamedata snapshot` saves the
current `data/` by hand. Builds resolve in this order: `local`, a directory, a
local snapshot, then a dumps-repo tag. A stale dataset stamped with another
build is never copied into a snapshot, so partial re-parses cannot mix builds.

The routine per patch, LIVE or PTU:

```bash
sc-gamedata all                        # or: extract --channel PTU, then parse
sc-gamedata builds                     # the new build now sits beside the old
sc-gamedata changes 4.10.196 local     # buffs and nerfs, written as CSV and JSON
```

### Buffs and nerfs for charts

`changes <from> <to>` writes `exports/changes/<from>_to_<to>/`:

- `stat_changes.csv`: one row per item per stat that moved, with before, after,
  delta, percent and `buff`, `nerf` or `changed`. Direction comes from the stat:
  higher DPS, shield HP or speed is a buff; higher EM, power draw, spread or
  regen delay is a nerf.
- `item_changes.csv`: one row per added, removed or changed item, with counts
  of buffs and nerfs, and the old name when it was renamed.
- `changes.json`: both, for web pages.

It tracks the headline stats of ship and FPS weapons, missiles, player ships
(flight, shields, armour, stock DPS, fuel, cargo, thrust) and the main
component categories. `diff` stays the tool for every field of every record;
`changes` is the curated, chart-shaped view. Feed the folder to
`scripts/csv-to-xlsx.py` for a workbook.

The build each dataset is stamped with is recorded at extract time in
`extracted-data/_build.json`. The installed game updates on its own schedule,
so a parse run after a launcher update would otherwise stamp old records with a
new build string, and the dumps repo refuses to publish a set that disagrees
about its build.

[dumps]: https://github.com/voidput/sc-gamedata-dumps
[scmcp]: https://github.com/voidput/SCMCP

## Requirements

- Node 20 or newer
- A Star Citizen install (any channel: LIVE, PTU, EPTU, HOTFIX)
- The [StarBreaker CLI][sb] on your PATH, or pointed at with `--starbreaker`

## Usage

```bash
npm install && npm run build

node dist/cli.js domains        # what can be built
node dist/cli.js all            # extract, then parse
node dist/cli.js all mining     # just one domain
node dist/cli.js parse          # re-parse without touching the archive
node dist/cli.js fetch          # API datasets, no game install needed
```

Both the game install and StarBreaker are auto-detected where possible. Override
either explicitly:

```bash
node dist/cli.js all \
  --p4k "/path/to/StarCitizen/LIVE/Data.p4k" \
  --starbreaker /path/to/starbreaker \
  --out ./data
```

| Option | Env var | Default |
| --- | --- | --- |
| `--p4k` | `SC_DATA_P4K` | auto-detected |
| `--starbreaker` | `STARBREAKER_BIN` | found on PATH |
| `--channel` | | `LIVE` |
| `--extract-dir` | | `./extracted-data` |
| `--out` | | `./data` |

Extraction and parsing are separate on purpose. Extraction is the slow half, and
each domain pulls only the record globs it declares, so a full run of the mining
domain takes seconds rather than unpacking a 145 GB archive. Once records are on
disk, `parse` re-runs against them freely.

## Output

Every dataset carries its provenance and the build it came from:

```json
{
  "_source": "Star Citizen game files via StarBreaker",
  "_extracted": "2026-09-05T00:00:00.000Z",
  "_build": "1.0.191.28374",
  "mineableElements": [
    {
      "name": "Raw_Quantainium",
      "recordName": "MineableElement.Quantainium_Raw",
      "instability": 1000,
      "resistance": 0.95,
      "explosionMultiplier": 260
    }
  ]
}
```

Because `_build` is recorded, keeping datasets in git gives you a patch-over-patch
diff for free.

## Adding a domain

A domain declares which records it needs and how to shape them:

```ts
export const miningDomain: Domain = {
  name: "game-mining",
  description: "Mineable element physics and mining laser/module modifiers.",
  filters: ["**/mining/**"],
  parse(records) {
    return { mineableElements: [] };
  },
};
```

Register it in `src/pipeline.ts` **after** its output has been checked against a
real extraction. A parser whose selectors silently match nothing still writes a
well-formed file full of nothing, which is worse than no file at all. The
pipeline warns when a domain yields an empty collection, but the warning is a
backstop, not a substitute for looking.

`filters` scope a domain twice: they select records out of the archive, and they
select which of the loaded records `parse` receives. That second part matters,
because extraction pools every registered domain's records together. Without
scoping, adding a domain silently widens what all the others see — registering
the FPS weapons domain put 252 personal weapons into the ship component counts
before this was fixed.

Two things about DataCore that cost real time to discover:

- **Extraction globs match the full record path, and `*` does not cross a `/`.**
  `*mineable*` silently matches zero records. `**/mineableelements/**` works.
- **Numeric modifiers are wrapped, not bare.** A stat reads
  `{"_Type_": "FloatModifierMultiplicative", "value": -10.0}`, and the wrapper
  type decides whether the value is a percentage or a flat offset. Reading these
  as plain numbers yields nothing.
- **A record's directory does not guarantee its kind.** Under
  `scitemmanufacturer`, over 900 of 1161 records are paint logos that merely
  carry their maker's name key. Reading the directory wholesale gives ten times
  too many manufacturers, most of them duplicate names.
- **The game's own data is sometimes wrong.** `FSKI` (FireStorm Kinetics)
  carries Aegis's name key. Such rows are reported as `sharedNames` rather than
  silently corrected.
- **What a record *is* comes from its component structs, not its path.** A ship
  item is categorised by which params struct it carries, because a glob for
  "shield" also matches a seat blast shield that has no shield generator.
- **Weapon damage is on the ammunition, not the weapon.** A weapon record has
  fire rate and heat; the damage lives at `AmmoParams.projectileParams.damage`.
  The `DeathExplosionParams.damage` on a weapon is what happens when the weapon
  itself is destroyed, and reads zero almost everywhere.
- **Some references resolve to a UUID rather than a name.** Every
  `StarMapObjectType` is named after its own id; the readable names sit in one
  `StarMapObjectTypes` record holding a `types` array, which has to be loaded and
  used as a lookup.
- **Not every number is a measurement.** A landing zone's container reads
  999x999x999, a stand-in for unbounded that computes to half a billion SCU.

## Localization

Records store display text as keys, not text: a manufacturer's name is
`@manufacturer_NameSASU`, resolving to "Sakura Sun". A domain that sets
`needsLocalization` makes the pipeline also pull
`Data/Localization/<language>/global.ini` (about 90,000 strings) and hands it to
`parse`. Unresolved keys pass through as their `@token` so gaps stay visible
rather than turning into empty strings.

The game ships exactly one string table per language and no other string file.
`sc-gamedata languages` lists the twelve; `--language` selects one.

### Inspecting strings

```bash
sc-gamedata strings --key @manufacturer_NameAEGS   # what does this token mean
sc-gamedata strings Wikelo                         # what key produces this text
sc-gamedata strings Stanton1 --keys-only
sc-gamedata strings --write                        # dump the table as a dataset
```

Search works in both directions, which matters because the table is the only
place some information exists in readable form. Location keys are themselves
string keys, so `Stanton1` resolves to "Hurston" and `Stanton1c` to "Magda". The
in-game Mining Compendium also lives here as one long string, and its ore lists
independently corroborate `game-mining-spawns`.

### Community string packs

Many players replace `global.ini` with a community pack that rewrites text for
readability. A pack installs as a loose `Data/Localization/<language>/global.ini`
which the game reads in preference to the archived table, so a tool reading only
the shipped strings describes a game the player is not seeing.

A pack is just `key=value` lines, so **any** pack works with no special support:

```bash
sc-gamedata strings --installed              # whatever is installed in the game
sc-gamedata strings --overlay ./mypack       # a file or directory
sc-gamedata strings --overlay a.ini,./b      # several, later ones win
sc-gamedata strings --pack starstrings       # a registered pack
sc-gamedata strings --installed --diff       # what the pack changes
```

`--installed` is usually what you want, since it reflects what the player is
actually reading, whichever pack it came from.

#### Registering a pack

The registry only saves you remembering a URL. `sc-gamedata overlays` lists what
is known and `overlays fetch <name>` downloads one. Add your own to
`overlay-sources.json`:

```json
[
  {
    "name": "mypack",
    "description": "My string tweaks.",
    "homepage": "https://example.com/mypack",
    "files": ["https://example.com/mypack/strings.ini"],
    "perPatch": true
  }
]
```

Entries are validated: names must be a single safe path segment, and URLs must be
HTTPS. A user entry replaces a built-in of the same name, so a pack can be
repointed at a fork or a pinned tag without editing code. `perPatch` defaults to
true and only controls whether the tool reminds you to refetch after an update.

[StarStrings](https://github.com/MrKraken/StarStrings) ships as a built-in entry
because it publishes per-category files under `src/For_Tool_Creators` in exactly
this format. Nothing is written into the game directory; packs are fetched to
`overlays/` and applied only when asked for.

## API datasets

Some things the game files cannot answer. The shipped data says what a commodity
*is*; only a live market says what it currently sells for and where. Those
datasets come from the [UEX Corp API](https://uexcorp.space) and need no game
install:

```bash
export UEXTOKEN=...        # register an app at uexcorp.space
node dist/cli.js fetch
node dist/cli.js fetch uex-terminals
```

UEX answers HTTP 200 with a `status` field even when a request fails, so the
body is checked rather than the status code alone. A bad token otherwise looks
like success.

## Status

From game files, verified against a real extraction:

| Dataset | Contents |
| --- | --- |
| `game-mining` | 46 mineable elements, 27 mining modules |
| `game-manufacturers` | 126 manufacturer codes with resolved names |
| `game-ship-components` | 73 shields, 63 quantum drives, 89 power plants, 84 coolers, 79 radars, 220 armour sets, 213 fuel tanks, 134 cargo grids, 169 missile racks, 194 countermeasures, 1433 thrusters and more, every one with power, EM/IR, HP, mass and durability |
| `game-blueprints` | 1607 crafting recipes across 14 categories, 26 resources |
| `game-wikelo-trades` | 53 Wikelo trades with full hand-in lists |
| `game-missions` | 2584 mission broker entries, 1978 of them live |
| `game-ships` | 1144 ships and ground vehicles with role, crew, dimensions, hull resistances, the stock loadout and its totals (DPS, shields, power, fuel, cargo, thrust) |
| `game-ammo` | 242 ammo records, 228 carrying damage. Join to weapons for DPS |
| `game-fps-weapons` | 446 personal ranged and melee weapons |
| `game-starmap` | 2068 locations with hierarchy, type and amenities |
| `game-reputation` | 380 ranks across 239 organisations |
| `game-containers` | 582 cargo grids and inventories with capacity |
| `game-mining-spawns` | 49 locations, 33 ores, 1014 spawn entries. Which ore is where |
| `game-weapons` | 237 ship weapons and 432 FPS weapons joined to their ammo, plus 67 missiles. The combat baseline |
| `game-commodities` | 207 resource types with group, density, refined form and volatility; 135 commodity cargo items linked to their resource; cargo impact and explosion damage |
| `game-refining` | The 9 refining methods with the in-game yield, cost and speed ratings and descriptions, 30 raw-to-refined pairs with densities, the 4 workload surcharge tiers, refining quality settings |
| `game-shops` | Global shop rules: sell-back percentages with 46 item-type overrides, supply and demand bands, auto-loading fees per box size |
| `game-cargo` | 82 NPC and mission cargo manifests in 10 categories (fill mode, capacity range, resources), freight loading timings |
| `game-salvage` | Structural salvage and repair beam constants |
| `game-quantum` | Global quantum travel rules: minimum altitude, atmosphere limit, link range |
| `game-armor` | 2429 armour pieces (476 cores, 459 arms, 468 legs, 665 helmets, 208 undersuits, 153 backpacks) with damage resistances, temperature range, radiation, storage and EM signature; 1865 clothing items with temperature range |
| `game-loot` | 220 loot tables and 267 archetypes in both the original and V3 formats, with weights and tag paths, plus pool filters and holiday loot |
| `game-law` | 43 crimes with felony status, prison merits, record lifetime and fines; 13 jurisdictions with base fine, prohibited goods, controlled substance limits and impound rules |
| `game-consumables` | 231 medpens, drugs, food and drink with total health, hunger, thirst, blood drug level, gas and buff effects |

Refinery numbers are not in the game files. Every DataCore record and every
text and config file in `Data.p4k` was searched: the client ships the method
names, their ratings as the text the kiosk shows ("High Yield // Low Cost //
Slowest"), the raw-to-refined pairs, and the workload tiers as surcharge text.
Yield percentages, costs, durations, station bonuses and capacities are worked
out by the server. `uex-refinery` has the player-reported values; its method
ratings disagree with the game's text for three methods.

From APIs:

| Dataset | Contents |
| --- | --- |
| `uex-terminals` | 826 terminals, shops and stations |
| `uex-commodities` | 205 commodity definitions |
| `uex-vehicle-prices` | 282 purchase and 336 rental listings, joined to ship names |
| `uex-refinery` | 215 station and commodity yield bonuses, 20 capacities, 9 methods with yield, cost and speed ratings, audited jobs. Public, no token needed |

### Combat baseline

`game-weapons` is the dataset to diff when a patch touches balance. Each weapon
carries the columns a loadout site shows, flattened so a change reads as one
line: `dps`, `alpha`, `fireRate`, `range`, `speed`, `spread`, `penetration`,
`power`, `em`, `ammoCapacity`, `health`, plus the ammo it fires and every
selectable fire mode with its own cadence and DPS. Missiles carry explosion
damage, lock time and range, tracking signal and speed.

None of that sits in one record, which is why the join is its own dataset:

- Damage is on the ammo. A ship gun embeds an `SAmmoContainerComponentParams`
  that points at it; an FPS gun names a magazine in its default loadout, and
  the magazine points at the ammo. Shotgun alpha is per-projectile damage times
  `pelletCount`; the `ammo` block stays per projectile.
- Fire rate is on the fire action. Most ship guns wrap it in a
  `SWeaponActionSequenceParams` whose entries carry a `delay` in RPM. That delay
  is the cadence the gun runs, not the nested action's `fireRate`: a 900 RPM
  sequence over two 500 RPM barrels fires at 900. Entries in seconds (a
  sniper's bolt cycle) are averaged into the same figure.
- Range is `speed * lifetime` of the projectile. Power draw and EM come from
  `ItemResourceComponentParams`, HP from `SHealthComponentParams`, penetration
  from `projectileParams.penetrationParams.basePenetrationDistance`.

### Ship and component baseline

`game-ship-components` is the same idea for everything else that slots into a
ship. Every component, whatever its category, carries the shared columns
`power` drawn, `em`, `ir`, `health`, `mass` and `distortionMax`, plus a
`durability` block: damage multipliers by type, self-repair count and time,
field-repair restore ratio, wear conversion rate and misfire window. Category
stats sit in `stats` as one flat numeric map (shield pool and regen, quantum
drive speed and spool time, power plant output, cooler output, thrust, fuel and
cargo capacity, missile rack slots and size) and tables that do not flatten to
one number sit in `detail` (shield resistance and absorption by damage type,
armour damage multipliers and deflection, radar detection per signature).

Category is decided by which stats struct the record carries, falling back to
`AttachDef.Type` for items whose only stats live on the shared resource struct
(power plants, coolers, life support). A countermeasure launcher carries the
weapon struct too, so it is sorted before guns. Cargo capacity is resolved
through the inventory container the grid names; one SCU is a 1.25 m cube.

`game-ships` carries each vehicle's stock loadout, read from
`SEntityComponentDefaultLoadoutParams` and sorted by what each item is rather
than what its port is called, since port names vary by ship. Guns inside
gimbals and turrets are listed with the mount they sit in, missiles with their
rack, identical items in sibling ports collapsed into a `count`. `totals` sums
it into the figures a patch is most likely to move: stock DPS, missile count
and damage, shield pool and regen, power output, hydrogen and quantum fuel,
cargo SCU, main, retro and manoeuvring thrust, and decoys by kind. `hull`
carries the damage normalisation value and the hull's damage multipliers;
hull HP and mass live in the vehicle XML, which DataCore does not hold, so
they are not invented here.

Each vehicle also carries:

- `flight`: SCM, max (NAV) and boost speeds, pitch, yaw and roll rates, boost
  capacitor and G stun threshold, read off the stock flight controller's
  `IFCSParams`. Use `maxAngularVelocity`; the `speedProfile` block beside it is
  a 60/200/60 placeholder on most hulls.
- `armor`: the stock armour's health, per-type deflection thresholds, hull
  damage multipliers and signature multipliers. `totals.armorHealth` repeats
  the health for diffing.
- `powerPools`: power pips per item type from the ship's resource network.

Weapons carry three more figures for comparing guns beyond listed DPS:

- `capacitor` (energy guns): shots in the pool, shots regained per second,
  and the delay before regen starts. `ammoCapacity` is the pool size.
- `burst`: shots, seconds and damage of one trigger hold, and what ends it
  (capacitor, magazine or heat).
- `sustainedDps`: damage per second over repeated cycles. For energy guns a
  cycle is empty, cooldown, refill, since the pool only refills when not
  firing. For ballistics it is fire until overheat, then the overheat lockout,
  assuming cooling runs while firing; magazine-limited guns keep their DPS.

Components and weapons carry `volume` in micro-SCU and `distortion`: the pool
size, decay delay and rate, recovery ratio, and seconds from a full pool to
restart.

### Export for charts and sites

```sh
npm run export
```

writes flat CSV tables, one Excel workbook and a `combat.json` bundle to
`exports/<build>/`: ships, stock loadouts, ship and FPS weapons, missiles,
each component category, a per-ship deflection summary and a full weapon by
ship deflection matrix. Weapons carry `record` and `key` (name plus size)
because display names repeat: capital turrets reuse player gun names at
larger sizes, and low-poly copies repeat a gun stat for stat (`variant_of`).
Scripts built on the export should match on `record`. Deflection is tested
against `per_hit_damage`, not `alpha`; a scattergun's alpha is per-hit damage
times pellets. A README.txt in each export lists units and derived columns.

### Engagements and effective range

```sh
node dist/cli.js engage gladius --target gladius
node dist/cli.js engage --weapons "M7A Cannon*2,AD5B Ballistic Gatling*2" --target cutter
node dist/cli.js engage gladius --target cutter --accel 0,4,8 --closing 150
node dist/cli.js engage --weapons "P4-AR Rifle" --bands 0,25,50,100,200
```

takes a ship's stock guns, or any guns by record, `name S<size>` or name with
`*count`, against a target ship, and answers "out to what range do I land X%
of my damage on a ship moving at its speed".

- Hull DPS first: deflection and hull multipliers decide what each round does
  if it hits. Ship ammo has no damage falloff in the files, so a hit does full
  damage out to `speed * lifetime`. Personal-weapon ammo does fall off
  (`damageDropParams`) and that is applied.
- Then hit chance. Rounds inherit the shooter's velocity (every vehicle ammo
  record sets `inheritVelocity`), so only relative motion matters. Rounds are
  aimed at the lead pip and scatter by the weapon's spread cone (degrees). A
  target that jinks the moment you fire slides `0.5 * a * t^2` off the pip
  during the flight time `t = d / (projectileSpeed + closingSpeed)`, capped by
  its SCM speed times `t`. Hit chance is the target's silhouette as a share of
  that miss disc, `(r / R)^2`, with `r` the radius of the target's average
  projected bounding-box area.
- Effective range is the farthest distance at which a share of the point-blank
  hull DPS still lands, reported for 90, 75, 50 and 25% (`--accuracy`).

Target acceleration is the one input not in DataCore: it needs hull mass,
which lives in the vehicle XML. So it is not guessed. The distance table uses
the first `--accel` value (default 0 g, a target holding course, where spread
alone limits hits) and the summary tabulates effective range at every value
given (default 0, 3, 6, 9 and 13 g). `--target-speed`, `--closing`, `--reaction`,
`--hit-radius` and `--spread min|max` adjust the rest.

Penetration depth (`ammo.penetration`, from `basePenetrationDistance`) is how
far a round travels into the hull to reach internal components, with the cone
radius at entry and exit (`penetrationRadius`) and what thin geometry it can
pierce (`pierce`). It is separate from armour deflection: deflection decides
whether a hit does hull damage at all, penetration decides how deep it reaches.

The export carries the same as tables: `weapon_penetration`,
`weapon_flight_time`, `fps_falloff`, and `loadout_vs_ship`, every player ship's
stock guns against every player ship, with hull DPS, the share of listed DPS
that lands, effective ranges steady, at 6 g and at 13 g, and seconds to strip the
armour; and `weapon_effective_range`, every distinct gun's 50% and 90% hit
ranges against a light, medium, large and capital reference target.

### Deflection check

```sh
node dist/cli.js deflect perseus --size 3
```

lists the ship weapons whose per-projectile damage gets through a ship's stock
armour, ranked by hull DPS after deflection and multipliers. A type at or
below the armour's deflection value is treated as deflected. Distortion is
listed apart, since it drains shields and components rather than hull. Armour
is at full health; the game lowers deflection as armour health drops.

```sh
node dist/cli.js diff 4.10.196.36804 local ships --item gladius --fields totals
node dist/cli.js diff 4.10.196.36804 local ship-components --collection shields --item rhada
```

Checked against a loadout site for every S5 ship gun it lists: DPS, alpha,
penetration, rate, range, speed, spread, power, EM, ammo and HP all match.
Burst modes report both the in-burst `fireRate` and a `sustainedFireRate` that
includes the cooldown, and DPS uses the sustained one. Beams carry damage per
second and their falloff ranges. Tractor, salvage, mining and healing actions
are listed as `other` so a weapon's mode list stays complete, but carry no DPS.
Store display props (`*_prop`) share a real gun's name and fire nothing; they
are flagged `isTemplate`.

### Ore locations

`game-mining-spawns` answers "what can I mine at Hurston" and its inverse. No
single record connects a place to an ore, so it walks four hops:

```
HarvestableProviderPreset  HPP_Stanton1, one per body or belt
  -> HarvestableElement      a spawn entry with a relative probability
    -> HarvestablePreset     Mining_Common_Iron
      -> EntityClassDefinition  MineableRock_SurfaceCommon_Iron
        -> MineableComposition  CommonShipMineables_Iron
          -> MineableElement    iron_ore at 34.3-84.3%
```

Provider presets are named with internal body keys (`Stanton1`, `Pyro5c`). The
key-to-name mapping is not in the data, so a small table supplies the common ones
and the raw key is always kept beside the label rather than guessed at.

### Not available from the game files

Refinery rates are **not** in DataCore, and this was checked exhaustively: no
field matching yield, rate, cost or processing time exists in any record. The
nine `RefiningProcess` records carry only speed and quality labels ("Normal",
"Wasteful") plus their UI strings. Refining is resolved server-side, which is why
community sites derive those numbers empirically. Mining lasers do carry
`extractionEfficiency`, but that governs extraction, not refining.

What a Wikelo trade *gives you* is likewise absent. The contract stores a
percentage `rewardContribution` and nothing else, so `rewardInferred` is read
from the record name and labelled as inferred rather than presented as fact. The
hand-in lists are real, and complete.

## Credits

The extraction approach follows the pipeline in
[dumpers_repo](https://github.com/Sinedrone-Sentinel/dumpers_repo)
(Copyright Michael Linzenmeyer, Apache-2.0), which established the
StarBreaker-extract-then-parse design and the domain breakdown. See
[NOTICE](NOTICE).

All game data belongs to Cloud Imperium Games. This tool ships no game content;
it reads files you already have from your own installation.

## Licence

Apache-2.0. See [LICENSE](LICENSE).
