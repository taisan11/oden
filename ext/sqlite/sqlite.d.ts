/** Synchronous Bun-compatible SQLite API, implemented with Turso (Rust). */
export type SQLQueryBindings =
  | string
  | number
  | bigint
  | boolean
  | null
  | ArrayBufferView
  | Record<string, string | number | bigint | boolean | null | ArrayBufferView>;
export interface DatabaseOptions {
  readonly?: boolean;
  readwrite?: boolean;
  create?: boolean;
  strict?: boolean;
  safeIntegers?: boolean;
}
export interface Changes {
  changes: number;
  lastInsertRowid: number | bigint;
}
export interface Transaction<A extends unknown[], T> {
  (...args: A): T;
  readonly default: Transaction<A, T>;
  readonly deferred: Transaction<A, T>;
  readonly immediate: Transaction<A, T>;
  readonly exclusive: Transaction<A, T>;
  readonly database: Database;
}
export class SQLiteError extends Error {
  code: string;
  errno: number;
  byteOffset: number;
}
export class Statement<
  ReturnType = unknown,
  ParamsType extends SQLQueryBindings[] = SQLQueryBindings[],
> implements Iterable<ReturnType>, Disposable {
  private constructor();
  readonly native: number | undefined;
  readonly isFinalized: boolean;
  readonly columnNames: string[];
  readonly columnTypes:
    | ("TEXT" | "INTEGER" | "FLOAT" | "BLOB" | "NULL" | null)[]
    | null;
  readonly declaredTypes: (string | null)[] | null;
  readonly paramsCount: number;
  get(...params: ParamsType): ReturnType | null;
  get(params: SQLQueryBindings[]): ReturnType | null;
  all(...params: ParamsType): ReturnType[];
  all(params: SQLQueryBindings[]): ReturnType[];
  values(
    ...params: ParamsType
  ): (string | number | bigint | Uint8Array | null)[][];
  values(
    params: SQLQueryBindings[],
  ): (string | number | bigint | Uint8Array | null)[][];
  /** Returns TEXT values as UTF-8 Uint8Array, in addition to BLOB values. */
  raw(...params: ParamsType): (number | bigint | Uint8Array | null)[][];
  raw(params: SQLQueryBindings[]): (number | bigint | Uint8Array | null)[][];
  run(...params: ParamsType): Changes;
  run(params: SQLQueryBindings[]): Changes;
  iterate(...params: ParamsType): IterableIterator<ReturnType>;
  iterate(params: SQLQueryBindings[]): IterableIterator<ReturnType>;
  as<T>(Class: new (...args: never[]) => T): Statement<T, ParamsType>;
  finalize(): void;
  toString(): string;
  toJSON(): {
    sql: string;
    isFinalized: boolean;
    paramsCount: number;
    columnNames: string[];
  };
  [Symbol.iterator](): IterableIterator<ReturnType>;
  [Symbol.dispose](): void;
}
export class Database implements Disposable {
  constructor(
    filename?: string | ArrayBufferLike | ArrayBufferView,
    options?: number | DatabaseOptions,
  );
  static MAX_QUERY_CACHE_SIZE: number;
  static open(filename?: string, options?: number | DatabaseOptions): Database;
  static deserialize(
    bytes: ArrayBufferLike | ArrayBufferView,
    options?: boolean | DatabaseOptions,
  ): Database;
  static setCustomSQLite(path: string): never;
  readonly filename: string;
  readonly handle: number | undefined;
  readonly closed: boolean;
  readonly strict: boolean;
  readonly safeIntegers: boolean;
  readonly inTransaction: boolean;
  prepare<
    ReturnType = unknown,
    ParamsType extends SQLQueryBindings | SQLQueryBindings[] =
      SQLQueryBindings[],
  >(
    sql: string,
    params?: ParamsType,
    flags?: number,
  ): Statement<
    ReturnType,
    ParamsType extends unknown[] ? ParamsType : [ParamsType]
  >;
  query<
    ReturnType = unknown,
    ParamsType extends SQLQueryBindings | SQLQueryBindings[] =
      SQLQueryBindings[],
  >(
    sql: string,
  ): Statement<
    ReturnType,
    ParamsType extends unknown[] ? ParamsType : [ParamsType]
  >;
  run(
    sql: string,
    ...bindings: (SQLQueryBindings | SQLQueryBindings[])[]
  ): Changes;
  exec(
    sql: string,
    ...bindings: (SQLQueryBindings | SQLQueryBindings[])[]
  ): Changes;
  clearQueryCache(): void;
  close(throwOnError?: boolean): void;
  transaction<A extends unknown[], T>(fn: (...args: A) => T): Transaction<A, T>;
  serialize(name?: string): import("node:buffer").Buffer;
  loadExtension(path: string, entryPoint?: string): never;
  fileControl(cmd: number, arg: unknown): never;
  [Symbol.dispose](): void;
}
export const constants: Readonly<Record<string, number>>;
/** Backend information; SQLite C bindings are not exposed by Turso. */
export const native: Readonly<{ backend: "turso"; version: string }>;
export default Database;
