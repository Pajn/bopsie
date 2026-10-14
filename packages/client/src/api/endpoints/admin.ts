import * as z from 'zod/mini';
import type { MaConnection } from '../client';
import { adminDevice, adminGroup, adminServer, logLine } from '../schemas/admin';

/** Every device and its endpoints, with all the server knows of each. Admins only. */
export const adminDevices = (connection: MaConnection) =>
  connection.command('admin/devices', {}, { schema: z.array(adminDevice) });

/** Each device that leads a group or plays, and the plan it plays by. Admins only. */
export const adminGroups = (connection: MaConnection) =>
  connection.command('admin/groups', {}, { schema: z.array(adminGroup) });

/** The server: version, what runs, the library and its last sync, the config. Admins only. */
export const adminServerInfo = (connection: MaConnection) =>
  connection.command('admin/server', {}, { schema: adminServer });

/** The log's lines after `after`, by sequence number, oldest first. Admins only. */
export const adminLogs = (connection: MaConnection, after?: number, limit?: number) =>
  connection.command(
    'admin/logs',
    { ...(after !== undefined && { after }), ...(limit !== undefined && { limit }) },
    { schema: z.array(logLine) },
  );
