import { useEffect, useRef, useState } from "react";
import { CheckCircle2, ExternalLink, ShieldAlert } from "lucide-react";
import { core, type DesktopPermissions } from "./bridge";

export function PermissionPanel({ onError }: { onError: (message: string) => void }) {
  const [status, setStatus] = useState<DesktopPermissions>();
  const [busy, setBusy] = useState(false);
  const errorRef = useRef(onError);
  errorRef.current = onError;
  useEffect(() => {
    let live = true;
    const refresh = () => core.desktopPermissions().then(next => live && setStatus(next)).catch(error => live && errorRef.current(String(error)));
    refresh();
    const timer = window.setInterval(refresh, 2000);
    return () => { live = false; window.clearInterval(timer); };
  }, []);
  const request = async () => {
    setBusy(true);
    try { await core.requestScreenRecording(); setStatus(await core.desktopPermissions()); }
    catch (error) { onError(String(error)); }
    finally { setBusy(false); }
  };
  const open = (kind: "screen" | "accessibility") => core.openPrivacySettings(kind).catch(error => onError(String(error)));
  return <article className="panel permission-panel" aria-label="Desktop permissions"><div className="panel-body">
    <div className="section-header"><h2>Desktop permissions</h2><span className="helper">{status?.platform ?? "Checking…"}</span></div>
    {status?.platform === "macOS" ? <>
      <p className="helper">Enable EasyControl in System Settings → Privacy &amp; Security. Restart the app after granting Screen Recording. The QA app has separate permission grants.</p>
      {([ ["Screen Recording", status.screenRecording, "screen"], ["Accessibility", status.accessibility, "accessibility"] ] as const).map(([label, value, kind]) => <div className="setting-row" key={kind}><div><strong>{value === "granted" ? <CheckCircle2 size={16}/> : <ShieldAlert size={16}/>} {label}</strong><small>{value === "granted" ? "Granted" : kind === "screen" ? "Required to share this display" : "Required to approve remote mouse and keyboard control"}</small></div>{value !== "granted" && <div className="inline-actions">{kind === "screen" && <button className="button" disabled={busy} onClick={request}>Request</button>}<button className="button ghost" onClick={() => open(kind)}>Open Settings <ExternalLink size={14}/></button></div>}</div>)}
      <p className="helper">Input Monitoring is not required. EasyControl does not record global keystrokes.</p>
    </> : <p className="helper">{status ? "Screen capture and input permissions depend on your desktop environment." : "Checking screen capture and input permissions…"}</p>}
  </div></article>;
}
