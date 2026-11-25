/**
 * Object containers: where things sit in space.
 *
 * DataCore names every location but places none of them. Positions live in the
 * solar system object containers, `.socpak` files in the archive, which are
 * plain zips holding one XML document that nests `<Child>` containers. Each
 * child carries a `pos` and `rot` relative to its parent and, when it is a star
 * map location, the `starMapRecord` id of its StarMapObject, which is the join
 * back to DataCore.
 *
 * Positions are metres. A child's are relative to its parent container, so the
 * absolute position is the chain composed from the system root, applying each
 * parent's rotation on the way down. Rotations are quaternions written
 * `w,x,y,z`, and nearly all are identity.
 */

import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";

import { findCryXml, nodesByTag, parseCryXml } from "./cryxml.js";

export type Vec3 = [number, number, number];
export type Quat = [number, number, number, number];

export interface PlacedContainer {
  /** Editor name of the container instance, e.g. `OOC_Stanton_1_Hurston`. */
  entityName?: string;
  /** StarMapObject record id, absent for containers that are not locations. */
  starMapRecord?: string;
  /** Archive path of the child's own container, when it is an external one. */
  container?: string;
  /** Absolute position in the system frame, metres. */
  position: Vec3;
  /** Nesting depth below the system root, 0 for its direct children. */
  depth: number;
  /** `entityName` of the containing child, absent at depth 0. */
  parent?: string;
  /** Record id of the nearest containing star map location. */
  parentRecord?: string;
  /** Absolute position of that containing location. */
  parentPosition?: Vec3;
  /** Absolute rotation, for placing the contents of the child's own container. */
  rotation: Quat;
}

/** The frame a container document is placed in. Defaults to the system root. */
export interface Frame {
  position: Vec3;
  rotation: Quat;
  depth: number;
  name?: string;
  record?: string;
  recordPosition?: Vec3;
}

const IDENTITY: Quat = [1, 0, 0, 0];

/** Rotate `v` by the unit quaternion `q` (w,x,y,z). */
export function rotate(q: Quat, v: Vec3): Vec3 {
  const [w, x, y, z] = q;
  const [vx, vy, vz] = v;
  const tx = 2 * (y * vz - z * vy);
  const ty = 2 * (z * vx - x * vz);
  const tz = 2 * (x * vy - y * vx);
  return [
    vx + w * tx + (y * tz - z * ty),
    vy + w * ty + (z * tx - x * tz),
    vz + w * tz + (x * ty - y * tx),
  ];
}

function multiply(a: Quat, b: Quat): Quat {
  const [aw, ax, ay, az] = a;
  const [bw, bx, by, bz] = b;
  return [
    aw * bw - ax * bx - ay * by - az * bz,
    aw * bx + ax * bw + ay * bz - az * by,
    aw * by - ax * bz + ay * bw + az * bx,
    aw * bz + ax * by - ay * bx + az * bw,
  ];
}

function numbers(value: string | undefined, count: number): number[] | undefined {
  if (!value) return undefined;
  const parts = value.split(",").map(Number);
  return parts.length === count && parts.every(Number.isFinite) ? parts : undefined;
}

function attributes(source: string): Map<string, string> {
  const out = new Map<string, string>();
  for (const match of source.matchAll(/([\w:.-]+)\s*=\s*"([^"]*)"/g)) out.set(match[1], match[2]);
  return out;
}

/**
 * Every `<Child>` container in an object container document, with its absolute
 * position. `<ChildObjectContainers>` wrappers are skipped by the word boundary,
 * and a child with an unreadable `pos` is placed at its parent rather than
 * dropped, so its own children still resolve.
 */
export function parseContainerXml(xml: string, frame?: Frame): PlacedContainer[] {
  const placed: PlacedContainer[] = [];
  const stack: Frame[] = [frame ?? { position: [0, 0, 0], rotation: IDENTITY, depth: -1 }];

  for (const tag of xml.matchAll(/<(\/?)Child\b([^>]*?)(\/?)>/g)) {
    if (tag[1] === "/") {
      if (stack.length > 1) stack.pop();
      continue;
    }

    const attrs = attributes(tag[2]);
    const parent = stack[stack.length - 1];
    const local = (numbers(attrs.get("pos"), 3) ?? [0, 0, 0]) as Vec3;
    const spin = (numbers(attrs.get("rot"), 4) ?? IDENTITY) as Quat;
    const offset = rotate(parent.rotation, local);
    const position: Vec3 = [
      parent.position[0] + offset[0],
      parent.position[1] + offset[1],
      parent.position[2] + offset[2],
    ];
    const entityName = attrs.get("entityName") || undefined;
    const starMapRecord = attrs.get("starMapRecord") || undefined;
    const rotation = multiply(parent.rotation, spin);
    const depth = parent.depth + 1;

    placed.push({
      entityName,
      starMapRecord,
      container: attrs.get("external") === "1" ? attrs.get("name") || undefined : undefined,
      position,
      depth,
      parent: parent.name,
      parentRecord: parent.record,
      parentPosition: parent.recordPosition,
      rotation,
    });

    if (tag[3] !== "/") {
      stack.push({
        position,
        rotation,
        depth,
        name: entityName,
        record: starMapRecord ?? parent.record,
        recordPosition: starMapRecord ? position : parent.recordPosition,
      });
    }
  }

  return placed;
}

/**
 * Entries of a zip, read from the central directory. A socpak's local headers
 * spell paths with backslashes where the central directory uses slashes, so
 * the central directory names are the ones returned.
 */
export function readZip(buffer: Buffer): Map<string, Buffer> {
  const entries = new Map<string, Buffer>();

  let end = -1;
  for (let i = buffer.length - 22; i >= 0 && i >= buffer.length - 22 - 0xffff; i--) {
    if (buffer.readUInt32LE(i) === 0x06054b50) {
      end = i;
      break;
    }
  }
  if (end < 0) throw new Error("not a zip: no end of central directory");

  const count = buffer.readUInt16LE(end + 10);
  let cursor = buffer.readUInt32LE(end + 16);

  for (let i = 0; i < count; i++) {
    if (buffer.readUInt32LE(cursor) !== 0x02014b50) throw new Error("corrupt zip central directory");
    const method = buffer.readUInt16LE(cursor + 10);
    const compressedSize = buffer.readUInt32LE(cursor + 20);
    const nameLength = buffer.readUInt16LE(cursor + 28);
    const extraLength = buffer.readUInt16LE(cursor + 30);
    const commentLength = buffer.readUInt16LE(cursor + 32);
    const localOffset = buffer.readUInt32LE(cursor + 42);
    const name = buffer.toString("utf-8", cursor + 46, cursor + 46 + nameLength).replace(/\\/g, "/");
    cursor += 46 + nameLength + extraLength + commentLength;

    if (name.endsWith("/")) continue;
    const dataStart =
      localOffset + 30 + buffer.readUInt16LE(localOffset + 26) + buffer.readUInt16LE(localOffset + 28);
    const data = buffer.subarray(dataStart, dataStart + compressedSize);

    if (method === 0) entries.set(name, Buffer.from(data));
    else if (method === 8) entries.set(name, zlib.inflateRawSync(data));
    // Other methods (the archive's own zstd) do not occur inside a socpak.
  }

  return entries;
}

/**
 * The container document of a socpak: the root-level `<name>.xml`, not its
 * `_editor` sibling and not the per-entity documents in subdirectories.
 */
export function readContainerXml(socpak: Buffer): string | undefined {
  for (const [name, data] of readZip(socpak)) {
    if (name.includes("/") || !name.endsWith(".xml") || name.endsWith("_editor.xml")) continue;
    return data.toString("utf-8");
  }
  return undefined;
}

/** Lower-cased, slash-separated archive path: the archive's casing is not stable. */
const archiveKey = (value: string) => value.replace(/\\/g, "/").toLowerCase();

/** Every extracted socpak under `extractDir`, by lower-cased archive path. */
export function indexContainers(extractDir: string): Map<string, string> {
  const index = new Map<string, string>();
  const walk = (dir: string, key: string) => {
    let entries: fs.Dirent[];
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const child = path.join(dir, entry.name);
      const childKey = `${key}/${entry.name.toLowerCase()}`;
      if (entry.isDirectory()) walk(child, childKey);
      else if (childKey.endsWith(".socpak")) index.set(childKey, child);
    }
  };

  for (const top of safeReaddir(extractDir)) {
    if (top.toLowerCase() !== "data") continue;
    for (const next of safeReaddir(path.join(extractDir, top))) {
      if (next.toLowerCase() !== "objectcontainers") continue;
      walk(path.join(extractDir, top, next), "data/objectcontainers");
    }
  }
  return index;
}

function safeReaddir(dir: string): string[] {
  try {
    return fs.readdirSync(dir);
  } catch {
    return [];
  }
}

/** Solar system containers: `pu/system/<system>/*system.socpak`. */
export function systemContainers(index: Map<string, string>): string[] {
  return [...index.keys()].filter((key) => /^data\/objectcontainers\/pu\/system\/[^/]+\/[^/]*system\.socpak$/.test(key)).sort();
}

/**
 * Obstruction radius of every star map location, by record id, read straight
 * from the extracted records. Needed while extracting, before any dataset
 * exists, to tell an orbiting station from an outpost on the surface.
 */
function obstructionRadii(extractDir: string): Map<string, number> {
  const radii = new Map<string, number>();
  const walk = (dir: string) => {
    for (const entry of safeDirents(dir)) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.name.endsWith(".json")) {
        try {
          const record = JSON.parse(fs.readFileSync(full, "utf-8"));
          const radius = record?._RecordValue_?.quantumTravelData?.obstructionRadius;
          if (typeof record?._RecordId_ === "string" && typeof radius === "number") radii.set(record._RecordId_, radius);
        } catch {
          // An unreadable record just leaves that location without a radius.
        }
      }
    }
  };
  walk(path.join(extractDir, "libs", "foundry", "records", "starmap"));
  return radii;
}

function safeDirents(dir: string): fs.Dirent[] {
  try {
    return fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return [];
  }
}

/** True when `placed` sits inside the body that contains it: on its surface. */
function onSurface(placed: PlacedContainer, radii: Map<string, number>): boolean {
  if (!placed.parentRecord || !placed.parentPosition) return false;
  const radius = radii.get(placed.parentRecord);
  if (!radius) return false;
  const [x, y, z] = placed.position;
  const [px, py, pz] = placed.parentPosition;
  return Math.hypot(x - px, y - py, z - pz) < radius;
}

/** How deep plain containers may nest inside one another below a system document. */
const MAX_PLAIN_NESTING = 4;

/**
 * What a system document places, with the contents of its plain containers
 * folded in. A plain container is an external `<Child>` that is no star map
 * location itself, such as an asteroid ring segment, yet can hold bodies:
 * Nyx's Delamar and Levski sit in `glaciemring_segment_levski`. Its children
 * belong to the system level, placed in the plain container's frame.
 * `onMissing` is told of each plain container not on disk.
 */
export function systemLevelPlacements(
  xml: string,
  document: (key: string) => string | undefined,
  onMissing: (key: string) => void = () => {},
): PlacedContainer[] {
  const out: PlacedContainer[] = [];
  const visit = (placements: PlacedContainer[], nesting: number, chain: Set<string>) => {
    for (const placed of placements) {
      out.push(placed);
      if (placed.starMapRecord || !placed.container || nesting >= MAX_PLAIN_NESTING) continue;
      const key = archiveKey(placed.container);
      if (chain.has(key)) continue;
      const inner = document(key);
      if (inner === undefined) {
        onMissing(key);
        continue;
      }
      const frame: Frame = {
        position: placed.position,
        rotation: placed.rotation,
        depth: placed.depth,
        name: placed.entityName,
        record: placed.parentRecord,
        recordPosition: placed.parentPosition,
      };
      visit(parseContainerXml(inner, frame), nesting + 1, new Set([...chain, key]));
    }
  };
  visit(parseContainerXml(xml), 0, new Set());
  return out;
}

/**
 * Containers still to extract: those of star map locations placed by a system
 * document or by a body it places, and the plain containers of the system
 * level that can hold bodies. A system's bodies come first; a second call,
 * once those are on disk, finds each body's stations, whose own containers
 * hold their armistice zones. Surface locations are not followed: there are
 * hundreds, and none of them is a place in space.
 */
export function unextractedLocationContainers(extractDir: string): string[] {
  const index = indexContainers(extractDir);
  const radii = obstructionRadii(extractDir);
  const wanted = new Set<string>();
  const want = (placed: PlacedContainer) => {
    if (!placed.starMapRecord || !placed.container || onSurface(placed, radii)) return;
    const target = archiveKey(placed.container);
    if (!index.has(target)) wanted.add(target);
  };
  const document = (key: string) => {
    const file = index.get(key);
    return file ? readContainerXml(fs.readFileSync(file)) : undefined;
  };

  for (const key of systemContainers(index)) {
    const xml = document(key);
    if (!xml) continue;
    for (const placed of systemLevelPlacements(xml, document, (missing) => wanted.add(missing))) {
      want(placed);
      if (!placed.starMapRecord || !placed.container) continue;
      const file = index.get(archiveKey(placed.container));
      const inner = file ? readContainerXml(fs.readFileSync(file)) : undefined;
      if (!inner) continue;
      const frame: Frame = {
        position: placed.position,
        rotation: placed.rotation,
        depth: placed.depth,
        name: placed.entityName,
        record: placed.starMapRecord,
        recordPosition: placed.position,
      };
      for (const child of parseContainerXml(inner, frame)) want(child);
    }
  }
  return [...wanted].sort();
}

/** What a location's own container says about the law around it. */
export interface ContainerZones {
  /** Radius of the largest armistice (green) zone in the container, metres. */
  armisticeRadius?: number;
  /** True when the container holds a monitored zone: crime there is seen. */
  monitored?: true;
  /**
   * The container holds a monitor zone jammer: a dead zone that cancels monitoring around it.
   * `radius` in metres when the placed jammer sets its own, absent when it keeps the default.
   */
  jammer?: { radius?: number };
}

/**
 * Armistice and monitored zones of one container.
 *
 * Armistice zones are `GreenZone` entities in the container's `.soc`, a chunk
 * file with one CryXmlB document inside; each carries its radius. Monitored
 * zones are placed in the editor document only, as objects of type
 * `MonitoredZone`, with no radius of their own: they take the default from the
 * MonitoredZone entity record. A `MonitorZoneJammer` entity in the `.soc` is the
 * opposite: a monitored zone of negative strength, a dead zone around an outlaw
 * station, with its own `radiusKm` when it departs from the entity's default.
 */
export function readZones(socpak: Buffer): ContainerZones {
  const zones: ContainerZones = {};
  for (const [name, data] of readZip(socpak)) {
    if (name.includes("/")) continue;
    if (name.endsWith("_editor.xml")) {
      if (/<Object [^>]*type="MonitoredZone"/.test(data.toString("utf-8"))) zones.monitored = true;
    } else if (name.endsWith(".soc")) {
      const start = findCryXml(data);
      if (start < 0) continue;
      for (const entity of nodesByTag(parseCryXml(data, start), "Entity")) {
        if (entity.attrs.EntityClass === "MonitorZoneJammer") {
          zones.jammer ??= {};
          for (const params of nodesByTag(entity, "EntityComponentMonitoredZone")) {
            const km = Number(params.attrs.radiusKm);
            if (km > 0) zones.jammer.radius = Math.max(zones.jammer.radius ?? 0, km * 1000);
          }
          continue;
        }
        if (entity.attrs.EntityClass !== "GreenZone") continue;
        let radius = Number(entity.attrs.Radius);
        for (const params of nodesByTag(entity, "EntityComponentGreenZone")) {
          if (Number(params.attrs.radius) > 0) radius = Number(params.attrs.radius);
        }
        if (radius > 0) zones.armisticeRadius = Math.max(zones.armisticeRadius ?? 0, radius);
      }
    }
  }
  return zones;
}

/** Zones of every extracted container a placement points at, by record id. */
export function loadZones(extractDir: string, placements: Map<string, PlacedContainer>): Map<string, ContainerZones> {
  const index = indexContainers(extractDir);
  const byContainer = new Map<string, ContainerZones>();
  const zones = new Map<string, ContainerZones>();

  for (const [record, placed] of placements) {
    if (!placed.container) continue;
    const key = archiveKey(placed.container);
    let found = byContainer.get(key);
    if (!found) {
      const file = index.get(key);
      try {
        found = file ? readZones(fs.readFileSync(file)) : {};
      } catch {
        found = {};
      }
      byContainer.set(key, found);
    }
    if (found.armisticeRadius || found.monitored || found.jammer) zones.set(record, found);
  }
  return zones;
}

/** How a planet or moon turns, from its own container document. */
export interface BodySpin {
  /** Hours the body takes to turn once (`planetRotationSpeed`). */
  rotationHours: number;
  /**
   * Radius of the body's rotating zone, metres (`gridRadius`): what the container places
   * inside it turns with the body, the surface and low orbit alike. What it places
   * farther out, such as moons, holds still.
   */
  gridRadius?: number;
}

/**
 * A body's rotation, read from its container document: the body's own entity
 * carries `planetRotationSpeed`, the document root `gridRadius`. The phase of
 * the turn is not in the files; only the rate.
 */
export function readSpin(xml: string): BodySpin | undefined {
  const hours = Number(xml.match(/\bplanetRotationSpeed="([^"]+)"/)?.[1]);
  if (!(hours > 0)) return undefined;
  const grid = Number(xml.match(/<ObjectContainer\b[^>]*\bgridRadius="([^"]+)"/)?.[1]);
  return grid > 0 ? { rotationHours: hours, gridRadius: grid } : { rotationHours: hours };
}

/** Rotation of every extracted body a placement points at, by record id. */
export function loadSpins(extractDir: string, placements: Map<string, PlacedContainer>): Map<string, BodySpin> {
  const index = indexContainers(extractDir);
  const spins = new Map<string, BodySpin>();
  for (const [record, placed] of placements) {
    if (!placed.container) continue;
    const file = index.get(archiveKey(placed.container));
    if (!file) continue;
    try {
      const xml = readContainerXml(fs.readFileSync(file));
      const spin = xml ? readSpin(xml) : undefined;
      if (spin) spins.set(record, spin);
    } catch {
      // An unreadable container just leaves that body without a rotation.
    }
  }
  return spins;
}

/**
 * Absolute positions of every star map location under `extractDir`, keyed by
 * StarMapObject record id.
 *
 * A system document places its bodies; each body's own container, where it was
 * extracted, places what belongs to it, in the body's frame. A record placed
 * more than once keeps its shallowest placement.
 */
export function loadPlacements(extractDir: string): Map<string, PlacedContainer> {
  const index = indexContainers(extractDir);
  const placements = new Map<string, PlacedContainer>();
  const documents = new Map<string, string | undefined>();

  const document = (key: string) => {
    if (!documents.has(key)) {
      const file = index.get(key);
      documents.set(key, file ? readContainerXml(fs.readFileSync(file)) : undefined);
    }
    return documents.get(key);
  };

  const keep = (placed: PlacedContainer) => {
    if (!placed.starMapRecord) return;
    const existing = placements.get(placed.starMapRecord);
    if (!existing || placed.depth < existing.depth) placements.set(placed.starMapRecord, placed);
  };

  for (const key of systemContainers(index)) {
    const xml = document(key);
    if (!xml) continue;

    for (const placed of systemLevelPlacements(xml, document)) {
      keep(placed);
      if (!placed.starMapRecord || !placed.container) continue;
      const inner = document(archiveKey(placed.container));
      if (!inner) continue;

      const frame: Frame = {
        position: placed.position,
        rotation: placed.rotation,
        depth: placed.depth,
        name: placed.entityName,
        record: placed.starMapRecord,
        recordPosition: placed.position,
      };
      for (const child of parseContainerXml(inner, frame)) keep(child);
    }
  }

  return placements;
}
