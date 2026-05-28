// Signature sound that plays mid-flight. Matches the upstream open-source build:
// only the duck "quack" sample ships. Extra sound packs are an upstream Pro
// feature; this fork does not duplicate them.
import quackUrl from "@assets/overlay/quack.wav";

export type SoundPack = { id: string; name: string };

export const SOUNDS: SoundPack[] = [{ id: "quack", name: "Duck" }];

export function soundById(id: string | undefined): SoundPack {
  return SOUNDS.find((s) => s.id === id) ?? SOUNDS[0];
}

const URLS: Record<string, string> = { quack: quackUrl };

const buffers: Record<string, AudioBuffer> = {};
const loading: Record<string, Promise<void>> = {};

function load(ctx: AudioContext, id: string): Promise<void> {
  if (buffers[id]) return Promise.resolve();
  const url = URLS[id];
  if (!url) return Promise.resolve();
  if (!loading[id]) {
    loading[id] = fetch(url)
      .then((r) => r.arrayBuffer())
      .then((buf) => ctx.decodeAudioData(buf))
      .then((decoded) => {
        buffers[id] = decoded;
      })
      .catch(() => {
        delete loading[id];
      });
  }
  return loading[id];
}

export function preloadSounds(ctx: AudioContext): void {
  void load(ctx, "quack");
}

export function playSound(ctx: AudioContext, id: string, when?: number, volume = 0.9): void {
  const at = when ?? ctx.currentTime;
  void load(ctx, id).then(() => {
    const buffer = buffers[id];
    if (!buffer) return;
    const src = ctx.createBufferSource();
    src.buffer = buffer;
    const gain = ctx.createGain();
    gain.gain.value = volume;
    src.connect(gain).connect(ctx.destination);
    src.start(Math.max(at, ctx.currentTime));
  });
}
