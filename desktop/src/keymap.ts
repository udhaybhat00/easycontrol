// Browser `code` is layout-independent; these values intentionally use the
// portable virtual-key subset consumed by the Rust/Enigo backend on macOS,
// Windows and Linux. System-reserved Meta shortcuts are left local.
const special: Record<string, number> = {
  Backspace: 0x08, Tab: 0x09, Enter: 0x0d, ShiftLeft: 0x11, ShiftRight: 0x11,
  ControlLeft: 0x10, ControlRight: 0x10, AltLeft: 0x12, AltRight: 0x12,
  Escape: 0x1b, Space: 0x20, PageUp: 0x21, PageDown: 0x22, End: 0x23,
  Home: 0x24, ArrowLeft: 0x25, ArrowUp: 0x26, ArrowRight: 0x27,
  ArrowDown: 0x28, Insert: 0x2d, Delete: 0x2e,
  F1: 0x70, F2: 0x71, F3: 0x72, F4: 0x73, F5: 0x74, F6: 0x75,
  F7: 0x76, F8: 0x77, F9: 0x78, F10: 0x79, F11: 0x7a, F12: 0x7b,
};

export function remoteKeyCode(event: { code: string; metaKey: boolean; altKey: boolean }): number | undefined {
  if (event.metaKey || (event.altKey && event.code === "Tab")) return undefined;
  if (special[event.code] !== undefined) return special[event.code];
  const match = /^(Key|Digit)([A-Z0-9])$/.exec(event.code);
  return match ? match[2].charCodeAt(0) : undefined;
}
