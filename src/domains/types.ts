import type { Localization } from "../localization.js";
import type { GameRecord } from "../records.js";

/** What a domain gets to work with. */
export interface ParseContext {
  records: GameRecord[];
  /** The game's string table. Empty if global.ini was not extracted. */
  localization: Localization;
  /** Where extraction wrote, for domains that read archive files (`files`). */
  extractDir?: string;
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
  /**
   * Archive file globs to extract as-is, for data DataCore does not hold. They
   * land under the extraction directory at their archive path.
   */
  files?: string[];
  /**
   * Exact archive paths to extract once `files` are on disk, for files that are
   * only discovered by reading the first ones. Called again after each batch
   * lands, until it returns nothing.
   */
  moreFiles?(extractDir: string): string[];
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
