import { useCallback, useEffect, useRef, useState } from "react";
import { Maximize2, Monitor, Power, ScanLine } from "lucide-react";
import { core, decodeFramePacket, type RemoteMonitor, type ViewerInput } from "./bridge";
import { HeldRemoteKeys, remoteKeyCode } from "./keymap";
import { mapCoordinates, mouseButton, scrollSteps } from "./viewer-utils";

export function RemoteViewer({ sessionId, inputAllowed, onInputAllowed, onDisconnect, onError }: {
  sessionId: string; inputAllowed: boolean; onInputAllowed: (enabled: boolean) => void;
  onDisconnect: () => void; onError: (message: string) => void;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const panel = useRef<HTMLElement>(null);
  const pendingMove = useRef<{ x: number; y: number }>();
  const raf = useRef<number>();
  const moveBusy = useRef(false);
  const wheelBusy = useRef(false);
  const pendingWheel = useRef({ dx: 0, dy: 0 });
  const heldKeys = useRef(new HeldRemoteKeys());
  const heldButtons = useRef(new Map<number, { x: number; y: number }>());
  const allowed = useRef(inputAllowed);
  const errors = useRef(onError);
  errors.current = onError;
  const [monitors, setMonitors] = useState<RemoteMonitor[]>([]);
  const [selected, setSelected] = useState<RemoteMonitor>();
  const [switching, setSwitching] = useState(false);
  const [stats, setStats] = useState({ fps: 0, dropped: 0, width: 0, height: 0, bytesPerSecond: 0 });
  const [originalSize, setOriginalSize] = useState(false);
  const [focused, setFocused] = useState(false);
  const [inputTransition, setInputTransition] = useState(false);
  allowed.current = inputAllowed && !inputTransition;
  const mounted = useRef(false);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);

  const send = useCallback((input: ViewerInput) => core.viewerInput(sessionId, input).catch(error => { if (mounted.current) errors.current(String(error)); }), [sessionId]);
  const release = useCallback(() => {
    pendingMove.current = undefined;
    pendingWheel.current = { dx: 0, dy: 0 };
    if (raf.current) cancelAnimationFrame(raf.current);
    raf.current = undefined;
    const releases = heldKeys.current.releaseAll().map(code => send({ kind: "keyUp", code }));
    for (const [button, point] of heldButtons.current) releases.push(send({ kind: "mouseUp", button, ...point }));
    heldButtons.current.clear();
    return Promise.all(releases);
  }, [send]);

  useEffect(() => {
    let live = true, sequence = 0, frames = 0, bytes = 0, started = performance.now();
    let timer: number | undefined;
    const refreshMonitors = async () => {
      try {
        const items = await core.monitors();
        if (live) {
          setMonitors(items);
          setSelected(current => items.find(item => item.index === current?.index) ?? items[0]);
        }
      } catch (error) { if (live) errors.current(String(error)); }
    };
    void refreshMonitors();
    const monitorTimer = window.setInterval(refreshMonitors, 2000);
    const poll = async () => {
      try {
        const raw = await core.latestFrame(sessionId, sequence);
        const packet = raw instanceof Uint8Array ? raw : new Uint8Array(raw as unknown as ArrayBuffer);
        const frame = decodeFramePacket(packet);
        if (packet.byteLength && !frame) throw new Error(`Invalid frame packet (${packet.byteLength} bytes)`);
        if (live && frame && frame.sequence > sequence) {
          sequence = frame.sequence;
          const target = canvas.current;
          const context = target?.getContext("2d", { alpha: false });
          if (target && context) {
            if (target.width !== frame.width) target.width = frame.width;
            if (target.height !== frame.height) target.height = frame.height;
            context.putImageData(new ImageData(new Uint8ClampedArray(frame.pixels), frame.width, frame.height), 0, 0);
            frames++;
            bytes += packet.byteLength;
            const elapsed = performance.now() - started;
            if (elapsed >= 1000) {
              setStats({ fps: Math.round(frames * 1000 / elapsed), dropped: frame.dropped,
                width: frame.width, height: frame.height, bytesPerSecond: bytes * 1000 / elapsed });
              frames = 0; bytes = 0; started = performance.now();
            }
          }
        }
      } catch (error) { if (live) errors.current(String(error)); }
      // At most one IPC request and one frame decode in flight.
      if (live) timer = window.setTimeout(poll, 16);
    };
    void poll();
    return () => { live = false; window.clearTimeout(timer); window.clearInterval(monitorTimer); release(); };
  }, [sessionId, release]);

  useEffect(() => { if (!inputAllowed) release(); }, [inputAllowed, release]);
  useEffect(() => {
    const blur = () => { setFocused(false); release(); };
    window.addEventListener("blur", blur);
    const visibility = () => { if (document.hidden) blur(); };
    document.addEventListener("visibilitychange", visibility);
    return () => { window.removeEventListener("blur", blur); document.removeEventListener("visibilitychange", visibility); };
  }, [release]);

  const coordinates = (x: number, y: number) => selected && canvas.current
    ? mapCoordinates(x, y, canvas.current.getBoundingClientRect(), selected) : undefined;
  const queueMove = (point: { x: number; y: number }) => {
    pendingMove.current = point;
    if (!raf.current && !moveBusy.current) raf.current = requestAnimationFrame(async () => {
      raf.current = undefined;
      const next = pendingMove.current;
      pendingMove.current = undefined;
      if (next && allowed.current) {
        moveBusy.current = true;
        await send({ kind: "mouseMove", ...next });
        moveBusy.current = false;
        if (pendingMove.current) queueMove(pendingMove.current);
      }
    });
  };
  const fullscreen = () => {
    const action = document.fullscreenElement ? document.exitFullscreen() : panel.current?.requestFullscreen();
    action?.catch(error => onError(String(error)));
  };
  const flushWheel = async () => {
    if (wheelBusy.current || !allowed.current) return;
    const delta = pendingWheel.current;
    if (!delta.dx && !delta.dy) return;
    pendingWheel.current = { dx: 0, dy: 0 };
    wheelBusy.current = true;
    await send({ kind: "mouseWheel", ...delta });
    wheelBusy.current = false;
    if (allowed.current) void flushWheel();
  };
  return <article ref={panel} className="live-viewer">
    <div className="card-title"><Monitor size={18}/><span>Remote session</span><em>{inputAllowed ? "Input requests enabled" : "View only"}</em>
      <div className="viewer-actions"><button className="icon-button" aria-label="Toggle fullscreen" title="Toggle fullscreen" onClick={fullscreen}><Maximize2 size={17}/></button><button className="button danger" onClick={onDisconnect}><Power size={15}/>Disconnect</button></div>
    </div>
    <div className="viewer-toolbar">
      <label>Remote monitor<select aria-label="Remote monitor" disabled={switching || !monitors.length} value={selected?.index ?? ""} onChange={async event => {
        const index = Number(event.target.value); release(); setSwitching(true);
        try { await core.selectMonitor(sessionId, index); setSelected(monitors.find(item => item.index === index)); }
        catch (error) { onError(String(error)); }
        finally { setSwitching(false); }
      }}>{monitors.length ? monitors.map(item => <option key={item.index} value={item.index}>{`Display ${item.index + 1} · ${item.width}×${item.height}`}</option>) : <option>Waiting for displays</option>}</select></label>
      <button className={originalSize ? "tool active" : "tool"} onClick={() => setOriginalSize(value => !value)}><ScanLine size={15}/>{originalSize ? "Original size" : "Fit to window"}</button>
      <label><input type="checkbox" checked={inputAllowed} disabled={inputTransition} onChange={async event => {
        const enabled = event.target.checked;
        setInputTransition(true);
        try {
          if (!enabled) { allowed.current = false; await release(); }
          if (mounted.current) await onInputAllowed(enabled);
        } finally { if (mounted.current) setInputTransition(false); }
      }}/>Send input (host approval required)</label>
      <span>{stats.width ? `${stats.width}×${stats.height} · ${stats.fps} fps · ${stats.dropped} dropped · ${(stats.bytesPerSecond / 1048576).toFixed(1)} MiB/s IPC` : "Waiting for screen frames…"}</span>
      <span>{focused ? "Display focused" : "Click the display to focus"}</span>
    </div>
    <div className="canvas-shell"><canvas ref={canvas} className={originalSize ? "remote-canvas original" : "remote-canvas"} tabIndex={0} aria-label="Remote desktop display"
      onFocus={() => setFocused(true)} onBlur={() => { setFocused(false); release(); }}
      onPointerMove={event => {
        if (focused && allowed.current && !switching) {
          const point = coordinates(event.clientX, event.clientY);
          if (point) { for (const button of heldButtons.current.keys()) heldButtons.current.set(button, point); queueMove(point); }
        }
      }}
      onPointerDown={event => {
        event.currentTarget.focus();
        if (!allowed.current || switching) return;
        const point = coordinates(event.clientX, event.clientY), button = mouseButton(event.button);
        if (point && button) { event.currentTarget.setPointerCapture(event.pointerId); heldButtons.current.set(button, point); void send({ kind: "mouseDown", button, ...point }); }
      }}
      onPointerUp={event => {
        const button = mouseButton(event.button), point = coordinates(event.clientX, event.clientY);
        const held = heldButtons.current.get(button);
        if (held) { heldButtons.current.delete(button); void send({ kind: "mouseUp", button, ...(point ?? held) }); }
        if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
      }}
      onLostPointerCapture={release} onContextMenu={event => event.preventDefault()}
      onWheel={event => {
        if (allowed.current && focused && !switching) {
          event.preventDefault();
          pendingWheel.current.dx = Math.max(-10, Math.min(10, pendingWheel.current.dx + scrollSteps(event.deltaX, event.deltaMode)));
          pendingWheel.current.dy = Math.max(-10, Math.min(10, pendingWheel.current.dy + scrollSteps(event.deltaY, event.deltaMode)));
          void flushWheel();
        }
      }}
      onKeyDown={event => {
        if (!allowed.current || switching) return;
        const code = remoteKeyCode(event);
        if (code !== undefined) {
          event.preventDefault();
          const pressed = heldKeys.current.press(event.code, code);
          if (pressed !== undefined) void send({ kind: "keyDown", code: pressed });
        }
      }}
      onKeyUp={event => {
        const code = heldKeys.current.release(event.code);
        if (code !== undefined) { event.preventDefault(); void send({ kind: "keyUp", code }); }
      }}
    /></div>
  </article>;
}
