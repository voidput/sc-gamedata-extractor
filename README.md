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

Localisation is redacted before publishing: name keys only, no descriptive text,
dialogue or mission prose. No game assets are ever published.

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
| `game-ship-components` | 427 weapons, 76 radars, 73 shields, 67 missiles, 63 quantum drives |
| `game-blueprints` | 1607 crafting recipes across 14 categories, 26 resources |
| `game-wikelo-trades` | 53 Wikelo trades with full hand-in lists |
| `game-missions` | 2584 mission broker entries, 1978 of them live |
| `game-ships` | 1101 ships and ground vehicles with role, crew and dimensions |
| `game-ammo` | 242 ammo records, 228 carrying damage. Join to weapons for DPS |
| `game-fps-weapons` | 446 personal ranged and melee weapons |
| `game-starmap` | 2068 locations with hierarchy, type and amenities |
| `game-reputation` | 380 ranks across 239 organisations |
| `game-containers` | 582 cargo grids and inventories with capacity |
| `game-mining-spawns` | 49 locations, 33 ores, 1014 spawn entries. Which ore is where |

From APIs:

| Dataset | Contents |
| --- | --- |
| `uex-terminals` | 826 terminals, shops and stations |
| `uex-commodities` | 205 commodity definitions |
| `uex-vehicle-prices` | 282 purchase and 336 rental listings, joined to ship names |

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
