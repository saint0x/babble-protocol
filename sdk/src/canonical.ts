export type CanonicalPrimitive = null | boolean | number | string | CanonicalFloat | CanonicalUnsigned;
export type CanonicalValue =
  | CanonicalPrimitive
  | readonly CanonicalValue[]
  | { readonly [key: string]: CanonicalValue };

const PREAMBLE = new TextEncoder().encode("babel.canonical.v1\0");
const FLOAT_BRAND: unique symbol = Symbol("babel.canonical.float");
const UNSIGNED_BRAND: unique symbol = Symbol("babel.canonical.unsigned");
const MAX_I64 = 9_223_372_036_854_775_807n;
const MIN_I64 = -9_223_372_036_854_775_808n;
const MAX_U64 = 18_446_744_073_709_551_615n;

export interface CanonicalFloat {
  readonly [FLOAT_BRAND]: true;
  readonly value: number;
}

export interface CanonicalUnsigned {
  readonly [UNSIGNED_BRAND]: true;
  readonly value: bigint;
}

export function canonicalFloat(value: number): CanonicalFloat {
  if (!Number.isFinite(value)) {
    throw new TypeError(`canonical float must be finite: ${value}`);
  }
  return { [FLOAT_BRAND]: true, value };
}

export function canonicalUnsigned(value: bigint): CanonicalUnsigned {
  if (value < 0n || value > MAX_U64) {
    throw new TypeError(`canonical unsigned integer must fit in u64: ${value}`);
  }
  return { [UNSIGNED_BRAND]: true, value };
}

export function canonicalValueBytes(value: CanonicalValue): Uint8Array {
  const writer = new CanonicalWriter();
  writer.bytes(PREAMBLE);
  encodeValue(value, writer);
  return writer.finish();
}

export function canonicalValueHex(value: CanonicalValue): string {
  return bytesToHex(canonicalValueBytes(value));
}

export function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function encodeValue(value: CanonicalValue, writer: CanonicalWriter): void {
  if (value === null) {
    writer.ascii("n");
    return;
  }
  if (typeof value === "boolean") {
    writer.ascii(value ? "t" : "f");
    return;
  }
  if (typeof value === "number") {
    encodeNumber(value, writer);
    return;
  }
  if (typeof value === "string") {
    encodeString(value, writer);
    return;
  }
  if (isCanonicalFloat(value)) {
    encodeFloat(value.value, writer);
    return;
  }
  if (isCanonicalUnsigned(value)) {
    writer.ascii("u");
    writer.u64(value.value);
    return;
  }
  if (isCanonicalArray(value)) {
    writer.ascii("a");
    writer.u64(BigInt(value.length));
    for (const item of value) {
      encodeValue(item, writer);
    }
    return;
  }
  encodeObject(value, writer);
}

function encodeNumber(value: number, writer: CanonicalWriter): void {
  if (!Number.isFinite(value)) {
    throw new TypeError(`canonical number must be finite: ${value}`);
  }
  if (Number.isSafeInteger(value)) {
    const integer = BigInt(value);
    if (integer < MIN_I64 || integer > MAX_I64) {
      throw new TypeError(`canonical integer must fit in i64: ${value}`);
    }
    writer.ascii("i");
    writer.i64(integer);
    return;
  }
  encodeFloat(value, writer);
}

function encodeFloat(value: number, writer: CanonicalWriter): void {
  if (!Number.isFinite(value)) {
    throw new TypeError(`canonical float must be finite: ${value}`);
  }
  writer.ascii("d");
  const buffer = new ArrayBuffer(8);
  new DataView(buffer).setFloat64(0, value, false);
  writer.bytes(new Uint8Array(buffer));
}

function encodeString(value: string, writer: CanonicalWriter): void {
  const encoded = new TextEncoder().encode(value);
  writer.ascii("s");
  writer.u64(BigInt(encoded.length));
  writer.bytes(encoded);
}

function encodeObject(value: { readonly [key: string]: CanonicalValue }, writer: CanonicalWriter): void {
  const entries = Object.entries(value).sort(([left], [right]) =>
    compareBytes(new TextEncoder().encode(left), new TextEncoder().encode(right)),
  );
  writer.ascii("o");
  writer.u64(BigInt(entries.length));
  for (const [key, item] of entries) {
    encodeString(key, writer);
    encodeValue(item, writer);
  }
}

function compareBytes(left: Uint8Array, right: Uint8Array): number {
  const length = Math.min(left.length, right.length);
  for (let index = 0; index < length; index += 1) {
    const difference = left[index]! - right[index]!;
    if (difference !== 0) {
      return difference;
    }
  }
  return left.length - right.length;
}

function isCanonicalFloat(value: unknown): value is CanonicalFloat {
  return typeof value === "object" && value !== null && (value as CanonicalFloat)[FLOAT_BRAND] === true;
}

function isCanonicalUnsigned(value: unknown): value is CanonicalUnsigned {
  return typeof value === "object" && value !== null && (value as CanonicalUnsigned)[UNSIGNED_BRAND] === true;
}

function isCanonicalArray(value: unknown): value is readonly CanonicalValue[] {
  return Array.isArray(value);
}

class CanonicalWriter {
  readonly #bytes: number[] = [];

  ascii(value: string): void {
    this.#bytes.push(value.charCodeAt(0));
  }

  bytes(value: Uint8Array): void {
    this.#bytes.push(...value);
  }

  i64(value: bigint): void {
    this.u64(BigInt.asUintN(64, value));
  }

  u64(value: bigint): void {
    if (value < 0n || value > MAX_U64) {
      throw new TypeError(`canonical length/integer must fit in u64: ${value}`);
    }
    for (let shift = 56n; shift >= 0n; shift -= 8n) {
      this.#bytes.push(Number((value >> shift) & 0xffn));
    }
  }

  finish(): Uint8Array {
    return Uint8Array.from(this.#bytes);
  }
}
