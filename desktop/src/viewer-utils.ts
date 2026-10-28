// Monitor bounds are input coordinates (points on macOS); frames use pixels.
export function mapCoordinates(clientX: number, clientY: number,
  rect: { left: number; top: number; width: number; height: number },
  monitor: { x: number; y: number; width: number; height: number }) {
  if (rect.width <= 0 || rect.height <= 0 || monitor.width <= 0 || monitor.height <= 0) return;
  const x = (clientX - rect.left) / rect.width, y = (clientY - rect.top) / rect.height;
  if (x < 0 || y < 0 || x > 1 || y > 1) return;
  return { x: monitor.x + Math.min(monitor.width - 1, Math.floor(x * monitor.width)),
    y: monitor.y + Math.min(monitor.height - 1, Math.floor(y * monitor.height)) };
}

// The Rust bridge accepts numeric SocketAddr, including bracketed IPv6.
export function validEndpoint(address: string) {
  const match = /^(\[[0-9a-fA-F:]+\]|\d{1,3}(?:\.\d{1,3}){3}):(\d{1,5})$/.exec(address.trim());
  if (!match || Number(match[2]) < 1 || Number(match[2]) > 65535) return false;
  return match[1].startsWith("[") || match[1].split(".").every(part => Number(part) <= 255);
}
export function mouseButton(button: number) { return [1, 3, 2][button]; }
export function scrollSteps(delta: number, mode: number) {
  return delta === 0 ? 0 : Math.sign(delta) * Math.min(10, Math.max(1, Math.round(Math.abs(delta) / (mode === 0 ? 40 : 1))));
}
