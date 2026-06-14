import type { components } from '../schema.gen'

export type Wire = components['schemas']
export type AssertTrue<T extends true> = T

/** Strips `null`/`undefined` so the nullability axis can be compared separately. */
type Defined<T> = T extends null | undefined ? never : T

type LeafCompatible<A, B> = [Defined<A>] extends [Defined<B>]
  ? true
  : [Defined<B>] extends [Defined<A>]
    ? true
    : false

type IsObject<T> =
  Defined<T> extends object
    ? Defined<T> extends readonly unknown[]
      ? false
      : string extends keyof Defined<T>
        ? false
        : number extends keyof Defined<T>
          ? false
          : true
    : false

type Depth = [unknown, unknown, unknown, unknown, unknown, unknown]
type Pop<D extends unknown[]> = D extends [unknown, ...infer Rest] ? Rest : []

type StructConforms<V, W, D extends unknown[]> = [Defined<V>] extends [never]
  ? true
  : [Defined<W>] extends [never]
    ? true
    : D extends []
      ? LeafCompatible<V, W>
      : IsObject<V> extends true
        ? IsObject<W> extends true
          ? [Exclude<keyof Defined<V>, keyof Defined<W>>] extends [never]
            ? [Exclude<keyof Defined<W>, keyof Defined<V>>] extends [never]
              ? AllKeysConform<Defined<V>, Defined<W>, D>
              : false
            : false
          : false
        : IsObject<W> extends true
          ? false
          : Defined<V> extends readonly (infer Ve)[]
            ? Defined<W> extends readonly (infer We)[]
              ? Conforms<Ve, We, D>
              : false
            : Defined<W> extends readonly unknown[]
              ? false
              : LeafCompatible<V, W>

type AllKeysConform<V, W, D extends unknown[]> = {
  [K in keyof V & keyof W]: Conforms<V[K], W[K], Pop<D>> extends true
    ? true
    : false
}[keyof V & keyof W] extends true
  ? true
  : false

type Covered<A, B, D extends unknown[]> = [false] extends [
  A extends unknown ? (MatchesAny<A, B, D> extends true ? true : false) : never,
]
  ? false
  : true

type MatchesAny<A, B, D extends unknown[]> = [true] extends [
  B extends unknown
    ? StructConforms<A, B, D> extends true
      ? true
      : false
    : never,
]
  ? true
  : false

type IsLeafUnion<T> = [true] extends [
  T extends unknown
    ? IsObject<T> extends true
      ? false
      : ArrayOf<T> extends true
        ? false
        : true
    : never,
]
  ? [false] extends [
      T extends unknown
        ? IsObject<T> extends true
          ? false
          : ArrayOf<T> extends true
            ? false
            : true
        : never,
    ]
    ? false
    : true
  : false

type ArrayOf<T> = Defined<T> extends readonly unknown[] ? true : false

export type Conforms<View, W, D extends unknown[] = Depth> =
  IsLeafUnion<View> extends true
    ? LeafCompatible<View, W>
    : IsLeafUnion<W> extends true
      ? LeafCompatible<View, W>
      : Covered<View, W, D> extends true
        ? Covered<W, View, D> extends true
          ? true
          : false
        : false
