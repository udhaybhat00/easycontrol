import { useEffect, useRef, useState } from "react";
import { MonitorUp, Power } from "lucide-react";
import { core, type HostSessionStatus, type Permissions } from "./bridge";

export function HostPanel({ permissions, onPermissions, onDisconnect, onError }: {
  permissions: Permissions; onPermissions: (permissions: Permissions) => Promise<void>;
  onDisconnect: () => void; onError: (message: string) => void;
}) {
  const [host, setHost] = useState<HostSessionStatus>();
  const [busy, setBusy] = useState(false);
  const callbacks = useRef({ onError, onPermissions });
  callbacks.current = { onError, onPermissions };
  useEffect(() => {
    let live = true;
    const refresh = () => core.hostSessionStatus().then(next => live && setHost(next)).catch(error => live && callbacks.current.onError(String(error)));
    refresh();
    const timer = window.setInterval(refresh, 1000);
    return () => { live = false; window.clearInterval(timer); };
  }, []);
  if (!host?.active) return null;
  const toggle = async () => {
    setBusy(true);
    try {
      await callbacks.current.onPermissions({ ...permissions, input: !host.remoteInput });
      setHost(await core.hostSessionStatus());
    } finally { setBusy(false); }
  };
  return <article className="panel host-panel section" aria-label="Host session">
    <div className="panel-body">
      <div className="section-header"><h2><MonitorUp size={18}/> Remote user connected</h2><button className="button danger" onClick={onDisconnect}><Power size={16}/>Disconnect</button></div>
      <p className="helper">Authenticated session · {host.screenSharing ? "Screen sharing is active" : "Waiting for screen capture. Check macOS permissions if no image appears."}</p>
      <div className="setting-row"><div><strong>Remote input</strong><small>{host.remoteInput ? "Mouse and keyboard approved. Revoke access at any time." : "View only. Approve mouse and keyboard control for this session."}</small></div><button className={`button ${host.remoteInput ? "danger" : "primary"}`} disabled={busy} onClick={toggle}>{busy ? "Applying…" : host.remoteInput ? "Revoke control" : "Approve control"}</button></div>
      <p className="helper">Clipboard: {host.clipboard ? "Allowed" : "Denied"} · File transfer: {host.fileTransfer ? "Allowed" : "Denied"}</p>
    </div>
  </article>;
}
