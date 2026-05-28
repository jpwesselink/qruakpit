import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { themeById } from "@shared/themes";
import { fontById } from "@shared/fonts";
import { playSound, preloadSounds } from "@shared/sounds";
import { planeBaseUrl, headUrl, BLADE_URL } from "@shared/flier-assets";
import type { Flight } from "@shared/types";

/** Propeller-engine drone that fades in/out and pans left to right. */
function startEngine(ctx: AudioContext, durationS: number): void {
  const now = ctx.currentTime;
  const master = ctx.createGain();
  master.gain.setValueAtTime(0.0001, now);
  master.gain.exponentialRampToValueAtTime(0.16, now + 0.8);
  master.gain.setValueAtTime(0.16, Math.max(now + 0.8, now + durationS - 0.8));
  master.gain.exponentialRampToValueAtTime(0.0001, now + durationS);

  const panner = ctx.createStereoPanner();
  panner.pan.setValueAtTime(-0.9, now);
  panner.pan.linearRampToValueAtTime(0.9, now + durationS);
  master.connect(panner).connect(ctx.destination);

  const lp = ctx.createBiquadFilter();
  lp.type = "lowpass";
  lp.frequency.value = 850;

  const wob = ctx.createGain();
  wob.gain.value = 1;
  lp.connect(wob).connect(master);

  const lfo = ctx.createOscillator();
  lfo.type = "sine";
  lfo.frequency.value = 11;
  const lfoGain = ctx.createGain();
  lfoGain.gain.value = 0.07;
  lfo.connect(lfoGain).connect(wob.gain);

  const o1 = ctx.createOscillator();
  o1.type = "sawtooth";
  o1.frequency.value = 92;
  const o2 = ctx.createOscillator();
  o2.type = "sawtooth";
  o2.frequency.value = 92 * 1.012;
  o1.connect(lp);
  o2.connect(lp);

  const stopAt = now + durationS + 0.05;
  o1.start(now);
  o2.start(now);
  lfo.start(now);
  o1.stop(stopAt);
  o2.stop(stopAt);
  lfo.stop(stopAt);
}

export default function OverlayApp() {
  const flyerRef = useRef<HTMLDivElement | null>(null);
  const bannerRef = useRef<HTMLDivElement | null>(null);
  const bannerTextRef = useRef<HTMLSpanElement | null>(null);
  const planeRef = useRef<HTMLImageElement | null>(null);
  const headRef = useRef<HTMLImageElement | null>(null);
  const propRef = useRef<HTMLImageElement | null>(null);
  const audioCtxRef = useRef<AudioContext | null>(null);

  useEffect(() => {
    if (propRef.current) propRef.current.src = BLADE_URL;

    const ensureAudio = (): AudioContext => {
      if (!audioCtxRef.current) audioCtxRef.current = new AudioContext();
      const ctx = audioCtxRef.current;
      if (ctx.state === "suspended") void ctx.resume();
      preloadSounds(ctx);
      return ctx;
    };

    const playFlight = (flight: Flight) => {
      const flyer = flyerRef.current;
      const banner = bannerRef.current;
      const bannerText = bannerTextRef.current;
      const planeImg = planeRef.current;
      const headImg = headRef.current;
      if (!flyer || !banner || !bannerText || !planeImg || !headImg) return;

      bannerText.textContent = flight.message;

      const theme = themeById(flight.theme);
      banner.style.setProperty("--stripe-a", theme.a);
      banner.style.setProperty("--stripe-b", theme.b);
      banner.style.setProperty("--banner-ink", theme.text);

      banner.style.setProperty("--banner-font", fontById(flight.font).stack);

      planeImg.src = planeBaseUrl(flight.color);
      headImg.src = headUrl(flight.head);

      const durationS = Math.max(2, flight.durationMs / 1000);

      flyer.classList.remove("flying");
      void flyer.offsetWidth; // force reflow so the animation can re-trigger
      flyer.style.setProperty("--fly-duration", `${durationS}s`);
      flyer.classList.add("flying");

      if (flight.sound !== false) {
        try {
          const ctx = ensureAudio();
          const start = ctx.currentTime;
          startEngine(ctx, durationS);
          playSound(ctx, flight.soundPack ?? "quack", start + durationS / 2);
        } catch {
          /* audio is best-effort */
        }
      }
    };

    const flyer = flyerRef.current;
    const handleAnimEnd = (e: AnimationEvent) => {
      if (e.animationName === "fly") flyer?.classList.remove("flying");
    };
    flyer?.addEventListener("animationend", handleAnimEnd);

    const unlisten = listen<Flight>("flight:start", (event) => {
      playFlight(event.payload);
    });

    return () => {
      flyer?.removeEventListener("animationend", handleAnimEnd);
      void unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <>
      <svg className="filters" aria-hidden="true" width="0" height="0">
        <defs>
          <filter id="banner-wave" x="-20%" y="-60%" width="140%" height="220%">
            <feTurbulence
              type="fractalNoise"
              baseFrequency="0.011 0.024"
              numOctaves={1}
              seed={7}
              result="noise"
            />
            <feOffset in="noise" dx={0} dy={0} result="noiseShift">
              <animate
                attributeName="dx"
                dur="3.2s"
                values="0;110;0"
                calcMode="spline"
                keyTimes="0;0.5;1"
                keySplines="0.45 0 0.55 1;0.45 0 0.55 1"
                repeatCount="indefinite"
              />
              <animate
                attributeName="dy"
                dur="2.4s"
                values="0;26;0"
                calcMode="spline"
                keyTimes="0;0.5;1"
                keySplines="0.45 0 0.55 1;0.45 0 0.55 1"
                repeatCount="indefinite"
              />
            </feOffset>
            <feDisplacementMap
              in="SourceGraphic"
              in2="noiseShift"
              scale={20}
              xChannelSelector="R"
              yChannelSelector="G"
            >
              <animate
                attributeName="scale"
                dur="2.2s"
                values="16;30;16"
                calcMode="spline"
                keyTimes="0;0.5;1"
                keySplines="0.45 0 0.55 1;0.45 0 0.55 1"
                repeatCount="indefinite"
              />
            </feDisplacementMap>
          </filter>
        </defs>
      </svg>

      <div className="stage">
        <div id="flyer" className="flyer" ref={flyerRef}>
          <div className="banner" ref={bannerRef}>
            <span id="banner-text" ref={bannerTextRef}></span>
          </div>
          <div className="rope" />
          <div className="aircraft">
            <img className="plane" alt="" ref={planeRef} />
            <img className="head" alt="" ref={headRef} />
            <img className="prop" alt="" ref={propRef} />
          </div>
        </div>
      </div>
    </>
  );
}
