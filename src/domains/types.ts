import type { Localization } from "../localization.js";
import type { GameRecord } from "../records.js";

/** What a domain gets to work with. */
export interface ParseContext {
  records: GameRecord[];
  /** The game's string table. Empty if global.ini was not extracted. */
  localization: Localization;
}

/**
 * A domain turns a set of DataCore records into one output dataset.
 *
 * `records` are DataCore extraction globs, matched against the full record path
 * where a single star does not cross a slash, so directory-spanning patterns
 * need a double star.
 *
 * `needsLocalization` makes the pipeline extract global.ini, which is required
 * by any domain surfacing display names.
 */
export interface Domain {
  /** Output file base name: "game-mining" -> game-mining.json. */
  name: string;
  description: string;
  filters: string[];
  needsLocalization?: boolean;
  parse(context: ParseContext): Record<string, unknown>;
}

/**
 * A dataset fetched from an API rather than extracted from game files.
 *
 * These need no StarBreaker run and no game install; they need network access
 * and, for UEX, a token.
 */
export interface ApiDomain {
  name: string;
  description: string;
  fetch(options: { token?: string }): Promise<Record<string, unknown>>;
}
