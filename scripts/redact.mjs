/**
 * What publishing removes from the parsed datasets. The local `data/` copy
 * stays complete; only the public dump is redacted.
 *
 * Two parts:
 *   - the string table keeps name-shaped keys only (redactStrings);
 *   - every other dataset loses its prose fields (redactProse). Records keep
 *     their `descriptionKey`, so the text is one string-table lookup away for
 *     anyone holding the game's own global.ini.
 */

export const STRINGS_FILE = "game-strings-english.json";

/**
 * Name-shaped keys only, matched on a segment boundary. A substring match is
 * not enough: dialogue keys like `PU_..._MyNamesSpecial` contain "name" and
 * would drag whole spoken lines into a public repo.
 */
const KEEP_KEY = /(^|_)(Names?|Titles?|DisplayName|ShortName|Nickname)([A-Z0-9_]|$)/;
/** Dialogue and narrative namespaces, plus anything self-declaring as prose. */
const DROP_KEY = /desc|tooltip|hint|subtitle|^Dlg_|^PU_|^DX[SM]/i;
/** A display name is never a paragraph — a long value is prose that slipped through. */
export const MAX_VALUE_LENGTH = 120;

export function redactStrings(dump) {
  const strings = dump.strings ?? {};
  const kept = {};
  for (const [key, value] of Object.entries(strings)) {
    if (!value || value.length > MAX_VALUE_LENGTH) continue;
    if (DROP_KEY.test(key) || !KEEP_KEY.test(key)) continue;
    kept[key] = value;
  }
  return {
    ...dump,
    _redacted: "Localisation limited to name keys; descriptive and dialogue text is not published.",
    strings: kept,
    summary: { ...(dump.summary ?? {}), published: Object.keys(kept).length },
  };
}

/** Prose fields per dataset, removed wherever they appear in it. */
export const PROSE_FIELDS = {
  "game-commodities.json": ["description"],
  "game-manufacturers.json": ["description"],
  "game-missions.json": ["description"],
  "game-refining.json": ["description", "rawDescription", "refinedDescription"],
  "game-ships.json": ["description"],
  "game-starmap.json": ["description"],
};

/** Remove `fields` from every object under `value`; returns a copy. */
function strip(value, fields) {
  if (Array.isArray(value)) return value.map((v) => strip(v, fields));
  if (!value || typeof value !== "object") return value;
  const out = {};
  for (const [key, v] of Object.entries(value)) {
    if (!fields.has(key)) out[key] = strip(v, fields);
  }
  return out;
}

/**
 * Short mission titles read as names and stay; a title past the string-table
 * name limit is briefing text and goes, with the record's own name standing in
 * for `name`, which copies the title.
 */
function stripLongTitles(dump) {
  if (!Array.isArray(dump.missions)) return { dump, dropped: 0 };
  let dropped = 0;
  const missions = dump.missions.map((m) => {
    if (typeof m.title !== "string" || m.title.length <= MAX_VALUE_LENGTH) return m;
    dropped += 1;
    const rest = { ...m };
    delete rest.title;
    const short = String(m.recordName ?? "").split(".").pop() || m.id;
    return m.name === m.title ? { ...rest, name: short } : rest;
  });
  return { dump: { ...dump, missions }, dropped };
}

/** A dataset as published: prose fields removed and listed in `_redacted`. */
export function redactProse(file, dump) {
  const fields = PROSE_FIELDS[file];
  if (!fields) return dump;
  let out = strip(dump, new Set(fields));
  const redacted = [...fields];
  if (file === "game-missions.json") {
    const result = stripLongTitles(out);
    out = result.dump;
    if (result.dropped > 0) redacted.push(`title over ${MAX_VALUE_LENGTH} characters`);
  }
  const { _source, _extracted, _build, ...rest } = out;
  return { _source, _extracted, _build, _redacted: redacted, ...rest };
}

/** Whatever publishing does to one dataset. */
export function redactDump(file, dump) {
  return file === STRINGS_FILE ? redactStrings(dump) : redactProse(file, dump);
}
