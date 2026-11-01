export type RecentSession = {
  id: string; address: string; started: number; ended?: number;
  result: "Authenticated" | "Completed" | "Failed";
};

// Only authenticated sessions are recorded. Completing an unknown or already
// completed ID must not change another session's history or duration.
export function completeSession(history: RecentSession[], id: string, ended: number): RecentSession[] {
  return history.map(row => row.id === id && row.result === "Authenticated"
    ? { ...row, result: "Completed", ended: Math.max(row.started, ended) }
    : row);
}

export function sessionDuration(row: RecentSession): string {
  if (row.ended === undefined) return row.result === "Authenticated" ? "—" : "Not recorded";
  const seconds = Math.max(0, Math.floor((row.ended - row.started) / 1000));
  return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}
