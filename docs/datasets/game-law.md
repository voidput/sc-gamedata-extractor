---
title: game-law (game-law.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-law (game-law.json)

The law system: every crime (`InfractionDefinition`) with its triggers and default penalty parameters, and every jurisdiction (`Jurisdiction`) with the crimes it enforces, per-jurisdiction overrides, base fine, prohibited goods, controlled-substance classes and impound rules. On LIVE 4.10.201.48085 it holds 43 crimes and 13 jurisdictions. The file is 23,912 bytes minified.

## Source

- DataCore, filters at `crates/sc-gamedata/src/domains/law.rs:236`:
  - `InfractionDefinition` under `**/lawsystem/infractiondefinitions/**` (folders `felonies`, `misdemeanours`, `special`).
  - `Jurisdiction` under `**/lawsystem/jurisdictions/**` (`uee`, `klescherprison`, `stanton/*`, `pyro/*`, `nyx/*`, `ellis/green`).
- String table for crime, jurisdiction and impound names.
- Code: `set_parameters` (law.rs:104), `parse_crime` (law.rs:148), `parse_jurisdiction` (law.rs:173), `Domain::parse` (law.rs:247).
- Derives from no other dataset. [game-starmap](game-starmap.md) loads the same jurisdiction records so that its `jurisdiction` names match `jurisdictions[].name`.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `crimes[]`, `jurisdictions[]`, `summary{crimes, jurisdictions}`. `_schema: "game-law@1"` follows `_build` (since tags 4.10.201.48085 / 5.0.202.35026).

### `crimes[]` (sorted by category, then name)

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `name` | string | Record short name (`Murder`) | 43 | record |
| `recordName` | string | `InfractionDefinition.<short>` | 43 | record |
| `displayName` | string? | Localised crime name; it can differ from `name` (`Murder` is displayed as `Homicide`) | 43/43 | `name` |
| `category` | string | Folder after `infractiondefinitions/` | `misdemeanours` 36, `felonies` 5, `special` 2 | record path |
| `triggers` | string[] | Game events that raise the crime (`KillActor`, `RamVehicle`, `State_Trespassing` …) | 27 distinct; empty on 19 crimes | `triggers` |
| `parameters` | object? | `defaultParameters` with every -1 ("inherit") and non-number dropped | see below | `defaultParameters` |

`parameters` keys seen (count of crimes): `lifetime` 40, `isFelony` 28, `coolOffTime` 15, `ignoreIfAgainstPartyMember` 11, `felonyMerits` 7, `earlyPaymentPeriod` 35, `escalatedPaymentFineMultiplier` 35, `graceAllowance` 3, `gracePeriod` 3, `hideCrimeInJournal` 3, `displayGraceTime` 1, `graceCooloffScale` 1, `hideCrimeNotification` 1, `removeTimeSeconds` 1. Flags (`isFelony`, `ignore*`, `hide*`) are stored as ints 1/0 and come out as booleans (law.rs:101); every other number is kept as stored. The module comment names `lifetime` "hours on record", `felonyMerits` "prison merits" and `escalatedPaymentFineMultiplier` "the late-payment fine multiplier" (law.rs:4). Units for `coolOffTime`, `gracePeriod`, `earlyPaymentPeriod` and `removeTimeSeconds` are not stated beyond the field names.

### `jurisdictions[]` (sorted by display name)

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `name` | string | Record short name (`ArcCorp`, `UEE`) | 13 | record |
| `recordName` | string | `Jurisdiction.<short>` | 13 | record |
| `displayName` | string? | Localised name (`Ungoverned` for `UnclaimedSpace`) | 13 | `name` |
| `parent` | string? | File stem of `parentJurisdiction`, lower case (`uee`) | 7 have one | `parentJurisdiction` |
| `respectsParentLaws` | bool? | Copied | | `respectsParentJurisdictionLaws` |
| `isPrison` | bool? | Copied; true only for `KlescherPrison` | | `isPrison` |
| `baseFine` | number? | Copied | 0, 125 or 126 (aUEC per the module doc's "base fine"; unit not stated) | `baseFine` |
| `maxStolenGoodsPossessionScu` | number? | Copied | 1 on all 13 (SCU) | `maxStolenGoodsPossessionSCU` |
| `crimes[]` | `{crime, overrides?}` | Crimes enforced here, `crime` = crime `name`; `overrides` like `parameters` (the only key overridden on LIVE is `escalatedPaymentFineMultiplier`) | UEE 33, Klescher 5, Stanton corps 4 each, 7 jurisdictions 0 | `infractions[]` |
| `prohibitedGoods` | string[] | Commodity file stems (`osoianhides`, `gaspingweevileggs`) | 0 to 9 | `prohibitedGoods[]` |
| `prohibitedResources` | string[] | `ResourceType` short names | 0 to 9 | `prohibitedResources[]` |
| `controlledSubstanceClasses[]` | `{commodities[], resources[], maxPossessionScu?}` | Controlled-substance classes with possession limit | 3 per jurisdiction; e.g. 0.125 SCU for the SLAM class | `controlledSubstanceClasses[]` |
| `impounding[]` | `{name?, trigger?, seconds?, fine?, ignoreIfAgainstPartyMember?}` | Impound rules; `seconds` is `impoundingTimeSeconds`, `fine` is `impoundingFineUEC` | UEE and Rough & Ready 3 each; e.g. `PadRamming` 3600 s, 50,000 UEC | `impoundingDefinitions[]` |

Example (real records; the Klescher `crimes` list is cut from 5 to 2 and its `controlledSubstanceClasses` from 3 identical entries to 1):

```json
{"crimes":[{"name":"Murder","recordName":"InfractionDefinition.Murder","displayName":"Homicide","category":"felonies",
  "triggers":["KillActor"],"parameters":{"isFelony":true,"lifetime":168,"felonyMerits":5000}}],
 "jurisdictions":[{"name":"KlescherPrison","recordName":"Jurisdiction.KlescherPrison","displayName":"Klescher Rehabilitation",
  "parent":"uee","respectsParentLaws":true,"isPrison":true,"baseFine":125,"maxStolenGoodsPossessionScu":1,
  "crimes":[{"crime":"PrisonSuicide"},{"crime":"BatteryFirstDegree","overrides":{"escalatedPaymentFineMultiplier":1280}}],
  "prohibitedGoods":[],"prohibitedResources":[],
  "controlledSubstanceClasses":[{"commodities":[],"resources":[],"maxPossessionScu":0.125}],"impounding":[]}]}
```

## Derived, estimated and curated values

- `category` comes from the folder name, not a record field.
- Dropping -1 is the domain's reading of "inherit" (law.rs:10). The effective value of a parameter in a jurisdiction (override, else crime default, else a parent's) is not computed.
- No estimates and no curated sources.

## Redaction

Nothing (no `PROSE_FIELDS` entry). Crime descriptions are not read at all.

## Consumers

- **starorg-codex** (`feat/dataset-reuse`) `crates/starorg-mcp/src/prebuilt.rs:166`: needs `crimes`, `jurisdictions`, `summary`; strips nothing.
- SCMCP, snares, MCsim, Bindscape and sc-telemetry do not read it.
- Contracts: `_build` equals the tag; `_schema` stem `game-law` once added ([consumer-datasets](../plans/specs/consumer-datasets.md)).

## Derivable data

Fields the records carry that the domain does not read (`~/git/sc-gamedata/extracted-data/libs/foundry/records/lawsystem`):

- `InfractionDefinition.description` (a string key, e.g. `@Infractions_Desc_001`): could be published as `descriptionKey` only, like the other datasets.
- `defaultParameters.graceWarnings[]`: string keys of the warnings shown before the crime applies, dropped because they are not numbers. Set on `VehicleTowing` (1), `FelonyTrespassing` (2: `@GraceWarnings_Trespassing1stDegree_001`, `_002`) and `MisdemeanorTrespassing` (2).
- `defaultParameters.graceAllowanceCooldown` and `pressChargesNotificationTime`: read, but -1 on every record in this build.
- `Jurisdiction.earlyPaymentPeriod` (-1 on all 13), `infractionSets[]` (empty on all 13), `logoPath`, `journalEntry` (e.g. `journalentry/pu/jurisdiction_uee.json`), `subsumptionJurisdiction` (empty on all 13).
- Impound `name` is read; the impound records carry no other unread fields.

Values computable by combining datasets:

- Effective law per jurisdiction: for each crime, override → crime default, plus the parent's crimes when `respectsParentLaws` is true and `parent` is set. Mark it as an estimate: the records give `respectsParentJurisdictionLaws`, but how the game merges the lists is not stated. Consumer: starorg-codex.
- Jurisdiction per location by joining [game-starmap](game-starmap.md) `jurisdiction` (101 locations, all 13 names match), and system per jurisdiction from the record folder (`stanton/`, `pyro/`, `nyx/`, `ellis/`).
- Prohibited and controlled goods resolved to commodity names by joining the file stems with [game-commodities](game-commodities.md), and `prohibitedResources` with resource types.
- Reverse link commodity → jurisdictions where it is prohibited or controlled, with the SCU limit. Consumer: trading/hauling views (starorg-codex, SCMCP).

Parse gaps:

- `parent` is a lower-case file stem (`uee`) while `jurisdictions[].name` is the record short name (`UEE`), so a join needs a case-insensitive match or the record name.
- Six crimes are defined but enforced by no jurisdiction: `AttemptedHomicide`, `BrandishingDeadlyWeapon`, `DischargeDeadlyWeapon`, `PossessionOfControlledSubstances_ClassC`, `PropertyDestruction`, `VehicleCollision`. Whether the game applies them some other way (for example through `infractionSets`) is not established.
- 19 crimes have no `triggers` (all controlled-substance and stolen-goods crimes, `EvadingArrest`, `Hacking` …); what raises them is not in these records.
- Seven jurisdictions (Citizens For Prosperity, Headhunters, Rough & Ready, XenoThreat, Ungoverned, People's Alliance, Green) enforce no crimes in their own record; Green and People's Alliance respect UEE law through `parent`.

Related specs: [dump-reuse](../plans/specs/dump-reuse.md).
