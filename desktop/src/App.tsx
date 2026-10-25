import { useEffect, useState, type ButtonHTMLAttributes } from "react";
import { Activity, ChevronRight, Computer, Copy, Eye, EyeOff, FileUp, Github, History, Home, Info, Link, Linkedin, LockKeyhole, Monitor, Plus, Radio, Settings, ShieldCheck, Trash2, UserRound, X } from "lucide-react";
import { core, Permissions, Status } from "./bridge";
import { RemoteViewer } from "./RemoteViewer";
import { HostPanel } from "./HostPanel";
import { PermissionPanel } from "./PermissionPanel";
import logo from "./assets/easycontrol-logo.png";

const defaultPermissions: Permissions = { input: false, clipboard: false, fileTransfer: false, chat: false };
const navigation = [[Home, "Home"], [Activity, "Recent Sessions"], [Computer, "Devices"], [FileUp, "File Transfer"], [Settings, "Settings"], [Info, "About"]] as const;

function Toggle({ label, value, onChange, description }: { label: string; value: boolean; onChange: (value: boolean) => void; description: string }) {
  return <label className="toggle"><span><strong>{label}</strong><small>{description}</small></span><button aria-label={label} className={value ? "switch on" : "switch"} onClick={() => onChange(!value)}><i /></button></label>;
}

function LegacyApp() {
  const [section, setSection] = useState("Home");
  const [status, setStatus] = useState<Status>({ state: "idle", detail: "Ready for a direct connection" });
  const [address, setAddress] = useState("");
  const [password, setPassword] = useState("");
  const [port, setPort] = useState(5900);
  const [permissions, setPermissions] = useState<Permissions>(defaultPermissions);
  const [notice, setNotice] = useState("");

  useEffect(() => {
    core.status().then(setStatus).catch(error => setNotice(String(error)));
    const unlisten = core.events(event => {
      if (event.kind === "connected") setStatus({ state: "connected", detail: "Encrypted session active", sessionId: event.sessionId });
      if (event.kind === "disconnected") setStatus({ state: "idle", detail: event.detail ?? "Session disconnected" });
      if (event.kind === "error" || event.kind === "denied") setNotice(event.detail ?? "Session operation was rejected");
    });
    return () => { unlisten.then(stop => stop()); };
  }, []);

  const updatePermissions = (next: Permissions) => { setPermissions(next); core.configurePermissions(next).catch(error => setNotice(String(error))); };
  const submit = async (action: "host" | "connect") => {
    setNotice("");
    if (!password.trim()) return setNotice("A non-empty session password is required.");
    try {
      setStatus({ state: action === "host" ? "hosting" : "connecting", detail: action === "host" ? "Waiting for a direct connection" : "Authenticating connection" });
      if (action === "host") await core.startHost(port, password, permissions);
      else await core.connect(address, password, permissions);
    } catch (error) { setStatus({ state: "idle", detail: "Connection did not start" }); setNotice(String(error)); }
  };
  const connected = status.state === "connected";
  const openCreatorProfile = (profile: "github" | "linkedin") => core.openCreatorProfile(profile).catch(error => setNotice(String(error)));

  return <main className="shell">
    <aside><div className="brand"><img className="brand-mark" src={logo} alt="EasyControl"/><span>EasyControl<small>Professional remote access</small></span></div><div className="workspace-label">WORKSPACE</div><nav>{navigation.map(([Icon, label]) => <button key={label} className={section === label ? "active" : ""} onClick={() => setSection(label)}><Icon size={18}/>{label}</button>)}</nav><div className="sidebar-foot"><ShieldCheck size={17}/><span><strong>Secure by design</strong><small>Protocol v5 · E2E encrypted</small></span></div></aside>
    <section className="content"><header><div><p className="eyebrow">{section === "Home" ? "CONTROL CENTER" : section.toUpperCase()}</p><h1>{section === "Home" ? "Remote access, under your control." : section}</h1><p className="header-subtitle">{section === "Home" ? "Start a protected session or connect directly to a trusted host." : "Configure and review your EasyControl workspace."}</p></div><div className="header-actions"><div className={"status " + status.state}><i />{status.detail}</div><span className="build-badge">DESKTOP · v0.1</span></div></header>
      {notice && <div className="notice"><span>{notice}</span><button onClick={() => setNotice("")}><X size={16}/></button></div>}
      {section === "Home" && <>
        <div className="dashboard-grid"><article className="card device"><div className="card-title"><Radio size={18}/><span>This computer</span><em>Ready</em></div><h2>Share this Mac</h2><p>Host a direct authenticated session. You remain in control of screen sharing and remote input.</p><div className="host-row"><label>Listening port<input type="number" value={port} onChange={event => setPort(Number(event.target.value))}/></label><button className="secondary" onClick={() => submit("host")} disabled={connected}>Start host</button></div><div className="card-foot"><span>Local endpoint</span><strong>Direct LAN only</strong></div></article>
          <article className="card connect"><div className="card-title"><Link size={18}/><span>Connect to a device</span><em>Direct</em></div><h2>Start a secure session</h2><p>Use a trusted <code>host:port</code> address and a session password. No cloud identity is implied.</p><input aria-label="Remote host and port" placeholder="192.168.1.24:5900" value={address} onChange={event => setAddress(event.target.value)}/><input aria-label="Session password" type="password" placeholder="Session password" value={password} onChange={event => setPassword(event.target.value)}/><button className="primary" onClick={() => submit("connect")} disabled={connected}>Connect securely <ChevronRight size={17}/></button></article></div>
        <div className="lower-grid"><article className="card"><div className="card-title"><LockKeyhole size={18}/><span>Connection security</span></div><ul className="security"><li><ShieldCheck/>SPAKE2 password authentication</li><li><ShieldCheck/>Encrypted key confirmation</li><li><ShieldCheck/>Least-privilege session controls</li></ul></article><article className="card"><div className="card-title"><Activity size={18}/><span>Recent connections</span></div><div className="empty"><Monitor size={22}/><span>No saved sessions yet</span><small>Recent session storage is planned for a later phase.</small></div></article></div>
        <HostPanel permissions={permissions} onPermissions={updatePermissions} onDisconnect={() => core.disconnect().catch(error => setNotice(String(error)))} onError={setNotice}/>
        {connected && <RemoteViewer inputAllowed={permissions.input} onDisconnect={() => core.disconnect().catch(error => setNotice(String(error)))} onError={setNotice}/>}
      </>}
      {section === "Settings" && <div className="settings-stack"><PermissionPanel onError={setNotice}/><article className="settings card"><div className="card-title"><Settings size={18}/><span>Session permissions</span></div><p>The host controls remote input. A client-side toggle only enables sending requests; the host independently denies them unless its own setting was enabled before hosting.</p><Toggle label="Remote input" description="Host approval for mouse and keyboard injection" value={permissions.input} onChange={value => updatePermissions({...permissions, input:value})}/><Toggle label="Clipboard" description="Disabled by default; no clipboard UI in this phase" value={permissions.clipboard} onChange={value => updatePermissions({...permissions, clipboard:value})}/><Toggle label="File transfer" description="Disabled by default; no file-transfer UI in this phase" value={permissions.fileTransfer} onChange={value => updatePermissions({...permissions, fileTransfer:value})}/><Toggle label="Chat" description="Disabled by default; no chat UI in this phase" value={permissions.chat} onChange={value => updatePermissions({...permissions, chat:value})}/><div className="preference-grid"><div><strong>Display preferences</strong><small>Use the viewer toolbar to select an advertised remote monitor and scaling mode.</small></div><div><strong>Connection preferences</strong><small>Only direct host:port connections are configured in this release.</small></div></div></article></div>}
      {section === "About" && <article className="card about-card"><div className="about-hero"><img src={logo} alt="EasyControl"/><div><div className="card-title"><Info size={18}/><span>About EasyControl</span></div><h2>Secure remote access, built with intent.</h2><p>EasyControl is a direct, authenticated remote desktop application. It keeps security controls with the host and does not depend on a cloud device identity.</p></div></div><div className="about-details"><div><UserRound size={18}/><span>Created by</span><strong>Udhay Bhat</strong><small>Computer Science Engineering · Cybersecurity</small></div><button type="button" className="about-link" onClick={() => openCreatorProfile("github")}><Github size={18}/><span>GitHub</span><strong>@udhaybhat00</strong><small>View public profile ↗</small></button><button type="button" className="about-link" onClick={() => openCreatorProfile("linkedin")}><Linkedin size={18}/><span>LinkedIn</span><strong>Udhay Bhat</strong><small>Open professional profile ↗</small></button><div><ShieldCheck size={18}/><span>Built on</span><strong>Protocol v5</strong><small>Encrypted direct sessions</small></div></div><footer className="about-footer">Designed and developed by Udhay Bhat · EasyControl Desktop</footer></article>}
      {section !== "Home" && section !== "Settings" && section !== "About" && <article className="card placeholder"><Info size={26}/><h2>{section}</h2><p>This desktop foundation wires the session core for direct connections. This surface will gain its workflow in a later phase.</p></article>}
      {connected && <div className="session-strip"><span><span className="dot"/>Active session {status.sessionId ? `· ${status.sessionId.slice(0, 10)}…` : ""}</span><button onClick={() => core.disconnect()}>Disconnect</button></div>}
    </section>
  </main>;
}

type Page = "home" | "recent" | "devices" | "settings" | "about";
type Device = { id: string; name: string; address: string; note?: string; lastUsed?: number };
type Recent = { id: string; address: string; started: number; duration?: number; result: "Completed" | "Failed" };
const KEY = { devices: "easycontrol.devices", recent: "easycontrol.recent", address: "easycontrol.lastAddress", theme: "easycontrol.theme", port: "easycontrol.port" };
const read = <T,>(key: string, fallback: T): T => { try { return JSON.parse(localStorage.getItem(key) ?? "") as T; } catch { return fallback; } };
const save = (key: string, value: unknown) => localStorage.setItem(key, JSON.stringify(value));
const password = () => {
  const chars = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
  const bytes = crypto.getRandomValues(new Uint8Array(14));
  return Array.from(bytes, byte => chars[byte % chars.length]).join("");
};
function Button({ children, kind = "secondary", className = "", ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { kind?: "primary" | "secondary" | "ghost" | "danger" }) {
  return <button className={`button ${kind} ${className}`} {...props}>{children}</button>;
}
function IconButton({ label, children, ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) { return <button className="icon-button" aria-label={label} title={label} {...props}>{children}</button>; }
function SwitchRow({ label, description, value, onChange }: { label: string; description: string; value: boolean; onChange: (next: boolean) => void }) {
  return <div className="setting-row"><div><strong>{label}</strong><small>{description}</small></div><button className={`switch ${value ? "on" : ""}`} role="switch" aria-checked={value} aria-label={label} onClick={() => onChange(!value)}><i /></button></div>;
}
function Header({ title, description, status }: { title: string; description?: string; status: Status }) {
  const text = status.state === "connected" ? "Connected" : status.state === "hosting" ? "Listening" : status.state === "connecting" ? "Connecting" : "Host stopped";
  return <div className="topbar"><div className="page-header"><h1>{title}</h1>{description && <p>{description}</p>}</div><div className="status-line"><i className={`status-dot ${status.state}`} />{text}</div></div>;
}
function time(value?: number) { return value ? new Date(value).toLocaleString() : "Never"; }

export function App() {
  const [page, setPage] = useState<Page>("home");
  const [status, setStatus] = useState<Status>({ state: "idle", detail: "Host stopped" });
  const [hostPassword, setHostPassword] = useState(password);
  const [hostPasswordVisible, setHostPasswordVisible] = useState(false);
  const [connectPassword, setConnectPassword] = useState("");
  const [connectPasswordVisible, setConnectPasswordVisible] = useState(false);
  const [address, setAddress] = useState(() => localStorage.getItem(KEY.address) ?? "");
  const [port, setPort] = useState(() => Number(localStorage.getItem(KEY.port) ?? 5900));
  const [permissions, setPermissions] = useState<Permissions>(defaultPermissions);
  const [saveDevice, setSaveDevice] = useState(false);
  const [devices, setDevices] = useState<Device[]>(() => read(KEY.devices, []));
  const [recent, setRecent] = useState<Recent[]>(() => read(KEY.recent, []));
  const [toast, setToast] = useState("");
  const [theme, setTheme] = useState(() => localStorage.getItem(KEY.theme) ?? "system");
  const [modal, setModal] = useState<"device" | "clear" | null>(null);
  const [draft, setDraft] = useState({ name: "", address: "", note: "" });
  const [startedAt, setStartedAt] = useState<number>();
  const [lastDirection, setLastDirection] = useState<"host" | "connect">("connect");
  const notify = (message: string) => { setToast(message); window.setTimeout(() => setToast(current => current === message ? "" : current), 3200); };
  const applyTheme = (next: string) => { setTheme(next); localStorage.setItem(KEY.theme, next); if (next === "system") delete document.documentElement.dataset.theme; else document.documentElement.dataset.theme = next; };
  useEffect(() => { applyTheme(theme); }, []);
  useEffect(() => {
    core.status().then(setStatus).catch(error => notify(String(error)));
    const unlisten = core.events(event => {
      if (event.kind === "connected") { setStatus({ state: "connected", detail: "Connected", sessionId: event.sessionId }); setStartedAt(Date.now()); if (lastDirection === "connect") { const item = { id: crypto.randomUUID(), address, started: Date.now(), result: "Completed" as const }; const next = [item, ...recent.filter(row => row.address !== address)].slice(0, 20); setRecent(next); save(KEY.recent, next); } notify("Connected"); }
      if (event.kind === "disconnected") { setStatus({ state: "idle", detail: "Host stopped" }); setStartedAt(undefined); notify(event.detail ?? "Disconnected"); }
      if (event.kind === "error" || event.kind === "denied") notify(event.detail ?? "Session operation was rejected");
    });
    return () => { unlisten.then(stop => stop()); };
  }, [address, lastDirection, recent]);
  const updatePermissions = (next: Permissions) => { setPermissions(next); core.configurePermissions(next).catch(error => notify(String(error))); };
  const validPort = Number.isInteger(port) && port >= 1024 && port <= 65535;
  const validAddress = /^.+:\d+$/.test(address.trim());
  const copy = async (value: string) => { try { await navigator.clipboard.writeText(value); notify("Copied"); } catch { notify("Could not copy"); } };
  const host = async () => {
    if (!validPort) return notify("Use a port from 1024 to 65535");
    if (status.state === "hosting" || status.state === "connected") { await core.disconnect().catch(error => notify(String(error))); return; }
    setLastDirection("host"); setStatus({ state: "hosting", detail: "Listening" }); localStorage.setItem(KEY.port, String(port));
    try { await core.startHost(port, hostPassword, permissions); notify(`Listening on port ${port}`); } catch (error) { setStatus({ state: "idle", detail: "Host stopped" }); notify(String(error)); }
  };
  const connect = async () => {
    if (!validAddress) return notify("Enter an address in host:port format");
    if (!connectPassword.trim()) return notify("Enter the session password");
    setLastDirection("connect"); setStatus({ state: "connecting", detail: "Connecting" }); localStorage.setItem(KEY.address, address.trim());
    try { await core.connect(address.trim(), connectPassword, permissions); if (saveDevice) { const existing = devices.find(device => device.address === address.trim()); if (!existing) { const item = { id: crypto.randomUUID(), name: address.trim(), address: address.trim(), lastUsed: Date.now() }; const next = [item, ...devices]; setDevices(next); save(KEY.devices, next); } } } catch (error) { setStatus({ state: "idle", detail: "Host stopped" }); notify(String(error)); }
  };
  const addDevice = () => { if (!draft.name.trim() || !/^.+:\d+$/.test(draft.address.trim())) return notify("Enter a name and host:port address"); const next = [{ id: crypto.randomUUID(), name: draft.name.trim(), address: draft.address.trim(), note: draft.note.trim() || undefined }, ...devices]; setDevices(next); save(KEY.devices, next); setModal(null); setDraft({ name: "", address: "", note: "" }); notify("Device saved"); };
  const removeDevice = (id: string) => { const next = devices.filter(device => device.id !== id); setDevices(next); save(KEY.devices, next); notify("Device removed"); };
  const removeRecent = (id: string) => { const next = recent.filter(row => row.id !== id); setRecent(next); save(KEY.recent, next); };
  const gotoConnect = (nextAddress: string) => { setAddress(nextAddress); setPage("home"); };
  const openProfile = (profile: "github" | "linkedin") => core.openCreatorProfile(profile).catch(error => notify(String(error)));
  const nav = [["home", Home, "Home"], ["recent", History, "Recent"], ["devices", Monitor, "Devices"]] as const;
  const connected = status.state === "connected";
  return <main className="app-shell">
    <aside className="sidebar"><div className="brand"><img src={logo} alt=""/><span>EasyControl</span></div><nav className="primary-nav">{nav.map(([id, Icon, label]) => <button key={id} className={`nav-item ${page === id ? "active" : ""}`} onClick={() => setPage(id)}><Icon size={18}/>{label}</button>)}</nav><nav className="secondary-nav"><button className={`nav-item ${page === "settings" ? "active" : ""}`} onClick={() => setPage("settings")}><Settings size={18}/>Settings</button><button className={`nav-item ${page === "about" ? "active" : ""}`} onClick={() => setPage("about")}><Info size={18}/>About</button></nav><div className="sidebar-version">Protocol v5</div></aside>
    <section className="main"><div className="page">
      {page === "home" && <><Header title="Home" status={status}/><div className="split-panels"><article className="panel"><div className="panel-body"><div className="panel-header"><div><h2 className="panel-title"><Monitor/>Share this computer</h2><p className="panel-description">Start a direct session for another EasyControl user.</p></div></div><div className="stack"><div className="field"><label htmlFor="host-port">Listening port</label><input id="host-port" className="input mono" type="number" min="1024" max="65535" value={port} onChange={event => setPort(Number(event.target.value))}/>{!validPort && <small>Use a port from 1024 to 65535.</small>}</div><div className="field"><label htmlFor="host-password">Session password</label><div className="control-row"><input id="host-password" className="input mono" type={hostPasswordVisible ? "text" : "password"} value={hostPassword} onChange={event => setHostPassword(event.target.value)}/><IconButton label={hostPasswordVisible ? "Hide password" : "Show password"} onClick={() => setHostPasswordVisible(!hostPasswordVisible)}>{hostPasswordVisible ? <EyeOff/> : <Eye/>}</IconButton><IconButton label="Copy password" onClick={() => copy(hostPassword)}><Copy/></IconButton></div><Button kind="ghost" onClick={() => setHostPassword(password())}>Regenerate</Button></div><div className="endpoint"><span className="mono">Listening on port {port}</span><IconButton label="Copy local endpoint" onClick={() => copy(`localhost:${port}`)}><Copy/></IconButton></div><p className="status-copy">{status.state === "hosting" ? "Waiting for a client" : connected && lastDirection === "host" ? "A client is connected" : "Host stopped"}</p><Button kind={status.state === "hosting" || (connected && lastDirection === "host") ? "danger" : "primary"} onClick={host}>{status.state === "hosting" || (connected && lastDirection === "host") ? "Stop host" : "Start host"}</Button></div></div></article>
      <article className="panel"><div className="panel-body"><div className="panel-header"><div><h2 className="panel-title"><Link/>Connect to a device</h2><p className="panel-description">Enter the address and password provided by the host.</p></div></div><form className="stack" onSubmit={event => { event.preventDefault(); connect(); }}><div className="field"><label htmlFor="remote-address">Address</label><input id="remote-address" autoFocus className="input mono" placeholder="192.168.1.24:5900" value={address} onChange={event => setAddress(event.target.value)}/>{address && !validAddress && <small>Use host:port format.</small>}</div><div className="field"><label htmlFor="remote-password">Password</label><div className="control-row"><input id="remote-password" className="input mono" type={connectPasswordVisible ? "text" : "password"} value={connectPassword} onChange={event => setConnectPassword(event.target.value)}/><IconButton label={connectPasswordVisible ? "Hide password" : "Show password"} onClick={() => setConnectPasswordVisible(!connectPasswordVisible)}>{connectPasswordVisible ? <EyeOff/> : <Eye/>}</IconButton></div></div><label className="helper"><input type="checkbox" checked={saveDevice} onChange={event => setSaveDevice(event.target.checked)}/> Save this device</label><Button kind="primary" type="submit" disabled={status.state === "connecting"}>{status.state === "connecting" ? "Connecting…" : "Connect"}</Button><div className="encryption-note"><ShieldCheck/>End-to-end encrypted (SPAKE2)</div></form></div></article></div>
      <section className="section"><div className="section-header"><h2>Recent connections</h2>{recent.length > 0 && <Button kind="ghost" onClick={() => setPage("recent")}>View all</Button>}</div><div className="panel list">{recent.length ? recent.slice(0, 4).map(row => <div className="list-row" key={row.id}><div><div className="list-title mono">{row.address}</div><div className="list-meta">{time(row.started)}</div></div><Button kind="secondary" onClick={() => gotoConnect(row.address)}>Connect</Button></div>) : <div className="empty">No recent connections.</div>}</div></section>
      {connected && <RemoteViewer inputAllowed={permissions.input} onDisconnect={() => core.disconnect().catch(error => notify(String(error)))} onError={notify}/>}</>}
      {page === "recent" && <><Header title="Recent connections" status={status}/><div className="section-header"><span className="helper">Connections recorded on this computer</span>{recent.length > 0 && <Button kind="danger" onClick={() => setModal("clear")}>Clear history</Button>}</div><div className="panel">{recent.length ? <table className="table"><thead><tr><th>Address</th><th>Direction</th><th>Started</th><th>Result</th><th /></tr></thead><tbody>{recent.map(row => <tr key={row.id}><td className="mono">{row.address}</td><td>Outgoing</td><td>{time(row.started)}</td><td><span className="badge">{row.result}</span></td><td><div className="inline-actions"><Button kind="ghost" onClick={() => gotoConnect(row.address)}>Connect</Button><IconButton label="Remove" onClick={() => removeRecent(row.id)}><Trash2/></IconButton></div></td></tr>)}</tbody></table> : <div className="empty">No sessions yet.</div>}</div></>}
      {page === "devices" && <><Header title="Devices" status={status}/><div className="section-header"><span className="helper">Saved addresses are stored locally. Passwords are never saved.</span><Button kind="primary" onClick={() => setModal("device")}><Plus/>Add device</Button></div><div className="panel list">{devices.length ? devices.map(device => <div className="list-row" key={device.id}><div><div className="list-title">{device.name}</div><div className="list-meta mono">{device.address}{device.note ? ` · ${device.note}` : ""}</div></div><div className="inline-actions"><Button kind="secondary" onClick={() => gotoConnect(device.address)}>Connect</Button><IconButton label="Delete device" onClick={() => removeDevice(device.id)}><Trash2/></IconButton></div></div>) : <div className="empty">No saved devices.</div>}</div></>}
      {page === "settings" && <><Header title="Settings" status={status}/><div className="settings-layout"><nav className="settings-tabs"><button className="active">General</button><button>Host</button><button>Security</button></nav><div className="panel settings-panel"><SwitchRow label="Theme" description="Choose the appearance used by EasyControl." value={false} onChange={() => {}}/><div className="setting-row"><div><strong>Appearance</strong><small>Follow the system setting or choose a fixed theme.</small></div><select className="select" value={theme} onChange={event => applyTheme(event.target.value)}><option value="system">System</option><option value="light">Light</option><option value="dark">Dark</option></select></div><div className="setting-row"><div><strong>Default listening port</strong><small>Used for new host sessions.</small></div><input className="input mono" style={{ width: 120 }} type="number" min="1024" max="65535" value={port} onChange={event => setPort(Number(event.target.value))}/></div><SwitchRow label="Allow remote input" description="Allow the host to approve mouse and keyboard control." value={permissions.input} onChange={input => updatePermissions({ ...permissions, input })}/><SwitchRow label="Allow file transfer" description="Permit file-transfer requests when supported by the session." value={permissions.fileTransfer} onChange={fileTransfer => updatePermissions({ ...permissions, fileTransfer })}/><SwitchRow label="Allow clipboard" description="Permit clipboard synchronization when supported by the session." value={permissions.clipboard} onChange={clipboard => updatePermissions({ ...permissions, clipboard })}/></div></div></>}
      {page === "about" && <><Header title="About" status={status}/><div className="panel"><div className="panel-body about-grid"><div><h2 className="panel-title"><Info/>EasyControl</h2><div className="key-value"><span>Version</span><strong>0.1.0</strong></div><div className="key-value"><span>Protocol</span><strong>v5</strong></div><div className="key-value"><span>Encryption</span><strong>SPAKE2 authenticated</strong></div></div><div className="stack"><Button kind="secondary" onClick={() => openProfile("github")}><Github/>Source and issues</Button><Button kind="secondary" onClick={() => copy(`EasyControl\nVersion: 0.1.0\nProtocol: v5\nStatus: ${status.state}`)}><Copy/>Copy diagnostic info</Button><Button kind="ghost" onClick={() => openProfile("linkedin")}><Linkedin/>Creator profile</Button></div></div></div></>}
    </div></section>
    {toast && <div className="toast" role="status">{toast}</div>}
    {modal === "device" && <div className="modal-backdrop" role="dialog" aria-modal="true" aria-label="Add device"><div className="modal"><h2>Add device</h2><p>Passwords are not stored.</p><div className="stack"><div className="field"><label>Name</label><input className="input" value={draft.name} onChange={event => setDraft({ ...draft, name: event.target.value })}/></div><div className="field"><label>Address</label><input className="input mono" placeholder="192.168.1.24:5900" value={draft.address} onChange={event => setDraft({ ...draft, address: event.target.value })}/></div><div className="field"><label>Note</label><input className="input" value={draft.note} onChange={event => setDraft({ ...draft, note: event.target.value })}/></div></div><div className="modal-actions"><Button kind="ghost" onClick={() => setModal(null)}>Cancel</Button><Button kind="primary" onClick={addDevice}>Save device</Button></div></div></div>}
    {modal === "clear" && <div className="modal-backdrop" role="dialog" aria-modal="true" aria-label="Clear history"><div className="modal"><h2>Clear history?</h2><p>This removes the local connection history from this computer.</p><div className="modal-actions"><Button kind="ghost" onClick={() => setModal(null)}>Cancel</Button><Button kind="danger" onClick={() => { setRecent([]); save(KEY.recent, []); setModal(null); notify("History cleared"); }}>Clear history</Button></div></div></div>}
  </main>;
}
