import { useCallback, useEffect, useRef, useState, type ButtonHTMLAttributes } from "react";
import { Activity, CircleDot, Copy, Eye, EyeOff, Github, History, Home, Info, Laptop, Link2, Linkedin, Monitor, Plus, RefreshCw, Settings, ShieldCheck, Trash2, Wifi, X } from "lucide-react";
import { core, type Permissions, type Status } from "./bridge";
import { RemoteViewer } from "./RemoteViewer";
import { HostPanel } from "./HostPanel";
import { PermissionPanel } from "./PermissionPanel";
import { validEndpoint } from "./viewer-utils";
import logo from "./assets/easycontrol-logo.png";

type Page = "home" | "recent" | "devices" | "settings" | "about";
type Device = { id: string; name: string; address: string; note?: string };
type Recent = { id: string; address: string; started: number; result: "Authenticated" | "Completed" | "Failed" };
const defaultPermissions: Permissions = { input: false, clipboard: false, fileTransfer: false, chat: false };
const namespace = import.meta.env.VITE_EC_PROFILE === "qa" ? "easycontrol.qa" : "easycontrol";
const KEY = { devices: `${namespace}.devices`, recent: `${namespace}.recent`, address: `${namespace}.lastAddress`, theme: `${namespace}.theme`, port: `${namespace}.port` };
const read = <T,>(key: string, fallback: T): T => { try { return JSON.parse(localStorage.getItem(key) ?? "") as T; } catch { return fallback; } };
const save = (key: string, value: unknown) => localStorage.setItem(key, JSON.stringify(value));
const makePassword = () => { const chars = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789"; return Array.from(crypto.getRandomValues(new Uint8Array(14)), byte => chars[byte % chars.length]).join(""); };
const formatTime = (value?: number) => value ? new Date(value).toLocaleString() : "Never";
const statusLabel = (status: Status) => status.state === "connected" ? "Connected" : status.state === "hosting" ? "Listening" : status.state === "connecting" ? "Connecting" : "Host stopped";

function Button({ children, kind = "secondary", className = "", ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { kind?: "primary" | "secondary" | "ghost" | "danger" }) { return <button type="button" className={`button ${kind} ${className}`} {...props}>{children}</button>; }
function IconButton({ label, children, ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) { return <button type="button" className="icon-button" aria-label={label} title={label} {...props}>{children}</button>; }
function StatusLine({ status }: { status: Status }) { return <span className={`connection-state ${status.state}`}><i aria-hidden="true" />{statusLabel(status)}</span>; }
function SwitchRow({ label, description, value, onChange, disabled = false }: { label: string; description: string; value: boolean; onChange: (next: boolean) => void; disabled?: boolean }) { return <div className="setting-row"><div><strong>{label}</strong><small>{description}</small></div><button className={`switch ${value ? "on" : ""}`} disabled={disabled} role="switch" aria-checked={value} aria-label={label} onClick={() => onChange(!value)}><i /></button></div>; }

export function App() {
  const [page, setPage] = useState<Page>("home");
  const [status, setStatus] = useState<Status>({ state: "idle", detail: "Host stopped" });
  const [hostPassword, setHostPassword] = useState(makePassword);
  const [hostPasswordVisible, setHostPasswordVisible] = useState(false);
  const [connectPassword, setConnectPassword] = useState("");
  const [connectPasswordVisible, setConnectPasswordVisible] = useState(false);
  const [address, setAddress] = useState(() => localStorage.getItem(KEY.address) ?? "");
  const [port, setPort] = useState(() => { const saved = Number(localStorage.getItem(KEY.port)); return Number.isInteger(saved) && saved >= 1024 && saved <= 65535 && saved !== 5900 ? saved : 5899; });
  const [endpoint, setEndpoint] = useState("");
  const [permissions, setPermissions] = useState<Permissions>(defaultPermissions);
  const [saveDevice, setSaveDevice] = useState(false);
  const [devices, setDevices] = useState<Device[]>(() => read(KEY.devices, []));
  const [recent, setRecent] = useState<Recent[]>(() => read(KEY.recent, []));
  const [theme, setTheme] = useState(() => localStorage.getItem(KEY.theme) ?? "system");
  const [toast, setToast] = useState("");
  const [modal, setModal] = useState<"device" | "clear" | null>(null);
  const [draft, setDraft] = useState({ name: "", address: "", note: "" });
  const [busy, setBusy] = useState(false);
  const connectionAddress = useRef(address);
  const recordedSessions = useRef(new Set<string>());
  const notify = useCallback((message: string) => { setToast(message); window.setTimeout(() => setToast(current => current === message ? "" : current), 5000); }, []);
  const applyTheme = (next: string) => { setTheme(next); localStorage.setItem(KEY.theme, next); if (next === "system") delete document.documentElement.dataset.theme; else document.documentElement.dataset.theme = next; };
  const validPort = Number.isInteger(port) && port >= 1024 && port <= 65535;
  const validAddress = validEndpoint(address);
  const hostActive = !!status.host && (status.state === "hosting" || status.state === "connected");
  const connectedAsClient = !status.host && status.state === "connected";

  useEffect(() => { applyTheme(theme); }, []);
  useEffect(() => {
    let live = true;
    const refresh = () => core.status().then(next => {
      if (!live) return;
      setStatus(next);
      if (next.state === "connected" && !next.host && next.sessionId && !recordedSessions.current.has(next.sessionId)) {
        recordedSessions.current.add(next.sessionId);
        setRecent(current => {
          const item: Recent = { id: next.sessionId!, address: connectionAddress.current, started: Date.now(), result: "Authenticated" };
          const updated = [item, ...current.filter(row => row.id !== item.id)].slice(0, 20);
          save(KEY.recent, updated);
          return updated;
        });
      }
    }).catch(error => live && notify(String(error)));
    refresh();
    const timer = window.setInterval(refresh, 1000);
    const unlisten = core.events(event => {
      if (!live) return;
      refresh();
      if (event.kind === "connected") {
        notify("Connected");
      }
      if (event.kind === "disconnected") { setStatus({ state: "idle", detail: "Host stopped" }); setEndpoint(""); notify(event.detail ?? "Disconnected"); }
      if (event.kind === "error" || event.kind === "denied") notify(event.detail ?? "Session operation was rejected");
    });
    return () => { live = false; window.clearInterval(timer); unlisten.then(stop => stop()); };
  }, [notify]);
  useEffect(() => { if (!hostActive) { setEndpoint(""); return; } core.localNetworkEndpoint(port).then(setEndpoint).catch(() => setEndpoint("")); }, [hostActive, port]);
  useEffect(() => { if (validPort && !hostActive) localStorage.setItem(KEY.port, String(port)); }, [port, validPort, hostActive]);
  useEffect(() => {
    if (!modal) return;
    const previous = document.activeElement as HTMLElement | null;
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]');
    const elements = () => Array.from(dialog?.querySelectorAll<HTMLElement>('button:not(:disabled), input, select, [tabindex="0"]') ?? []);
    elements()[0]?.focus();
    const trap = (event: KeyboardEvent) => {
      if (event.key !== "Tab") return;
      const items = elements(), first = items[0], last = items[items.length - 1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    };
    document.addEventListener("keydown", trap);
    return () => { document.removeEventListener("keydown", trap); previous?.focus(); };
  }, [modal]);
  useEffect(() => { const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") setModal(null); }; window.addEventListener("keydown", onKey); return () => window.removeEventListener("keydown", onKey); }, []);

  const updatePermissions = async (next: Permissions) => { try { await core.configurePermissions(next); setPermissions(next); } catch (error) { notify(String(error)); } };
  const copy = async (value: string) => { try { await navigator.clipboard.writeText(value); notify("Copied"); } catch { notify("Could not copy"); } };
  const toggleHost = async () => {
    if (!validPort) return notify("Use a port from 1024 to 65535");
    if (busy) return;
    setBusy(true);
    try {
      if (hostActive) { await core.disconnect(); setStatus(await core.status()); return; }
      localStorage.setItem(KEY.port, String(port));
      await core.startHost(port, hostPassword, { ...permissions, input: false });
      setPermissions(current => ({ ...current, input: false }));
      setStatus(await core.status()); notify(`Listening on port ${port}`);
    } catch (error) { setStatus(await core.status()); notify(String(error)); }
    finally { setBusy(false); }
  };
  const connect = async () => {
    if (!validAddress) return notify("Enter an address in host:port format");
    if (!connectPassword.trim()) return notify("Enter the session password");
    if (status.state !== "idle") return notify("Disconnect the current session first");
    connectionAddress.current = address.trim(); setStatus({ state: "connecting", detail: "Connecting" }); localStorage.setItem(KEY.address, address.trim());
    try { await core.connect(address.trim(), connectPassword, permissions); if (saveDevice && !devices.some(device => device.address === address.trim())) { const next = [{ id: crypto.randomUUID(), name: address.trim(), address: address.trim() }, ...devices]; setDevices(next); save(KEY.devices, next); } } catch (error) { setStatus({ state: "idle", detail: "Connection failed" }); notify(String(error)); }
  };
  const addDevice = () => {
    if (!draft.name.trim() || !validEndpoint(draft.address)) return notify("Enter a name and host:port address");
    const next = [{ id: crypto.randomUUID(), name: draft.name.trim(), address: draft.address.trim(), note: draft.note.trim() || undefined }, ...devices];
    setDevices(next); save(KEY.devices, next); setDraft({ name: "", address: "", note: "" }); setModal(null); notify("Device saved");
  };
  const gotoConnect = (nextAddress: string) => { setAddress(nextAddress); setPage("home"); };
  const nav: Array<[Page, typeof Home, string]> = [["home", Home, "Home"], ["recent", History, "Session history"], ["devices", Laptop, "Devices"]];

  return <main className="app-shell">
    <aside className="sidebar"><div className="brand"><img src={logo} alt=""/><span>EasyControl</span></div><nav aria-label="Main navigation" className="primary-nav">{nav.map(([id, Icon, label]) => <button key={id} className={`nav-item ${page === id ? "active" : ""}`} onClick={() => setPage(id)}><Icon size={18}/><span>{label}</span></button>)}</nav><nav aria-label="Application navigation" className="secondary-nav"><button className={`nav-item ${page === "settings" ? "active" : ""}`} onClick={() => setPage("settings")}><Settings size={18}/><span>Settings</span></button><button className={`nav-item ${page === "about" ? "active" : ""}`} onClick={() => setPage("about")}><Info size={18}/><span>About</span></button></nav><span className="sidebar-version">Protocol v5</span></aside>
    <section className="main"><div className="page">
      <header className="topbar"><div><h1>{page === "home" ? "Remote access" : page === "recent" ? "Session history" : page === "devices" ? "Devices" : page === "settings" ? "Settings" : "About"}</h1>{(page === "home" || page === "settings") && <p>{page === "home" ? "Host this computer or connect directly to another device." : "Appearance and host permissions."}</p>}</div><StatusLine status={status}/></header>
        {hostActive && <HostPanel permissions={permissions} onPermissions={updatePermissions} onDisconnect={() => core.disconnect().catch(error => notify(String(error)))} onError={notify}/>}
      {page === "home" && <>
        {connectedAsClient && status.sessionId && <RemoteViewer key={status.sessionId} sessionId={status.sessionId} inputAllowed={permissions.input} onInputAllowed={input => updatePermissions({ ...permissions, input })} onDisconnect={() => core.disconnect().catch(error => notify(String(error)))} onError={notify}/>}
        <div hidden={connectedAsClient}>
        <div className="split-panels">
          <article className="panel elevated-panel"><div className="panel-body"><div className="panel-header"><span className="panel-icon"><Monitor/></span><div><h2>Share this computer</h2><p>Give a trusted user the address and session password below.</p></div></div><div className="stack"><div className="field"><label htmlFor="host-port">Listening port</label><input id="host-port" disabled={hostActive || busy} className="input mono" type="number" min="1024" max="65535" value={port} onChange={event => setPort(Number(event.target.value))}/>{!validPort && <small className="field-error">Use a port from 1024 to 65535.</small>}</div><div className="field"><div className="field-label-row"><label htmlFor="host-password">Session password</label><Button kind="ghost" type="button" disabled={hostActive || busy} onClick={() => setHostPassword(makePassword)}><RefreshCw/>Regenerate</Button></div><div className="control-row"><input id="host-password" disabled={hostActive || busy} className="input mono" type={hostPasswordVisible ? "text" : "password"} value={hostPassword} onChange={event => setHostPassword(event.target.value)}/><IconButton label={hostPasswordVisible ? "Hide password" : "Show password"} onClick={() => setHostPasswordVisible(!hostPasswordVisible)}>{hostPasswordVisible ? <EyeOff/> : <Eye/>}</IconButton><IconButton label="Copy password" onClick={() => copy(hostPassword)}><Copy/></IconButton></div></div><div className={`endpoint ${hostActive ? "" : "endpoint-muted"}`}><Wifi size={16}/><span className="mono">{hostActive ? endpoint || "Address unavailable" : "Not listening"}</span><IconButton label="Copy local endpoint" disabled={!endpoint} onClick={() => copy(endpoint)}><Copy/></IconButton></div><Button kind={hostActive ? "danger" : "primary"} disabled={busy || (!hostActive && (!validPort || !hostPassword.trim() || status.state !== "idle"))} onClick={toggleHost}>{busy ? "Please wait…" : hostActive ? "Stop host" : "Start host"}</Button></div></div></article>
          <article className="panel elevated-panel"><div className="panel-body"><div className="panel-header"><span className="panel-icon"><Link2/></span><div><h2>Connect to a device</h2><p>Enter the address and password provided by the remote host.</p></div></div><form className="stack" onSubmit={event => { event.preventDefault(); connect(); }}><div className="field"><label htmlFor="remote-address">Address</label><input id="remote-address" autoFocus className="input mono" placeholder="192.168.1.24:5899" value={address} onChange={event => setAddress(event.target.value)}/>{address && !validAddress && <small className="field-error">Use host:port format.</small>}</div><div className="field"><label htmlFor="remote-password">Password</label><div className="control-row"><input id="remote-password" className="input mono" type={connectPasswordVisible ? "text" : "password"} value={connectPassword} onChange={event => setConnectPassword(event.target.value)}/><IconButton label={connectPasswordVisible ? "Hide password" : "Show password"} onClick={() => setConnectPasswordVisible(!connectPasswordVisible)}>{connectPasswordVisible ? <EyeOff/> : <Eye/>}</IconButton></div></div><label className="checkbox"><input type="checkbox" checked={saveDevice} onChange={event => setSaveDevice(event.target.checked)}/><span aria-hidden="true"/>Save this device</label><Button kind="primary" className="connect-button" type="submit" disabled={!validAddress || !connectPassword.trim() || status.state !== "idle"}>{status.state === "connecting" ? <><Activity className="spinner"/>Connecting</> : "Connect"}</Button><p className="security-note"><ShieldCheck/> End-to-end encrypted with SPAKE2</p></form></div></article>
        </div>
        <section className="section"><div className="section-header"><h2>Recent connections</h2>{recent.length > 0 && <Button kind="ghost" onClick={() => setPage("recent")}>View history</Button>}</div><div className="panel list">{recent.length ? recent.slice(0, 4).map(row => <div className="list-row" key={row.id}><div><strong className="mono">{row.address}</strong><small>{formatTime(row.started)}</small></div><Button onClick={() => gotoConnect(row.address)}>Connect</Button></div>) : <div className="empty"><History/><span>No recent connections.</span></div>}</div></section>
        {status.state === "connecting" && <Button kind="danger" onClick={() => core.disconnect().then(() => core.status()).then(setStatus).catch(error => notify(String(error)))}>Cancel connection</Button>}
        </div></>}
      {page === "recent" && <><div className="section-toolbar"><p>Connections stored on this computer.</p>{recent.length > 0 && <Button kind="danger" onClick={() => setModal("clear")}>Clear history</Button>}</div><div className="panel table-wrap">{recent.length ? <table className="table"><thead><tr><th>Device or address</th><th>Started</th><th>Result</th><th /></tr></thead><tbody>{recent.map(row => <tr key={row.id}><td className="mono">{row.address}</td><td>{formatTime(row.started)}</td><td><span className="badge"><CircleDot/> {row.result}</span></td><td><div className="inline-actions"><Button kind="ghost" onClick={() => gotoConnect(row.address)}>Connect</Button><IconButton label="Remove session" onClick={() => { const next = recent.filter(item => item.id !== row.id); setRecent(next); save(KEY.recent, next); }}><Trash2/></IconButton></div></td></tr>)}</tbody></table> : <div className="empty"><History/><span>No sessions yet.</span></div>}</div></>}
      {page === "devices" && <><div className="section-toolbar"><p>Saved addresses stay on this device. Passwords are never saved.</p><Button kind="primary" onClick={() => setModal("device")}><Plus/>Add device</Button></div><div className="panel list">{devices.length ? devices.map(device => <div className="list-row" key={device.id}><div><strong>{device.name}</strong><small className="mono">{device.address}{device.note ? ` · ${device.note}` : ""}</small></div><div className="inline-actions"><Button onClick={() => gotoConnect(device.address)}>Connect</Button><IconButton label="Delete device" onClick={() => { const next = devices.filter(item => item.id !== device.id); setDevices(next); save(KEY.devices, next); notify("Device removed"); }}><Trash2/></IconButton></div></div>) : <div className="empty"><Laptop/><span>No saved devices.</span><Button kind="secondary" onClick={() => setModal("device")}>Add device</Button></div>}</div></>}
      {page === "settings" && <PermissionPanel onError={notify}/>}
      {page === "settings" && <div className="settings-layout"><nav className="settings-tabs" aria-label="Settings sections"><button className="active">General</button><button disabled title="Not available yet">Host</button><button disabled title="Not available yet">Security</button></nav><div className="panel settings-panel"><div className="setting-row"><div><strong>Appearance</strong><small>Use the system setting or choose a fixed appearance.</small></div><select aria-label="Appearance" className="select" value={theme} onChange={event => applyTheme(event.target.value)}><option value="system">System</option><option value="light">Light</option><option value="dark">Dark</option></select></div><div className="setting-row"><div><strong>Default listening port</strong><small>Used when starting a new host session.</small></div><input aria-label="Default listening port" disabled={hostActive || busy} className="input mono compact-input" type="number" min="1024" max="65535" value={port} onChange={event => setPort(Number(event.target.value))}/></div><SwitchRow label="Allow remote input" description="Enable input only for this active session. Hosting starts view-only." disabled={status.state !== "connected"} value={permissions.input} onChange={input => updatePermissions({ ...permissions, input })}/><SwitchRow label="Allow file transfer" disabled={status.state !== "idle"} description="Permit file-transfer requests when supported by the active session." value={permissions.fileTransfer} onChange={fileTransfer => updatePermissions({ ...permissions, fileTransfer })}/><SwitchRow label="Allow clipboard" disabled={status.state !== "idle"} description="Permit clipboard synchronization when supported by the active session." value={permissions.clipboard} onChange={clipboard => updatePermissions({ ...permissions, clipboard })}/></div></div>}
      {page === "about" && <div className="panel about-card"><div className="panel-body about-grid"><div><div className="about-brand"><img src={logo} alt=""/><h2>EasyControl</h2></div><div className="key-value"><span>Version</span><strong>0.1.0</strong></div><div className="key-value"><span>Protocol</span><strong>v5</strong></div><div className="key-value"><span>License</span><strong>MIT</strong></div><div className="key-value"><span>Created by</span><strong>Udhay Bhat</strong></div><div className="key-value"><span>Encryption</span><strong>SPAKE2 authenticated</strong></div></div><div className="stack"><Button onClick={() => core.openCreatorProfile("source").catch(error => notify(String(error)))}><Github/>Source code</Button><Button onClick={() => core.openCreatorProfile("issues").catch(error => notify(String(error)))}><Info/>Report an issue</Button><Button onClick={() => core.openCreatorProfile("docs").catch(error => notify(String(error)))}><Link2/>Documentation</Button><Button onClick={() => copy(`EasyControl\nVersion: 0.1.0\nProtocol: v5\nStatus: ${status.state}`)}><Copy/>Copy diagnostic info</Button><Button kind="ghost" onClick={() => core.openCreatorProfile("linkedin").catch(error => notify(String(error)))}><Linkedin/>LinkedIn</Button><Button kind="ghost" onClick={() => core.openCreatorProfile("github").catch(error => notify(String(error)))}><Github/>Udhay on GitHub</Button></div></div></div>}
    </div></section>
    {toast && <div className="toast" role="status"><span>{toast}</span><IconButton label="Dismiss" onClick={() => setToast("")}><X/></IconButton></div>}
    {modal === "device" && <div className="modal-backdrop" role="dialog" aria-modal="true" aria-label="Add device"><section className="modal"><header><h2>Add device</h2><IconButton label="Close" onClick={() => setModal(null)}><X/></IconButton></header><p>Passwords are not stored.</p><div className="stack"><div className="field"><label htmlFor="device-name">Name</label><input id="device-name" className="input" value={draft.name} onChange={event => setDraft({ ...draft, name: event.target.value })}/></div><div className="field"><label htmlFor="device-address">Address</label><input id="device-address" className="input mono" placeholder="192.168.1.24:5899" value={draft.address} onChange={event => setDraft({ ...draft, address: event.target.value })}/></div><div className="field"><label htmlFor="device-note">Note</label><input id="device-note" className="input" value={draft.note} onChange={event => setDraft({ ...draft, note: event.target.value })}/></div></div><footer><Button kind="ghost" onClick={() => setModal(null)}>Cancel</Button><Button kind="primary" onClick={addDevice}>Save device</Button></footer></section></div>}
    {modal === "clear" && <div className="modal-backdrop" role="dialog" aria-modal="true" aria-label="Clear session history"><section className="modal"><header><h2>Clear history?</h2><IconButton label="Close" onClick={() => setModal(null)}><X/></IconButton></header><p>This removes local session history from this computer.</p><footer><Button kind="ghost" onClick={() => setModal(null)}>Cancel</Button><Button kind="danger" onClick={() => { setRecent([]); save(KEY.recent, []); setModal(null); notify("History cleared"); }}>Clear history</Button></footer></section></div>}
  </main>;
}
