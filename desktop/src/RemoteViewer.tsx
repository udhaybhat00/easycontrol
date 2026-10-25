import { useEffect, useRef, useState } from "react";
import { Maximize2, Monitor, MousePointer2, Power, ScanLine } from "lucide-react";
import { core, decodeFramePacket, RemoteMonitor } from "./bridge";
import { remoteKeyCode } from "./keymap";

export function RemoteViewer({ inputAllowed, onDisconnect, onError }: { inputAllowed: boolean; onDisconnect: () => void; onError: (message: string) => void }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const lastSequence = useRef(0);
  const pendingMove = useRef<{ x: number; y: number }>();
  const raf = useRef<number>();
  const heldKeys = useRef(new Set<number>());
  const [monitors, setMonitors] = useState<RemoteMonitor[]>([]);
  const [selected, setSelected] = useState<RemoteMonitor>();
  const [stats, setStats] = useState({ fps: 0, dropped: 0, width: 0, height: 0 });
  const [originalSize, setOriginalSize] = useState(false);
  const [focused, setFocused] = useState(false);

  useEffect(() => {
    let live = true, frames = 0, started = performance.now();
    const refreshMonitors = () => core.monitors().then(items => { if (live) { setMonitors(items); setSelected(current => current && items.some(item => item.index === current.index) ? current : items[0]); } }).catch(error => onError(String(error)));
    refreshMonitors();
    const monitorTimer = window.setInterval(refreshMonitors, 1000);
    const timer = window.setInterval(async () => {
      try {
        const raw = await core.latestFrame(lastSequence.current);
        const bytes = raw instanceof Uint8Array ? raw : new Uint8Array(raw as unknown as ArrayBuffer);
        const frame = decodeFramePacket(bytes);
        if (!live || !frame) return;
        lastSequence.current = frame.sequence;
        const target = canvas.current;
        const context = target?.getContext("2d", { alpha: false });
        if (!target || !context) return;
        target.width = frame.width;
        target.height = frame.height;
        const pixels = new Uint8ClampedArray(frame.pixels.length);
        pixels.set(frame.pixels);
        context.putImageData(new ImageData(pixels, frame.width, frame.height), 0, 0);
        frames += 1;
        const elapsed = performance.now() - started;
        if (elapsed >= 1000) { setStats({ fps: Math.round(frames * 1000 / elapsed), dropped: frame.dropped, width: frame.width, height: frame.height }); frames = 0; started = performance.now(); }
      } catch (error) { if (live) onError(String(error)); }
    }, 50);
    return () => { live = false; window.clearInterval(timer); window.clearInterval(monitorTimer); };
  }, [onError]);

  const coordinates = (clientX: number, clientY: number) => {
    const target = canvas.current, monitor = selected;
    if (!target || !monitor) return;
    const rect = target.getBoundingClientRect();
    if (!rect.width || !rect.height) return;
    return { x: Math.round(monitor.x + (clientX - rect.left) * target.width / rect.width), y: Math.round(monitor.y + (clientY - rect.top) * target.height / rect.height) };
  };
  const send = (work: () => Promise<void>) => work().catch(error => onError(String(error)));
  const queueMove = (point: { x: number; y: number }) => { pendingMove.current = point; if (!raf.current) raf.current = requestAnimationFrame(() => { raf.current = undefined; const next = pendingMove.current; if (next) send(() => core.viewerInput({ kind: "mouseMove", ...next })); }); };
  const releaseKeys = () => { for (const code of heldKeys.current) send(() => core.viewerInput({ kind: "keyUp", code })); heldKeys.current.clear(); };

  return <article className="card live-viewer"><div className="card-title"><Monitor size={18}/><span>Remote session</span><em>{inputAllowed ? "Input requested" : "View only"}</em><div className="viewer-actions"><button className="icon-button" title="Fullscreen" onClick={() => document.documentElement.requestFullscreen?.().catch(error => onError(String(error)))}><Maximize2 size={17}/></button><button className="secondary" onClick={onDisconnect}><Power size={15}/>Disconnect</button></div></div>
    <div className="viewer-toolbar"><label><Monitor size={15}/>Remote monitor<select value={selected?.index ?? ""} onChange={event => { const index = Number(event.target.value); send(async () => { await core.selectMonitor(index); setSelected(monitors.find(monitor => monitor.index === index)); }); }}>{monitors.length ? monitors.map(monitor => <option key={monitor.index} value={monitor.index}>{`Display ${monitor.index + 1} · ${monitor.width}×${monitor.height}`}</option>) : <option>Waiting for remote inventory…</option>}</select></label><button className={originalSize ? "tool active" : "tool"} onClick={() => setOriginalSize(value => !value)}><ScanLine size={15}/>{originalSize ? "Original size" : "Fit to window"}</button><span><MousePointer2 size={15}/>{focused ? "Viewer focused" : "Click display to control"}</span><span>{stats.width ? `${stats.width}×${stats.height} · ${stats.fps} fps · ${stats.dropped} dropped` : "Waiting for first frame…"}</span></div>
    <div className="canvas-shell"><canvas ref={canvas} className={originalSize ? "remote-canvas original" : "remote-canvas"} tabIndex={0} aria-label="Remote desktop display" onFocus={() => setFocused(true)} onBlur={() => { setFocused(false); releaseKeys(); }} onMouseMove={event => { if (inputAllowed) { const point = coordinates(event.clientX, event.clientY); if (point) queueMove(point); } }} onMouseDown={event => { event.currentTarget.focus(); if (inputAllowed) { const point = coordinates(event.clientX, event.clientY); if (point) send(() => core.viewerInput({ kind: "mouseDown", button: event.button === 2 ? 2 : 1, ...point })); } }} onMouseUp={event => { if (inputAllowed) { const point = coordinates(event.clientX, event.clientY); if (point) send(() => core.viewerInput({ kind: "mouseUp", button: event.button === 2 ? 2 : 1, ...point })); } }} onContextMenu={event => event.preventDefault()} onWheel={event => { if (inputAllowed) { event.preventDefault(); send(() => core.viewerInput({ kind: "mouseWheel", dx: Math.round(event.deltaX), dy: Math.round(event.deltaY) })); } }} onKeyDown={event => { if (!inputAllowed) return; const code = remoteKeyCode(event); if (code !== undefined) { event.preventDefault(); heldKeys.current.add(code); send(() => core.viewerInput({ kind: "keyDown", code })); } }} onKeyUp={event => { const code = remoteKeyCode(event); if (code !== undefined) { event.preventDefault(); heldKeys.current.delete(code); send(() => core.viewerInput({ kind: "keyUp", code })); } }} /></div>
  </article>;
}
