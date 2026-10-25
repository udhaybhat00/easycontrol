import { useEffect, useState } from "react";
import { Activity, ChevronRight, Computer, FileUp, Github, Home, Info, Link, Linkedin, LockKeyhole, Monitor, Radio, Settings, ShieldCheck, UserRound, X } from "lucide-react";
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

export function App() {
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
