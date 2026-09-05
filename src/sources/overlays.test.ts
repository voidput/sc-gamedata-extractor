import { describe, it, expect, beforeAll, afterAll } from "vitest";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import {
  BUILT_IN_SOURCES,
  fetchSource,
  findInstalledOverride,
  loadSources,
  validateSource,
} from "./overlays.js";

describe("validateSource", () => {
  const valid = { name: "mypack", files: ["https://example.com/a.ini"] };

  it("accepts a minimal source", () => {
    expect(validateSource(valid)).toMatchObject({ name: "mypack", perPatch: true });
  });

  it("defaults perPatch to true, since packs track game builds", () => {
    expect(validateSource(valid).perPatch).toBe(true);
    expect(validateSource({ ...valid, perPatch: false }).perPatch).toBe(false);
  });

  it("requires a name and at least one file", () => {
    expect(() => validateSource({ files: valid.files })).toThrow(/needs a name/);
    expect(() => validateSource({ name: "x", files: [] })).toThrow(/lists no files/);
  });

  it("rejects a name that could escape a directory", () => {
    // The name becomes a directory under overlays/.
    expect(() => validateSource({ ...valid, name: "../etc" })).toThrow(/may only contain/);
    expect(() => validateSource({ ...valid, name: "a/b" })).toThrow(/may only contain/);
  });

  it("rejects a non-HTTPS URL", () => {
    expect(() => validateSource({ ...valid, files: ["http://example.com/a.ini"] })).toThrow(
      /non-HTTPS/,
    );
  });
});

describe("loadSources", () => {
  let dir: string;

  beforeAll(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sc-sources-"));
  });

  afterAll(async () => {
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("returns the built-ins when no user file exists", async () => {
    const sources = await loadSources(dir);
    expect(sources.map((s) => s.name)).toEqual(BUILT_IN_SOURCES.map((s) => s.name));
  });

  it("adds user-defined packs alongside the built-ins", async () => {
    await fs.writeFile(
      path.join(dir, "overlay-sources.json"),
      JSON.stringify([{ name: "mypack", files: ["https://example.com/a.ini"] }]),
    );
    const names = (await loadSources(dir)).map((s) => s.name);
    expect(names).toContain("mypack");
    expect(names).toContain("starstrings");
  });

  it("lets a user entry replace a built-in of the same name", async () => {
    // So a pack can be repointed at a fork or pinned tag without editing code.
    await fs.writeFile(
      path.join(dir, "overlay-sources.json"),
      JSON.stringify([
        { name: "starstrings", description: "pinned", files: ["https://example.com/pinned.ini"] },
      ]),
    );
    const source = (await loadSources(dir)).find((s) => s.name === "starstrings");
    expect(source?.description).toBe("pinned");
    expect(source?.files).toHaveLength(1);
  });

  it("accepts a { sources: [...] } wrapper as well as a bare array", async () => {
    await fs.writeFile(
      path.join(dir, "overlay-sources.json"),
      JSON.stringify({ sources: [{ name: "wrapped", files: ["https://example.com/a.ini"] }] }),
    );
    expect((await loadSources(dir)).some((s) => s.name === "wrapped")).toBe(true);
  });

  it("reports a malformed file rather than silently ignoring it", async () => {
    await fs.writeFile(path.join(dir, "overlay-sources.json"), JSON.stringify([{ name: "bad" }]));
    await expect(loadSources(dir)).rejects.toThrow(/lists no files/);
  });
});

describe("fetchSource", () => {
  it("names files so lexical order matches published order", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "sc-fetch-"));
    const stub = (async () => ({ ok: true, status: 200, text: async () => "k=v" })) as unknown as typeof fetch;

    const written = await fetchSource(
      {
        name: "p",
        description: "",
        files: ["https://x/components.ini", "https://x/contracts.ini"],
      },
      dir,
      stub,
    );

    // A directory load sorts by name, so the prefix preserves intent.
    expect(written.map((w) => path.basename(w.file))).toEqual([
      "00-components.ini",
      "01-contracts.ini",
    ]);
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("names the pack and URL on an HTTP error", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "sc-fetch-"));
    const stub = (async () => ({ ok: false, status: 404 })) as unknown as typeof fetch;

    await expect(
      fetchSource({ name: "p", description: "", files: ["https://x/a.ini"] }, dir, stub),
    ).rejects.toThrow(/p: https:\/\/x\/a\.ini returned HTTP 404/);
    await fs.rm(dir, { recursive: true, force: true });
  });
});

describe("findInstalledOverride", () => {
  let dir: string;

  beforeAll(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sc-installed-"));
  });

  afterAll(async () => {
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("returns nothing when no pack is installed", async () => {
    expect(await findInstalledOverride(path.join(dir, "Data.p4k"))).toBeUndefined();
  });

  it("finds a pack installed with a capital Data directory", async () => {
    // The real install uses "Data", and checking only lowercase misses it.
    const target = path.join(dir, "Data", "Localization", "english");
    await fs.mkdir(target, { recursive: true });
    await fs.writeFile(path.join(target, "global.ini"), "k=v");

    expect(await findInstalledOverride(path.join(dir, "Data.p4k"))).toBe(
      path.join(target, "global.ini"),
    );
  });

  it("looks in the language directory it is asked for", async () => {
    expect(
      await findInstalledOverride(path.join(dir, "Data.p4k"), "german_(germany)"),
    ).toBeUndefined();
  });
});
