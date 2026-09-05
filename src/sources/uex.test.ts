import { describe, it, expect } from "vitest";
import { MissingTokenError, trimRow, uexFetch } from "./uex.js";

function stubFetch(body: unknown, ok = true, status = 200) {
  return (async () => ({
    ok,
    status,
    json: async () => body,
  })) as unknown as typeof fetch;
}

describe("uexFetch", () => {
  it("returns the data array", async () => {
    const rows = await uexFetch("terminals", {
      token: "t",
      fetchImpl: stubFetch({ status: "ok", data: [{ id: 1 }, { id: 2 }] }),
    });
    expect(rows).toHaveLength(2);
  });

  it("wraps a single object so callers always get an array", async () => {
    const rows = await uexFetch("game_versions", {
      token: "t",
      fetchImpl: stubFetch({ status: "ok", data: { live: "4.10" } }),
    });
    expect(rows).toEqual([{ live: "4.10" }]);
  });

  it("explains how to get a token when none is set", async () => {
    await expect(
      uexFetch("terminals", { token: "", fetchImpl: stubFetch({}) }),
    ).rejects.toBeInstanceOf(MissingTokenError);
  });

  it("fails on a body-level error even though HTTP said 200", async () => {
    // UEX answers 200 with status:"access_denied" for a bad token, so checking
    // the status code alone would treat a failure as success.
    await expect(
      uexFetch("terminals", {
        token: "bad",
        fetchImpl: stubFetch({ status: "access_denied", message: "Invalid bearer token." }),
      }),
    ).rejects.toThrow(/Invalid bearer token/);
  });

  it("fails on a genuine HTTP error", async () => {
    await expect(
      uexFetch("terminals", { token: "t", fetchImpl: stubFetch({}, false, 502) }),
    ).rejects.toThrow(/HTTP 502/);
  });

  it("returns an empty array when there is no data", async () => {
    const rows = await uexFetch("x", { token: "t", fetchImpl: stubFetch({ status: "ok" }) });
    expect(rows).toEqual([]);
  });
});

describe("trimRow", () => {
  it("drops statistical columns that triple the payload", () => {
    expect(
      trimRow({
        price_buy: 100,
        price_buy_min: 90,
        price_buy_max_week: 120,
        price_buy_avg_month: 105,
        volatility_price_buy: 3,
      }),
    ).toEqual({ price_buy: 100 });
  });

  it("drops nulls and empty strings", () => {
    expect(trimRow({ a: 1, b: null, c: "" })).toEqual({ a: 1 });
  });

  it("keeps zero, which is a real price", () => {
    expect(trimRow({ price_buy: 0 })).toEqual({ price_buy: 0 });
  });

  it("passes non-objects through untouched", () => {
    expect(trimRow("x")).toBe("x");
    expect(trimRow(null)).toBe(null);
  });
});
