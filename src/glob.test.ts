import { describe, it, expect } from "vitest";
import { globToRegExp, matchesAny } from "./glob.js";

const m = (glob: string, path: string) => globToRegExp(glob).test(path);

describe("globToRegExp", () => {
  it("does not let a single star cross a slash", () => {
    // This is why "*mineable*" matched zero records against the archive.
    expect(m("*mineable*", "libs/foundry/records/mining/mineableelements/gold")).toBe(false);
  });

  it("lets a double star span directories", () => {
    expect(m("**/mineableelements/**", "libs/foundry/records/mining/mineableelements/gold")).toBe(
      true,
    );
  });

  it("matches a single segment with a star", () => {
    expect(m("mining/*", "mining/gold")).toBe(true);
    expect(m("mining/*", "mining/ore/gold")).toBe(false);
  });

  it("allows a leading double star to match zero directories", () => {
    expect(m("**/gold", "gold")).toBe(true);
    expect(m("**/gold", "a/b/gold")).toBe(true);
  });

  it("anchors the whole path rather than matching a substring", () => {
    expect(m("mining", "mining/gold")).toBe(false);
    expect(m("mining", "mining")).toBe(true);
  });

  it("matches case-insensitively, as record paths vary", () => {
    expect(m("**/SciTem/**", "libs/records/scitem/ships/x")).toBe(true);
  });

  it("treats regex characters in a glob as literals", () => {
    expect(m("a.b/c", "a.b/c")).toBe(true);
    expect(m("a.b/c", "axb/c")).toBe(false);
  });

  it("matches exactly one character with ?", () => {
    expect(m("s?ip", "ship")).toBe(true);
    expect(m("s?ip", "sllip")).toBe(false);
  });
});

describe("matchesAny", () => {
  const filters = ["**/entities/scitem/ships/**", "**/mining/**"];

  it("is true when any filter matches", () => {
    expect(matchesAny("libs/records/entities/scitem/ships/shield/x", filters)).toBe(true);
    expect(matchesAny("libs/records/mining/mineableelements/gold", filters)).toBe(true);
  });

  it("keeps FPS weapons out of a ship-scoped domain", () => {
    // Registering the FPS domain widened the record pool and let 252 personal
    // weapons into the ship component counts until domains were scoped.
    expect(matchesAny("libs/records/entities/scitem/weapons/rifle", filters)).toBe(false);
  });

  it("matches everything when there are no filters", () => {
    expect(matchesAny("anything/at/all", [])).toBe(true);
  });
});
