import { useEffect, useState } from "react";
import { MonitorUp, Power, ShieldCheck } from "lucide-react";
import { core, HostSessionStatus, Permissions } from "./bridge";

export function HostPanel({ permissions, onPermissions, onDisconnect, onError }: { permissions: Permissions; onPermissions: (permissions: Permissions) => void; onDisconnect: () => void; onError: (message: string) => void }) {
  const [host, setHost] = useState<HostSessionStatus>();
  useEffect(() => { let active = true; const refresh = () => core.hostSessionStatus().then(status => active && setHost(status)).catch(error => active && onError(String(error))); refresh(); const timer = window.setInterval(refresh, 1000); return () => { active = false; window.clearInterval(timer); }; }, [onError]);
  if (!host?.active) return null;
  const setInput = (input: boolean) => onPermissions({ ...permissions, input });
  return <article className="card host-panel"><div className="card-title"><MonitorUp size={18}/><span>Host session</span><em>Authenticated peer</em></div><div className="host-status"><span><ShieldCheck size={16}/>Screen sharing active</span><span>{host.peer ?? "Peer identity is not exposed by protocol v5"}</span></div><div className="host-permissions"><label><span><strong>Remote input</strong><small>{host.remoteInput ? "Approved — revoke at any time" : "Denied by host"}</small></span><button className={host.remoteInput ? "switch on" : "switch"} aria-label="Toggle remote input" onClick={() => setInput(!host.remoteInput)}><i/></button></label><label><span><strong>Clipboard</strong><small>{host.clipboard ? "Enabled without UI" : "Denied"}</small></span><span>{host.clipboard ? "Enabled" : "Denied"}</span></label><label><span><strong>File transfer</strong><small>{host.fileTransfer ? "Enabled without UI" : "Denied"}</small></span><span>{host.fileTransfer ? "Enabled" : "Denied"}</span></label></div><button className="secondary danger" onClick={onDisconnect}><Power size={15}/>Disconnect peer</button></article>;
}
