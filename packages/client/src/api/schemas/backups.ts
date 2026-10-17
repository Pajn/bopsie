import * as z from 'zod/mini';

/** A backup kept in the server's backups directory. */
export const backup = z.looseObject({
  name: z.string(),
  /** Bytes. */
  size: z.number(),
  /** Unix seconds. */
  created: z.number(),
  encrypted: z.boolean(),
  /** Made on the schedule, which removes the oldest of them. */
  scheduled: z.boolean(),
});
export type Backup = z.infer<typeof backup>;

/** When backups are made by themselves; hours and days are UTC, Monday being day 0. */
export const backupSchedule = z.looseObject({
  every: z.enum(['daily', 'weekly']),
  hour: z.number(),
  weekday: z.number(),
  keep: z.number(),
});
export type BackupSchedule = z.infer<typeof backupSchedule>;

export const backupsStatus = z.looseObject({
  /** Where backups are kept, on the server. */
  dir: z.string(),
  /** Newest first. */
  backups: z.array(backup),
  schedule: z.nullable(backupSchedule),
  /** Scheduled backups are encrypted with a passphrase kept on the server. */
  encrypted: z.boolean(),
  last_run: z.nullable(
    z.looseObject({
      at: z.number(),
      backup: z.nullable(z.string()),
      error: z.nullable(z.string()),
    }),
  ),
  /** A backup is being made now. */
  making: z.boolean(),
});
export type BackupsStatus = z.infer<typeof backupsStatus>;
