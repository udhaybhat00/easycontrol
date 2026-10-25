import { useEffect, useState } from "react";
import { CheckCircle2, ExternalLink, ShieldAlert } from "lucide-react";
import { core, DesktopPermissions } from "./bridge";

export function PermissionPanel({ onError }: { onError: (message: string) => void }) {
  const [status, setStatus] = useState<DesktopPermissions>();
  const refresh = () => core.desktopPermissions().then(setStatus).catch(error => onError(String(error)));
  useEffect(() => { refresh(); const timer = window.setInterval(refresh, 2000); return () => window.clearInterval(timer); }, []);
  const grantScreen = () => core.requestScreenRecording().then(refresh).catch(error => onError(String(error)));
  const open = (kind: "screen" | "accessibility") => core.openPrivacySettings(kind).catch(error => onError(String(error)));
  const state = (value?: string) => value === "granted" ? <CheckCircle2 size={17}/> : <ShieldAlert size={17}/>;
  return <article className="card permission-panel"><div className="card-title"><ShieldAlert size={18}/><span>Desktop permissions</span><em>{status?.platform ?? "Checking…"}</em></div><p>EasyControl uses only macOS-approved capture and input APIs. It never bypasses System Settings.</p><div className="permission-row"><span>{state(status?.screenRecording)}<strong>Screen Recording</strong><small>{status?.screenRecording === "granted" ? "Ready to share this Mac’s screen" : "Required before hosting a live display"}</small></span>{status?.screenRecording === "granted" ? <b>Granted</b> : <div><button className="secondary" onClick={grantScreen}>Request</button><button className="link-button" onClick={() => open("screen")}>Open Settings <ExternalLink size={13}/></button></div>}</div><div className="permission-row"><span>{state(status?.accessibility)}<strong>Accessibility</strong><small>{status?.accessibility === "granted" ? "Ready for approved remote input" : "Required only when the host approves remote input"}</small></span>{status?.accessibility === "granted" ? <b>Granted</b> : <button className="link-button" onClick={() => open("accessibility")}>Open Settings <ExternalLink size={13}/></button>}</div><div className="permission-row"><span><CheckCircle2 size={17}/><strong>Input Monitoring</strong><small>Not required: EasyControl does not observe global keystrokes.</small></span><b>Not required</b></div></article>;
}
