export type ViewerFrameStats = {
  fps: number; dropped: number; width: number; height: number; bytesPerSecond: number;
  state: "waiting" | "live" | "stalled"; frameAgeMs?: number;
};

// Metadata only. A stalled producer must not leave an old FPS value on screen.
// Sampling is driven by a clock, independently of frame arrival.
export class FrameMeter {
  private frames = 0;
  private bytes = 0;
  private width = 0;
  private height = 0;
  private dropped = 0;
  private lastFrameAt?: number;
  constructor(private windowStarted: number) {}

  receive(frame: { width: number; height: number; dropped: number }, bytes: number, now: number) {
    this.frames++;
    this.bytes += bytes;
    this.width = frame.width; this.height = frame.height; this.dropped = frame.dropped;
    this.lastFrameAt = now;
  }

  sample(now: number): ViewerFrameStats {
    const elapsed = Math.max(1, now - this.windowStarted);
    const frameAgeMs = this.lastFrameAt === undefined ? undefined : Math.max(0, now - this.lastFrameAt);
    const stats: ViewerFrameStats = {
      fps: Math.round(this.frames * 1000 / elapsed), bytesPerSecond: this.bytes * 1000 / elapsed,
      width: this.width, height: this.height, dropped: this.dropped, frameAgeMs,
      state: frameAgeMs === undefined ? "waiting" : frameAgeMs >= 3000 ? "stalled" : "live",
    };
    this.frames = 0; this.bytes = 0; this.windowStarted = now;
    return stats;
  }
}
