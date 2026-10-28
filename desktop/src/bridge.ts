import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Permissions = { input: boolean; clipboard: boolean; fileTransfer: boolean; chat: boolean };
export type Status = { state: string; detail: string; sessionId?: string; host?: boolean };
export type DesktopPermissions = { platform: string; screenRecording: string; accessibility: string; inputMonitoring: string };
export type HostSessionStatus = { active: boolean; screenSharing: boolean; remoteInput: boolean; clipboard: boolean; fileTransfer: boolean; peer?: string };
export type CoreEvent = { kind: string; detail?: string; sessionId?: string };
export type RemoteMonitor = { index: number; x: number; y: number; width: number; height: number; primary: boolean };
export type ViewerInput =
  | { kind: "mouseMove"; x: number; y: number }
  | { kind: "mouseDown" | "mouseUp"; button: number; x: number; y: number }
  | { kind: "mouseWheel"; dx: number; dy: number }
  | { kind: "keyDown" | "keyUp"; code: number };
export type FramePacket = { sequence: number; width: number; height: number; stride: number; dropped: number; pixels: Uint8ClampedArray };

export function decodeFramePacket(raw: Uint8Array): FramePacket | undefined {
  if (raw.byteLength < 32 || String.fromCharCode(...raw.slice(0, 4)) !== "ECF1") return undefined;
  const view = new DataView(raw.buffer, raw.byteOffset, raw.byteLength);
  const sequence = Number(view.getBigUint64(4, true));
  const width = view.getUint32(12, true), height = view.getUint32(16, true), stride = view.getUint32(20, true), dropped = Number(view.getBigUint64(24, true));
  if (!Number.isSafeInteger(sequence) || !width || !height || stride < width * 4 || stride * height > 64 * 1024 * 1024 || raw.byteLength !== 32 + stride * height) return undefined;
  const pixels = new Uint8ClampedArray(width * height * 4);
  for (let row = 0; row < height; row++) pixels.set(raw.subarray(32 + row * stride, 32 + row * stride + width * 4), row * width * 4);
  return { sequence, width, height, stride, dropped, pixels };
}

export const core = {
  status: () => invoke<Status>("connection_status"),
  connect: (address: string, password: string, permissions: Permissions) => invoke<void>("connect", { address, password, permissions }),
  startHost: (port: number, password: string, permissions: Permissions) => invoke<void>("start_host", { port, password, permissions }),
  localNetworkEndpoint: (port: number) => invoke<string>("local_network_endpoint", { port }),
  disconnect: () => invoke<void>("disconnect"),
  configurePermissions: (permissions: Permissions) => invoke<void>("configure_permissions", { permissions }),
  desktopPermissions: () => invoke<DesktopPermissions>("desktop_permissions"),
  requestScreenRecording: () => invoke<boolean>("request_screen_recording_permission"),
  openPrivacySettings: (kind: "screen" | "accessibility") => invoke<void>("open_privacy_settings", { kind }),
  openCreatorProfile: (profile: "github" | "linkedin" | "source" | "issues") => invoke<void>("open_creator_profile", { profile }),
  hostSessionStatus: () => invoke<HostSessionStatus>("host_session_status"),
  monitors: () => invoke<RemoteMonitor[]>("available_monitors"),
  selectMonitor: (sessionId: string, index: number) => invoke<void>("select_remote_monitor", { sessionId, index }),
  latestFrame: (sessionId: string, afterSequence: number) => invoke<Uint8Array>("latest_frame", { sessionId, afterSequence }),
  viewerInput: (sessionId: string, input: ViewerInput) => invoke<void>("viewer_input", { sessionId, input }),
  events: (handler: (event: CoreEvent) => void) => listen<CoreEvent>("session-event", event => handler(event.payload))
};
