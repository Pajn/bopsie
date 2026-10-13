import * as z from 'zod/mini';

/** A string the server may leave out or null; reads as `fallback` then. */
export const stringOr = (fallback: string) =>
  z.pipe(
    z.nullish(z.string()),
    z.transform((value) => value ?? fallback),
  );

/** A number the server may leave out or null; reads as `fallback` then. */
export const numberOr = (fallback: number) =>
  z.pipe(
    z.nullish(z.number()),
    z.transform((value) => value ?? fallback),
  );

/** A boolean the server may leave out or null; reads as `fallback` then. */
export const booleanOr = (fallback: boolean) =>
  z.pipe(
    z.nullish(z.boolean()),
    z.transform((value) => value ?? fallback),
  );

/** A list the server may leave out or null; reads as empty then. */
export const arrayOr = <T extends z.ZodMiniType>(item: T) =>
  z.pipe(
    z.nullish(z.array(item)),
    z.transform((value): z.output<T>[] => value ?? []),
  );
