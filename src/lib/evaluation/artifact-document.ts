export const artifactPolicy = "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' blob:; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; connect-src 'none'; frame-src 'none'; object-src 'none'; worker-src 'none'; base-uri 'none'; form-action 'none'";

function playbackRuntime(token: string, initiallyPlaying: boolean) {
  let playing = initiallyPlaying;
  let pausedAt = 0;
  let pausedDuration = 0;
  const now = performance.now.bind(performance);
  const frame = requestAnimationFrame.bind(window);
  const cancelFrame = cancelAnimationFrame.bind(window);
  const nativeTimeout = window.setTimeout.bind(window);
  const clear = window.clearTimeout.bind(window);
  const frames = new Map<number, { callback: FrameRequestCallback; native?: number }>();
  const timers = new Map<number, { handler: TimerHandler; args: unknown[]; delay: number; remaining: number; due: number; repeat: boolean; native?: number }>();
  const pausedAnimations = new Set<Animation>();
  const style = document.createElement('style');
  let serial = 0;
  style.textContent = '*,*::before,*::after{animation-play-state:paused!important}';
  const scheduleFrame = (id: number) => {
    const item = frames.get(id);
    if (!item || !playing) return;
    item.native = frame((timestamp) => {
      frames.delete(id);
      item.callback(timestamp - pausedDuration);
    });
  };
  const scheduleTimer = (id: number) => {
    const item = timers.get(id);
    if (!item || !playing) return;
    item.due = now() + item.remaining;
    item.native = nativeTimeout(() => {
      if (!item.repeat) timers.delete(id);
      if (typeof item.handler === 'function') item.handler(...item.args);
      else (0, eval)(item.handler);
      if (item.repeat && timers.has(id)) { item.remaining = item.delay; scheduleTimer(id); }
    }, item.remaining) as unknown as number;
  };
  window.requestAnimationFrame = (callback) => { const id = ++serial; frames.set(id, { callback }); scheduleFrame(id); return id; };
  window.cancelAnimationFrame = (id) => { const item = frames.get(id); if (item?.native != null) cancelFrame(item.native); frames.delete(id); };
  const addTimer = (handler: TimerHandler, delay: number | undefined, args: unknown[], repeat: boolean) => {
    const id = ++serial;
    const duration = Math.max(0, Number(delay) || 0);
    timers.set(id, { handler, args, delay: duration, remaining: duration, due: 0, repeat });
    scheduleTimer(id);
    return id;
  };
  window.setTimeout = ((handler: TimerHandler, delay?: number, ...args: unknown[]) => addTimer(handler, delay, args, false)) as typeof window.setTimeout;
  window.setInterval = ((handler: TimerHandler, delay?: number, ...args: unknown[]) => addTimer(handler, delay, args, true)) as typeof window.setInterval;
  const cancelTimer = (id?: number) => { if (id == null) return; const item = timers.get(id); if (item?.native != null) clear(item.native); timers.delete(id); };
  window.clearTimeout = cancelTimer as typeof window.clearTimeout;
  window.clearInterval = cancelTimer as typeof window.clearInterval;
  const syncVisuals = () => {
    if (playing) style.remove();
    else if (!style.isConnected) document.documentElement.append(style);
    document.querySelectorAll('svg').forEach((svg) => { if (playing) svg.unpauseAnimations(); else svg.pauseAnimations(); });
    if (playing) { for (const animation of pausedAnimations) animation.play(); pausedAnimations.clear(); }
    else for (const animation of document.getAnimations()) { if (animation.playState === 'running') { pausedAnimations.add(animation); animation.pause(); } }
  };
  const setPlaying = (value: boolean) => {
    if (value === playing) { syncVisuals(); return; }
    playing = value;
    if (playing) {
      pausedDuration += now() - pausedAt;
      for (const id of frames.keys()) scheduleFrame(id);
      for (const id of timers.keys()) scheduleTimer(id);
    } else {
      pausedAt = now();
      for (const item of frames.values()) if (item.native != null) cancelFrame(item.native);
      for (const item of timers.values()) if (item.native != null) { clear(item.native); item.remaining = Math.max(0, item.due - pausedAt); }
    }
    syncVisuals();
  };
  if (!playing) pausedAt = now();
  addEventListener('message', (event) => {
    if (event.source !== parent || event.data?.type !== 'vela:artifact-playback' || event.data.token !== token || typeof event.data.playing !== 'boolean') return;
    setPlaying(event.data.playing);
  });
  addEventListener('click', (event) => { if (event.target instanceof Element && event.target.closest('a')) event.preventDefault(); }, true);
  addEventListener('submit', (event) => event.preventDefault(), true);
  addEventListener('DOMContentLoaded', () => {
    syncVisuals();
    parent.postMessage({ type: 'vela:artifact-ready', token }, '*');
  }, { once: true });
}

export function createArtifactDocument(html: string, token: string, playing = true): string {
  const bootstrap = `(${playbackRuntime.toString()})(${JSON.stringify(token)},${JSON.stringify(playing)});`;
  return `<!doctype html><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${artifactPolicy}"><meta name="referrer" content="no-referrer"><script>${bootstrap}</script>\n${html}`;
}
