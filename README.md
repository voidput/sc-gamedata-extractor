# sc-gamedata

Extract Star Citizen game data into structured JSON, straight from the shipped
game files.

The game ships its own database. The `sc-gamedata` binary (Rust, in
`crates/sc-gamedata`) links the [StarBreaker][sb] crates, reads the `Data.p4k`
archive and the DataCore records inside it directly, and writes one JSON
dataset per domain. No API, no scraping, no waiting
for a third party to update after patch day.

[sb]: https://github.com/diogotr7/StarBreaker

## Where the output goes

Parsed datasets are not committed here. `make publish` (`sc-gamedata publish`,
then the GitHub release) publishes a build to [sc-gamedata-dumps][dumps] — one commit per game build, tagged with the
build string, which is what makes patch-over-patch diffs possible.
[SCMCP][scmcp] reads that repo to answer questions like "what changed for the
Gladius between two patches".

```bash
make                # read Data.p4k into data/
make publish-dry    # what would be published, and how big
make publish        # commit, tag, push, then the GitHub release
```

`sc-gamedata publish` commits, tags and pushes; it never runs `gh`. `make
publish` has it write the release steps to `.tmp/gh-release-<CHANNEL>.sh` and
runs that script: `gh release edit` on a republish, then `gh release create`
unless the release already exists.

Each tag also gets a GitHub release titled with the patch and channel, such as
`4.10.2 LIVE (4.10.197.2878)`. The channel is recorded at extract time:
PTU, EPTU and TECH-PREVIEW builds become prereleases, HOTFIX builds are
releases that are never marked latest, and only LIVE is marked latest.
Re-running against a tag that has no release creates the release, and
`PUBLISH_CHANNEL=` (`--channel`) overrides the recorded channel. One channel
end to end: `make release-ptu` (`make ptu && make publish-dry && make
publish`).

Publishing redacts prose (`localization::redaction` in `crates/sc-gamedata/src/localization.rs`); the local `data/` copy stays
complete:

- `game-strings-english.json` keeps name-shaped keys only: no descriptive
  text, dialogue or mission prose.
- `description` is removed from commodities, manufacturers, missions, ships and
  starmap, and `description`, `rawDescription` and `refinedDescription` from
  refining. Mission titles over 120 characters go too (`name` then falls back
  to the record name); shorter titles stay, as they read as names.
- Each redacted dataset lists what it lost in `_redacted`. Records keep their
  string-table keys (`nameKey`, `descriptionKey`, and for missions `titleKey`
  and `giverKey`), so the text is one lookup away in the game's own
  `global.ini`. `nameStatus` (absent when real: `placeholder`, `unresolved` or
  `empty`) says when the game has no real name and `name` is the class name.

No game assets are ever published.

Each publish also writes patch notes for the build against the previous
published build on the same release line (same major version, by version
order, so `4.10.200` follows `4.10.197` whatever the tag dates, and a `1.0.x`
tag is never the base for a `4.x` build) to `notes/<build>.md` and `notes/<build>.json` in the same
commit, beside `data/`. They come from the `notes` command
below, with the newer side read from the redacted dumps exactly as published,
so the string counts compare like with like. The dry run lists both files and
their sizes. No earlier tag in the checkout and the notes are skipped with a warning rather than blocking the publish; so are notes
that fail to build. To add notes
to a build already out, `make republish` it. SCMCP serves them as
`sc_release_notes`.

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

### Patch notes

`sc-gamedata notes <from> <to>` (or `make notes FROM=4.10.197 TO=local
[NOTES=file.md]`) turns a diff into player-facing notes: new ships, ship and
personal weapon balance with before and after stats, armour sets, crafting,
missions, loot, renames, and counts for everything else. Variants collapse to
one line, templates, AI hulls and Vanduul (NPC-only) hulls and weapons are
only counted, and moves under `--min-change` (default 1%) are hidden. A stat
the old build had at 0 is shown without a buff/nerf verdict, and tracked
records that changed only in other fields (a loadout, a volume) are counted
under their section rather than dropped. Against a snapshot whose string table
came from the redacted published tag, the full local table is compared on the
published name keys only, and the notes say which datasets a fallback served. Markdown goes to stdout; `--json`
prints the structured document, and `--write <file.md|file.json>` saves either.

```bash
sc-gamedata notes 4.10.197.2878 local
sc-gamedata notes 4.10.197 local --json --min-change 2
```

### Every patch, kept locally

Each `parse` saves a gzipped copy of what it wrote to `snapshots/<build>/`
(about 1 MB per build), so every patch the tool has parsed is a diff target by
build number with no publish step and no git. `sc-gamedata snapshot` saves the
current `data/` by hand. Builds resolve in this order: `local`, a directory, a
local snapshot, then a dumps-repo tag. A stale dataset stamped with another
build is never copied into a snapshot, so partial re-parses cannot mix builds.

The routine per patch, LIVE or PTU:

```bash
make                                   # or: make ptu
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

- Rust (stable, edition 2024) and cargo; `make` builds the binary
- A Star Citizen install (any channel: LIVE, PTU, EPTU, HOTFIX)
- `gh` for `make publish` (the GitHub release), `fzf` for `make find`
- Node 20 or newer only for `make export` (`src/deflection.ts`, `src/engagement.ts`),
  the TypeScript tests and SCMCP's `src/query/`

## Usage

```bash
make                            # cargo build --release, then every dataset into data/
make ptu                        # the PTU install into data-ptu/
make DOMAINS="mining"           # just one domain
make help                       # every target and variable

sc-gamedata domains             # what can be built (./target/release/sc-gamedata)
sc-gamedata all mining          # what make DOMAINS=mining runs
sc-gamedata fetch               # API datasets, no game install needed
```

The Makefile runs `./target/release/sc-gamedata`; each target that needs it
depends on `make build` (`cargo build --release -p sc-gamedata`, a no-op when
current). `CHANNEL`, `P4K`, `SC_ROOT`, `EXTRACT_DIR`, `OUT` and `DOMAINS` map
onto the binary's flags:

| make variable | Flag | Env var | Default |
| --- | --- | --- | --- |
| `P4K` | `--p4k` | `SC_DATA_P4K` | `$(SC_ROOT)/$(CHANNEL)/Data.p4k` |
| `CHANNEL` | `--channel` | | `LIVE` |
| `EXTRACT_DIR` | `--extract-dir` | | `./extracted-data` (`-ptu` etc. per channel) |
| `OUT` | `--out` | | `./data` (`./data-ptu` etc. per channel) |

The binary reads the archive itself, so `extract`, `parse` and `all` are one
pass of about 20 seconds for every domain, the string table and the search
index (a `DOMAINS` run leaves the last two be). `EXTRACT_DIR` now only holds the
`_build.json` marker that publishing reads; there is no extracted record tree.

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

A domain is a Rust type implementing `Domain` (`crates/sc-gamedata/src/domain.rs`)
under `crates/sc-gamedata/src/domains/`: it declares which records it needs and how to
shape them.

```rust
impl Domain for Mining {
    fn name(&self) -> &'static str { "game-mining" }
    fn description(&self) -> &'static str { "Mineable element physics and mining laser/module modifiers." }
    fn filters(&self) -> &'static [&'static str] { &["**/mining/**"] }
    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let mut body = Map::new();
        field(&mut body, "mineableElements", &Vec::<Value>::new());
        Ok(body)
    }
}
```

Register it in `all_domains()` (`crates/sc-gamedata/src/bin/sc-gamedata.rs`) **after** its
output has been checked against a real extraction. A parser whose selectors silently match
nothing still writes a well-formed file full of nothing, which is worse than no file at
all. The pipeline warns when a domain yields an empty collection, but the warning is a
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
sc-gamedata fetch
sc-gamedata fetch uex-terminals   # public, works without a token
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
| `game-blueprints` | 1618 crafting recipes across 14 categories, 26 resources; 1586 named after the item they make, 687 with the missions that award them (title, contract, chance, pool weight); 160 reward pools |
| `game-wikelo-trades` | 53 Wikelo trades with full hand-in lists |
| `game-missions` | 2584 mission broker entries, 1978 of them live |
| `game-ships` | 1144 ships and ground vehicles with role, crew, dimensions, hull resistances, the stock loadout and its totals (DPS, shields, power, fuel, cargo, thrust) |
| `game-ammo` | 242 ammo records, 228 carrying damage. Join to weapons for DPS |
| `game-fps-weapons` | 446 personal ranged and melee weapons |
| `game-starmap` | 2132 locations with type, amenities, jurisdiction, and their chain from the system down (`path`, `system`); 444 carry an absolute `position` in metres (stars, planets, moons, Lagrange points, stations, jump points, asteroid bases), read from the solar system object containers, and each its `quantum` travel geometry (arrival, obstruction and adoption radii) |
| `game-reputation` | 380 ranks across 239 organisations |
| `game-containers` | 582 cargo grids and inventories with capacity |
| `game-mining-spawns` | 49 spawn locations (named and placed on the star map where it has them), 33 ores, 1014 spawn entries with each entry's share of its group; 268 rocks with RS scan signature, deposit tier, ship/vehicle/hand kind and composition with per-ore quality scale; `oreSpawns`, one row per ore per location |
| `game-weapons` | 237 ship weapons and 432 FPS weapons joined to their ammo, plus 67 missiles. The combat baseline |
| `game-commodities` | 207 resource types with group, density, refined form, volatility and description (193), and for 44 of them quality: the rarity tier, the 0-1000 distribution quality is rolled from, location overrides (Pyro, Nyx clusters) and the bands that snap a roll to the shown value; 135 commodity cargo items linked to their resource; cargo impact and explosion damage |
| `game-refining` | The 9 refining methods with the in-game yield, cost and speed ratings and descriptions, 30 raw-to-refined pairs with names, descriptions and densities, the 4 workload surcharge tiers, refining quality settings |
| `game-shops` | Global shop rules: sell-back percentages with 46 item-type overrides, supply and demand bands, auto-loading fees per box size |
| `game-cargo` | 82 NPC and mission cargo manifests in 10 categories (fill mode, capacity range, resources), freight loading timings |
| `game-salvage` | Structural salvage and repair beam constants |
| `game-quantum` | Global quantum travel rules: minimum altitude, atmosphere limit, link range |
| `game-fire` | Fire-hazard rules (ignition multipliers per damage type and source, oxygen range, spread distances, 900 K burning-room temperature, door threshold, damage to players) and the 490 ship items that can start a fire: ignite roll on death (chance, radius), thermal death explosion, fire share of misfires, overheat and misfire temperatures and the cooling law |
| `game-armor` | 2429 armour pieces (476 cores, 459 arms, 468 legs, 665 helmets, 208 undersuits, 153 backpacks) with damage resistances, temperature range, radiation, storage and EM signature; 1865 clothing items with temperature range |
| `game-loot` | 220 loot tables and 267 archetypes in both the original and V3 formats, with weights and tag paths, plus pool filters and holiday loot |
| `game-law` | 43 crimes with felony status, prison merits, record lifetime and fines; 13 jurisdictions with base fine, prohibited goods, controlled substance limits and impound rules |
| `game-consumables` | 231 medpens, drugs, food and drink with total health, hunger, thirst, blood drug level, gas and buff effects |
| `game-keybinds` | 50 action maps and 1104 actions with default keyboard, mouse, gamepad and joystick bindings, activation modes, and the 3 option trees (invert, curve, sensitivity groups), from `Data/Libs/Config/defaultProfile.xml` |
| `game-attributes` | 345 option rows (334 names): default value, console variable, platform and confirmation flag per game option, from `Data/Libs/Config/Profiles/default/attributes.xml` |

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
| `uex-terminals` | 826 terminals, shops and stations with their station or city, moon, planet and system. Public, no token needed |
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
make export
```

(TypeScript: `make export` runs `tsc` on `src/deflection.ts` and `src/engagement.ts`, then
reads `$(OUT)` only, `data/` by default) writes flat CSV tables, one Excel workbook and a `combat.json` bundle to
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
sc-gamedata engage gladius --target gladius
sc-gamedata engage --weapons "M7A Cannon*2,AD5B Ballistic Gatling*2" --target cutter
sc-gamedata engage gladius --target cutter --accel 0,4,8 --closing 150
sc-gamedata engage --weapons "P4-AR Rifle" --bands 0,25,50,100,200
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
sc-gamedata deflect perseus --size 3
```

lists the ship weapons whose per-projectile damage gets through a ship's stock
armour, ranked by hull DPS after deflection and multipliers. A type at or
below the armour's deflection value is treated as deflected. Distortion is
listed apart, since it drains shields and components rather than hull. Armour
is at full health; the game lowers deflection as armour health drops.

```sh
sc-gamedata diff 4.10.196.36804 local ships --item gladius --fields totals
sc-gamedata diff 4.10.196.36804 local ship-components --collection shields --item rhada
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

### Where to find an item

`sc-gamedata obtain <item>` (or `make obtain ITEM="..."`) answers "where do I
get this": the best loot areas and boxes, every loot table that can roll it,
blueprints and the missions that award them, mission and Wikelo item awards,
and shop prices. Search by display name, class name or manufacturer
(`obtain kastak lmg`); more than three matches lists them instead.

```
make parse DOMAINS="item-tags item-sources loot blueprints wikelo-trades"
sc-gamedata fetch uex-item-prices uex-terminals   # shop stock and places, public UEX endpoints
sc-gamedata obtain "Verdict LMG"
sc-gamedata obtain ksar_pistol_energy_01 --json --limit 30
sc-gamedata obtain "Cambio SRT Canister" --at grimhex   # make obtain ITEM=... AT=grimhex
sc-gamedata obtain "Cambio SRT" --ammo --at grimhex     # make obtain ITEM=... AMMO=1 AT=...
sc-gamedata where "FS-9 LMG"                      # make where ITEM="FS-9 LMG"
sc-gamedata shop "grim hex"                       # make shop AT="grim hex"
```

`--ammo` reports on what a weapon loads instead of the weapon itself:
`obtain "Cambio SRT" --ammo` answers for the Cambio SRT Canister, headed
`ammo for Cambio SRT: Cambio SRT Canister`. The link is `ammoItems` on each
weapon's row in `game-item-tags.json`, read from the weapon component's
`ammoContainerRecord` and `secondaryAmmoContainers` (the Cambio ignores its
internal battery and refills from a canister), then widened to every magazine
item with the same attach tag, the tag the weapon's port requires. So the empty
and filled canister both appear, and a P4-AR also lists the CQ7 magazine, which
carries the P4-AR's port tag. The Cambio-Lite canister has the multitool's tag
and never does. Where several ammo records match, the one you can buy comes
first. A magazine with no source at all (no shop, loot, blueprint or award, such
as a creature's copy of a player magazine) is left out when another one has a
source, and named in a closing note.

`where <item>` gives the same sources as one short table per item, easiest
first: buy (the cheapest shop, plus the cheapest confirmed in the last 30 days
when the cheapest is stale), then contract or Wikelo awards and crafting
blueprints with the missions that grant them, then the two best loot areas with
their box and rough odds. It takes `--at`, `--ammo` and `--json`.

Shop rows come from UEX, which players report; the game files hold no shop
stock. Each row shows its terminal and place (station or city, moon, planet,
system, joined from `uex-terminals` on `id_terminal`; without that file only the
terminal name), the day UEX last had it confirmed, and `stale` when that is over
30 days before the price file was fetched. `--at` keeps shops at one place.
Case, spaces and punctuation are ignored, and filler words such as `station` or
`shop` are dropped, so `grimhex`, `grim hex` and `grim hex station` match, as do
`Yela`, `Orison` or `Stanton`. Matching takes the best tier that hits any
terminal: an exact name (system, planet, moon, station, city, outpost or
terminal), then whole words or word starts, then, for 4+ letters, a substring.
So `pyro` is the Pyro system, not the Pyro Gateway on the Stanton side. A query
needs 3+ letters. With `--at`, an item sold only elsewhere says so and shows the
cheapest shop elsewhere.

`shop <place>` lists everything sold at the matching terminals, grouped by
terminal, named from item tags when the row's `item_uuid` is a game record, else
by UEX. It lists 25 items per terminal (`--limit n` or `--limit all`); past 15
terminals it lists the matching places with terminal and item counts instead
(`--all` lists every terminal). `--json` on both gives the date and the `stale`
flag per row.

`sc-gamedata find [query]` (or `make find`) is the fuzzy front end: fzf over
every item and ship, with the `obtain` report as a live preview and keys to act
on the highlighted entry (enter: full report · ctrl-j: JSON · ctrl-t: tags ·
ctrl-e / ctrl-d: engage / deflection for ships · ctrl-y: print the key and exit ·
tab: multi-select). Unnamed variants are listed by weapon family (`Verdict
(unnamed: ksar_lmg_energy_01)`), and player hulls rank above their AI and faction
copies. Without fzf, or with `--list` or `--print`, it prints ranked matches.
A word that matches nowhere may still be a typo of a word in the name, within
one edit for 4 to 6 letters and two for 7 or more (swapped letters count as
one), so `find cambino srt` finds the Cambio SRT. A typo hit ranks below any
correctly spelled contiguous match but above letters merely scattered through a
name, so `gallent` finds the Gallant first. `obtain` and `where` fall back to
the best fuzzy match when no item matches by words, and say which one they used.

Every `parse` that leaves `game-item-tags.json` and `game-ships.json` in the
output also rebuilds `game-search-index.json`: the find entries precomputed as
compact rows (about 0.9 MB for 14k entries, against 12 MB of inputs), with world
furniture and templates flagged so one file serves the default list and
`--all`. It is published with the other datasets, but `snapshot`, `diff` and
`notes` skip it (it only restates its two inputs, as positional rows; name it,
`diff a b search-index`, to compare it anyway), and a parse that cannot rebuild
it removes the stale copy. The format is documented in
`src/query/search-index.ts`; `loadSearchIndexEntries` gives back exactly what
`buildEntries` would.

The query code (`src/query/`: types, `obtain`, `where`, shop places and ages, fuzzy
ranking, the search index format) imports nothing from Node or the rest of the
repo. SCMCP vendors it verbatim for its `sc_find`, `sc_obtain_item`, `sc_where` and `sc_shop`
MCP tools (`npm run sync:gamedata` there), so keep it that way;
the tests vendor along with it.

Loot never names an item; it selects by tag, so the answer is a join across
four records:

```
object container spawn point   tags: LootSize/Large + LootRarity/Common + LootContents/Weapons
  -> slot preset (an area)     v3slotpreset_loot_military: for those tags, spawn
    -> slot                    Standard Container (weight 2), a rack, or a loose item
      -> LootConfig            table, pool filter, secondary choices, max results
        -> table -> archetype  weighted entries, each a tag selector
          -> item tags         game-item-tags, the item's side of the match
```

Two datasets carry it:

- `game-item-tags`: every item's record-level tags resolved to paths, and
  `ammoItems` on weapons (448 personal weapons in 4.10.200; ship weapons name no
  ammo container record, so they get none). Tags include
  `LootGeneration/CanGenerateAsLoot` or `CannotGenerateAsLoot/<reason>`
  (PromotionalItem, InGameReward/Wikelo, TwitchDrop, ...). Random loot is
  opt-in: only `CanGenerateAsLoot` items roll. No loot record mentions the flag,
  so the check is in game code, but the unflagged items are consistently
  NPC-only (`_lr01`), creature weapons, event or contested-zone rewards and
  store skins. Fixed rack and loose-item spawns ignore the flag.
- `game-item-sources`: the 167 slot presets ("areas") with each spawn point's
  tags and slots, fixed rack loadouts, and every `ItemAwardEntityClass` a
  contract hands out.

Odds are rough and labelled so. `per roll` multiplies the weight shares along
table -> archetype -> entry -> each secondary layer (rarity, faction, and in V3
areas manufacturer weights: Military weights Behring 100 and Kastak 1). It then
divides by the number of items in that pool, assuming an even pick, which the
data does not state. `odds` is the share of a spawn point's spawns that are
that box, times up to `maxResults` rolls; containers usually stop earlier when
full, so treat it as an upper bound and compare areas against each other rather
than reading it as a drop rate.

What it cannot say: which named locations use each area preset, and how many
spawn points of each kind an area has. Both live in the object containers.
Area names are the preset names, which are descriptive (`caves: loot caves
occupied rock stanton`, `distributioncentre multiconfig lobby high`).

### Not available from the game files

Refinery rates are **not** in DataCore, and this was checked exhaustively: no
field matching yield, rate, cost or processing time exists in any record. The
nine `RefiningProcess` records carry only speed and quality labels ("Normal",
"Wasteful") plus their UI strings. Refining is resolved server-side, which is why
community sites derive those numbers empirically. Mining lasers do carry
`extractionEfficiency`, but that governs extraction, not refining.

What a Wikelo trade *gives you* is not on the trade's ContractTemplate, which
stores only a percentage `rewardContribution`, so `rewardInferred` is read from
the record name and labelled as inferred. Item rewards are, however, on the
contract generator (`contracts/contractgenerator/thecollector`) as
`ItemAwardEntityClass` entries; `game-item-sources.contractAwards` lists them and
`obtain` prefers them over the inferred name. The hand-in lists are real, and
complete.

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
