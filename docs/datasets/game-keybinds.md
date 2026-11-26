---
title: game-keybinds (game-keybinds.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-keybinds (game-keybinds.json)

The game's default control bindings: every action map and action in `defaultProfile.xml`, with the default inputs per device (keyboard, mouse, gamepad, joystick), activation mode, labels resolved from the string table, the activation-mode presets, the modifier inputs, the action categories and the option trees (invert, curve, sensitivity groups). On LIVE 4.10.201.48085 it holds 50 action maps with 1104 actions. The file is 394,109 bytes minified.

## Source

- Not a DataCore record. The archive file `Data/Libs/Config/defaultProfile.xml` (binary CryXmlB), declared under `Domain::files` (`crates/sc-gamedata/src/domains/keybinds.rs:650`, glob `**/Libs/Config/defaultProfile.xml`).
- String table for `UILabel`/`UICategory` keys. `LabelResolver` (keybinds.rs:162) matches keys case-insensitively and tries the `,P` suffix (about 13,000 keys carry it). An unresolved key comes out as the raw `@token` and is counted.
- Code: `device_binding` (keybinds.rs:248), `parse_activation_modes` (keybinds.rs:274), `parse_modifiers` (keybinds.rs:290), `parse_categories` (keybinds.rs:304), `parse_action` (keybinds.rs:340), `parse_action_map` (keybinds.rs:448), `parse_option_trees` (keybinds.rs:488), `parse_default_profile` (keybinds.rs:571), `Domain::parse` (keybinds.rs:658).
- Derives from no other dataset.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `activationModes[]`, `modifiers[]`, `categories[]`, `optionTrees[]`, `actionmaps[]`, `summary{}`. `_schema: "game-keybinds@1"` follows `_build` (since tags 4.10.201.48085 / 5.0.202.35026).

`summary` (LIVE): `actionmaps` 50, `actions` 1104, `boundByDevice` {keyboard 519, mouse 133, gamepad 249, joystick 67} (actions with at least one default input), `unlabelledActions` 350 (no `UILabel`), `unresolvedLabels` 10, `unresolvedLabelKeys` (the 10 keys, e.g. `@ui_CCEVAZGT`, `@ui_CCFlightModes`, `@ui_CIFPSIncreaseMiningThrottle`).

### `activationModes[]` (18)

`{name, onPress, onHold, onRelease, multiTap, multiTapBlock, pressTriggerThreshold, releaseTriggerThreshold, releaseTriggerDelay, retriggerable}`: numbers as written, `null` when absent. Example: `{"name":"tap","onPress":0,"onHold":0,"onRelease":1,"multiTap":1,"multiTapBlock":1,"pressTriggerThreshold":-1,"releaseTriggerThreshold":0.25,"releaseTriggerDelay":0,"retriggerable":0}`. Units of the thresholds and delays are not stated in the file.

### `modifiers[]`

Input names that act as modifiers: `["gp_shoulderl","kb_lalt","lshift"]`.

### `categories[]` (9, nested)

`{name, action, label, labelKey, children[]}`, from `<ActionCategories>`. Example: `PlayerActions` (`@PIT_ACPlayerActions` → "Player") with child `Emotes`.

### `optionTrees[]` (3: keyboard, gamepad, joystick)

| Field | Type | Meaning | Source |
|---|---|---|---|
| `type` | string | Device kind | `optiontree@type` |
| `instances` | number? | 8 for joystick, `null` otherwise. Meaning not established beyond "instance" in a player's options (keybinds.rs:485) | `instances` |
| `sensitivityMin`, `sensitivityMax` | number? | 0.01 to 6.25 (keyboard/mouse), 0.01 to 2 (gamepad, joystick) | `UISensitivityMin/Max` |
| `groups[]` | object[] | Flattened `optiongroup`s in tree order: 27, 49 and 55 | `optiongroup` |
| `groups[].name`, `.parent` | string, string\|null | Group and enclosing group (`null` at the top) | `name` |
| `groups[].label`, `.labelKey` | string? | Resolved label and key | `UILabel` |
| `groups[].showInvert`, `.showCurve`, `.showSensitivity` | number? | UI flags (e.g. 0, -1) | `UIShowInvert/Curve/Sensitivity` |
| `groups[].invert`, `.exponent` | number? | Default invert and response exponent | `invert`, `exponent` |
| `groups[].curve` | `{in,out}[]`\|null | Non-linearity curve points; set on 6 groups | `nonlinearity_curve/point` |

### `actionmaps[]` (50, file order = UI order)

| Field | Type | Meaning | Source |
|---|---|---|---|
| `name` | string | Map name (`seat_general`, `spaceship_general`, `player`) | `name` |
| `version` | number? | Map version (1 to 27 on LIVE; `null` on 11 maps) | `version` |
| `label`, `labelKey` | string, string? | Resolved label; the name when unlabelled | `UILabel` (or the misspelt `UILable`) |
| `descriptionKey` | string? | Key only, no text | `UIDescription` |
| `category`, `categoryKey` | string? | Resolved `UICategory`; raw `@token` when unresolved (`@ui_CCSeatGeneral`) | `UICategory` |
| `actions[]` | object[] | See below | `<action>` |

### `actions[]`

Every key from `name` to `flags` is always written; a missing value is `null` (or `{}` for `flags`), not an absent key. Only `perDeviceActivation`, `perDeviceFlags` and `states` are left out when empty.

| Field | Type | Meaning | Count/range | Source |
|---|---|---|---|---|
| `name` | string | Action name, the key telemetry uses | 1104; 74 names occur in more than one map | `name` |
| `label`, `labelKey` | string, string? | Resolved `UILabel`, else the name | 754 with a key | `UILabel` |
| `descriptionKey` | string? | Key only | 675 set | `UIDescription` |
| `category`, `categoryKey` | string? | `UICategory` resolved, else the `Category` entry's label | 153 set | `UICategory` / `Category` |
| `activationMode` | string? | Preset name, always one of `activationModes[].name` | 754 set (press 334, tap 187, hold 79, delayed_press 64 …) | `activationMode` in any casing |
| `defaults` | `{keyboard, mouse, gamepad, joystick}` | Per device: `null` = device not applicable, `[]` = explicitly unbound, else input strings (`"u+lshift"`, `"button9"`, `"rotz"`) | | attribute, or child `<device input>` / `<inputdata>` (the child wins) |
| `optionGroup` | string? | `optionTrees[].groups[].name` it uses (45 distinct) | 76 set | `optionGroup` |
| `flags` | object | Numeric attributes when set: `onPress` 227, `onRelease` 151, `onHold` 88, `always` 38, `retriggerable` 35, `useAnalogCompare`/`analogCompareVal`/`analogCompareOp` 20, `holdTriggerDelay` 19, `noModifiers` 13, `holdRepeatDelay` 10, `pressTriggerThreshold` 1 | | same attributes |
| `perDeviceActivation` | object? | Device → activation mode, from device children | 20 | child `activationMode` |
| `perDeviceFlags` | object? | Device → numeric flags from device children | 10 | child attributes |
| `states[]` | `{name, label, labelKey}`? | Named states of toggle actions (`opened`/`closed`) | 27 | `<states><state>` |

Example (real):

```json
{"name":"v_emergency_exit","label":"Emergency Exit Seat","labelKey":"@ui_CIEmergencyExit",
 "descriptionKey":"@ui_CIEmergencyExitDesc","category":null,"categoryKey":null,"activationMode":"tap",
 "defaults":{"keyboard":["u+lshift"],"mouse":null,"gamepad":null,"joystick":[]},"optionGroup":null,"flags":{}}
```

## Derived, estimated and curated values

- `label` falls back to the action or map name when there is no `UILabel`, and keeps the raw `@token` when the key is missing from the string table (10 keys). `summary.unlabelledActions` counts the first case.
- The `null`/`[]` split in `defaults` is the domain's reading of an absent attribute versus `" "` (keybinds.rs:22).
- No estimates and no curated sources.

## Redaction

Nothing is removed from this file: `label`, `labelKey`, `defaults`, `optionGroup`, `optionTrees` and `version` stay unredacted (keybinds.rs:31), and descriptions were never included (keys only). The published string table drops every keybind `labelKey` (0 of 754 match the name-key rule), so `label` in this file is the only published label text.

## Consumers

- **Bindscape** (`scsticks/scripts/import_sc_keybinds.js`): requires `actionmaps` (`:20`); per map `name`, `label` (dev-only maps dropped, `:45`); per action `defaults[kind]` (`:53`), `label` (`:57`), `optionGroup` (`:60`); per option tree group `showInvert`, `showCurve`, `label`, `parent`, `invert`, `exponent` (`:70-81`).
- **sc-telemetry** (`sctelemetry/keybinds.py:43`): builds `{action name: label, activation_mode, actionmaps[], defaults}` from `actionmaps[].actions[]` (`name`, `label`, `activationMode`, `defaults`); `sctelemetry/gamedata_hub.py:35` fetches the file from the hub and checks `_build` against the tag (`:107`).
- **sc-gamedata**: not read by any other domain.
- Contracts ([consumer-datasets](../plans/specs/consumer-datasets.md)): never redact `label`, `labelKey`, `optionTrees` (`showInvert`, `showCurve`, `invert`, `exponent`, `parent`), `optionGroup`, `version`; keep `defaults` per device kind and `label` even when descriptions are redacted; publish it in every tag at `data/game-keybinds.json`; `_build` equals the tag; `_schema` stem `game-keybinds`. Action names used by sc-telemetry's `ListTriggerableActions` must keep matching `name` (section 5 of that spec).

## Derivable data

The source file is not in the extracted record trees (it is read straight from `Data.p4k`), so unread attributes cannot be listed from a raw copy here. From the code, the domain reads a fixed attribute list (`NUMERIC_FLAGS`, `STRING_FLAGS`, keybinds.rs:67-83); any other `<action>` attribute is dropped. Listing the attribute names the file uses would show what is left (not checked).

Values computable from this dataset alone:

- Reverse index input → actions per map and device. On LIVE, 71 (map, keyboard input) pairs are the default of two or more actions (e.g. `a` in `character_customizer` for `character_customizer_feature_down` and `character_customizer_dnamode_down`). Whether a pair is a real conflict depends on activation modes and map activation, which the file does not state, so any "conflict" flag is an estimate. Consumer: Bindscape.
- Action name → all maps it appears in (74 names occur in more than one map), which sc-telemetry builds itself today.
- Effective activation timing per action: join `activationMode` (and `perDeviceActivation`) with `activationModes[]` to give each action its `onPress`/`onHold`/thresholds inline.
- Action → option group → curve/invert defaults by joining `optionGroup` with `optionTrees[].groups[]`.

Combining with other datasets:

- [game-attributes](game-attributes.md): options such as `FlightThrustInvertToggle` or the `VJoy*` settings sit beside the invert/curve groups here. A join by name is not established by any key in either file; treat it as an estimate.

Parse gaps:

- 10 label keys are unresolved (`summary.unresolvedLabelKeys`): 4 map `category` values stay raw `@…` tokens (`@ui_CCEVAZGT`, `@ui_CCSeatGeneral`, `@ui_CCTurrets`, `@ui_CGLightControllerDesc`).
- 350 actions have no `UILabel`, so `label` is the action name.
- `activationMode` is unset on 350 actions; which mode the game then uses is not stated.

Related specs: [tool-data-sync](../plans/specs/tool-data-sync.md) (Bindscape step), [consumer-datasets](../plans/specs/consumer-datasets.md) (section 5).
