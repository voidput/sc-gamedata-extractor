/**
 * UEX Corp API — live, crowdsourced market data.
 *
 * This is the complement to the game files. The shipped data says what a
 * commodity *is*; UEX says what it currently sells for and where. Neither source
 * can answer the other's question.
 *
 * Needs a bearer token from https://uexcorp.space (My Apps). Set UEXTOKEN.
 */

const BASE_URL = "https://api.uexcorp.space/2.0";

export interface UexOptions {
  token?: string;
  baseUrl?: string;
  /** Injectable for tests. */
  fetchImpl?: typeof fetch;
}

export class MissingTokenError extends Error {
  constructor() {
    super(
      "UEX datasets need an API token. Set UEXTOKEN in the environment, or pass --uex-token. Register an app at https://uexcorp.space to get one.",
    );
    this.name = "MissingTokenError";
  }
}

/**
 * Fetch one UEX endpoint.
 *
 * UEX answers with HTTP 200 and a `status` field even when a request fails, so
 * the body is checked rather than the status code alone.
 */
export async function uexFetch(
  endpoint: string,
  options: UexOptions = {},
): Promise<unknown[]> {
  const token = options.token ?? process.env.UEXTOKEN;
  if (!token) throw new MissingTokenError();

  const doFetch = options.fetchImpl ?? fetch;
  const url = `${options.baseUrl ?? BASE_URL}/${endpoint}`;

  const response = await doFetch(url, {
    headers: { Authorization: `Bearer ${token}` },
  });

  if (!response.ok) {
    throw new Error(`UEX ${endpoint} returned HTTP ${response.status}`);
  }

  const body = (await response.json()) as { status?: string; message?: string; data?: unknown };
  if (body.status && body.status !== "ok") {
    throw new Error(`UEX ${endpoint} failed: ${body.message || body.status}`);
  }

  const data = body.data;
  if (Array.isArray(data)) return data;
  if (data && typeof data === "object") return [data];
  return [];
}

/**
 * Historical and statistical columns roughly triple the payload and are not what
 * a snapshot is for, so drop them.
 */
export function trimRow(row: unknown): unknown {
  if (!row || typeof row !== "object" || Array.isArray(row)) return row;

  const out: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(row as Record<string, unknown>)) {
    if (/_(min|max|avg)(_week|_month)?$/.test(key)) continue;
    if (key.startsWith("volatility_")) continue;
    if (value === null || value === "") continue;
    out[key] = value;
  }
  return out;
}
