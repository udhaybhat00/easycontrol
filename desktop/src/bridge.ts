import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Permissions = { input: boolean; clipboard: boolean; fileTransfer: boolean; chat: boolean };
export type Status = { state: string; detail: string; sessionId?: string };
export type CoreEvent = { kind: string; detail?: string; sessionId?: string };

export const core = {
  status: () => invoke<Status>("connection_status"),
  connect: (address: string, password: string, permissions: Permissions) => invoke<void>("connect", { address, password, permissions }),
  startHost: (port: number, password: string, permissions: Permissions) => invoke<void>("start_host", { port, password, permissions }),
  disconnect: () => invoke<void>("disconnect"),
  configurePermissions: (permissions: Permissions) => invoke<void>("configure_permissions", { permissions }),
  monitors: () => invoke<Array<{ index: number; width: number; height: number }>>("available_monitors"),
  events: (handler: (event: CoreEvent) => void) => listen<CoreEvent>("session-event", event => handler(event.payload))
};
