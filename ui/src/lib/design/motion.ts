// Spring motion (plan 7.3). Springs are solved once at startup into CSS
// `linear()` easings (CSS transitions) and sampled easing functions (Svelte
// transitions / Web Animations). Only transform and opacity are animated.

import type { TransitionConfig } from 'svelte/transition';

export interface Spring {
  /** CSS easing: linear(0, ..., 1). */
  css: string;
  /** Milliseconds until settled. */
  duration: number;
  /** Easing function for JS-driven animation. */
  ease: (t: number) => number;
}

/** Simulates a unit spring (mass 1) from 0 to 1 and samples it. */
export function solveSpring(stiffness: number, damping: number, samples = 48): Spring {
  const dt = 1 / 600;
  let x = 0;
  let v = 0;
  let t = 0;
  const trace: number[] = [0];
  // Settle when within 0.1% and nearly still (cap 1.5 s).
  while (t < 1.5) {
    const a = -stiffness * (x - 1) - damping * v;
    v += a * dt;
    x += v * dt;
    t += dt;
    trace.push(x);
    if (Math.abs(x - 1) < 0.001 && Math.abs(v) < 0.01) break;
  }
  const n = trace.length - 1;
  const at = (p: number) => {
    const f = p * n;
    const i = Math.floor(f);
    if (i >= n) return 1;
    return trace[i] + (trace[i + 1] - trace[i]) * (f - i);
  };
  const points: string[] = [];
  for (let i = 0; i <= samples; i++) {
    const y = i === samples ? 1 : at(i / samples);
    points.push(String(Math.round(y * 10000) / 10000));
  }
  return { css: `linear(${points.join(', ')})`, duration: Math.round(t * 1000), ease: at };
}

export const springs = {
  /** Hover, press, toggles. */
  snappy: solveSpring(400, 32),
  /** Panels, popovers, tab open/close. */
  smooth: solveSpring(260, 30),
  /** Page-level transitions, the new-tab reveal. */
  gentle: solveSpring(170, 26),
};

export type SpringName = keyof typeof springs;

const reduced = typeof matchMedia === 'function' ? matchMedia('(prefers-reduced-motion: reduce)') : null;

export function prefersReducedMotion(): boolean {
  return !!reduced?.matches;
}

/** Exposes the springs as CSS custom properties (--ease-*, --dur-*). */
export function installMotionTokens(root: HTMLElement = document.documentElement) {
  const apply = () => {
    for (const [name, s] of Object.entries(springs)) {
      root.style.setProperty(`--ease-${name}`, prefersReducedMotion() ? 'ease-out' : s.css);
      root.style.setProperty(`--dur-${name}`, `${prefersReducedMotion() ? 120 : s.duration}ms`);
    }
  };
  apply();
  reduced?.addEventListener('change', apply);
}

interface PopOptions {
  spring?: SpringName;
  /** Transform origin, e.g. "top right" (the anchor point). */
  origin?: string;
  /** Starting scale. */
  from?: number;
  /** Starting translateY in px. */
  y?: number;
  delay?: number;
}

/**
 * Scale-and-fade from the anchor. Exit is 30% faster than enter.
 * With reduced motion: a 120 ms opacity fade.
 */
export function pop(node: Element, opts: PopOptions = {}, direction: { direction?: 'in' | 'out' | 'both' } = {}): TransitionConfig {
  const s = springs[opts.spring ?? 'smooth'];
  const out = direction.direction === 'out';
  if (prefersReducedMotion()) {
    return { duration: 120, css: (t) => `opacity: ${t}` };
  }
  (node as HTMLElement).style.transformOrigin = opts.origin ?? 'top center';
  const from = opts.from ?? 0.96;
  const y = opts.y ?? 0;
  return {
    delay: opts.delay ?? 0,
    duration: out ? Math.round(s.duration * 0.7) : s.duration,
    easing: out ? (t) => t : s.ease,
    css: (t) => {
      const scale = from + (1 - from) * t;
      const ty = y * (1 - t);
      const opacity = Math.min(1, t * 1.6);
      return `opacity: ${opacity}; transform: translateY(${ty}px) scale(${scale})`;
    },
  };
}

/** Plain fade (backdrops, snapshots). */
export function fade(_node: Element, opts: { duration?: number } = {}): TransitionConfig {
  const duration = prefersReducedMotion() ? 120 : (opts.duration ?? 160);
  return { duration, css: (t) => `opacity: ${t}` };
}

/** Slide in from a side (sidebar). */
export function slide(_node: Element, opts: { x?: number; y?: number; spring?: SpringName } = {}): TransitionConfig {
  const s = springs[opts.spring ?? 'smooth'];
  if (prefersReducedMotion()) return { duration: 120, css: (t) => `opacity: ${t}` };
  const x = opts.x ?? 0;
  const y = opts.y ?? 0;
  return {
    duration: s.duration,
    easing: s.ease,
    css: (t, u) => `transform: translate(${x * u}px, ${y * u}px); opacity: ${Math.min(1, t * 2)}`,
  };
}

/**
 * FLIP: animates elements from their old positions to their new ones
 * (tab reorder, open, close). Call `measure()` before the DOM changes and
 * `play()` after.
 */
export function flip(getItems: () => HTMLElement[], spring: SpringName = 'smooth') {
  let before = new Map<HTMLElement, DOMRect>();
  return {
    measure() {
      before = new Map(getItems().map((el) => [el, el.getBoundingClientRect()]));
    },
    play() {
      if (prefersReducedMotion()) return;
      const s = springs[spring];
      for (const el of getItems()) {
        const prev = before.get(el);
        if (!prev) continue;
        const now = el.getBoundingClientRect();
        const dx = prev.left - now.left;
        if (Math.abs(dx) < 0.5) continue;
        el.animate([{ transform: `translateX(${dx}px)` }, { transform: 'translateX(0)' }], {
          duration: s.duration,
          easing: s.css,
        });
      }
    },
  };
}

/** Tweens a number toward a target (memory pill), calling `update` per frame. */
export function tween(from: number, to: number, ms: number, update: (v: number) => void): () => void {
  if (prefersReducedMotion() || from === to) {
    update(to);
    return () => {};
  }
  const start = performance.now();
  let raf = 0;
  const step = (now: number) => {
    const p = Math.min(1, (now - start) / ms);
    const e = 1 - Math.pow(1 - p, 3);
    update(from + (to - from) * e);
    if (p < 1) raf = requestAnimationFrame(step);
  };
  raf = requestAnimationFrame(step);
  return () => cancelAnimationFrame(raf);
}
