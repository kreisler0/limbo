import { describe, expect, it } from 'vitest';
import { solveSpring, springs } from './motion';

describe('solveSpring', () => {
  it('starts at 0, ends exactly at 1 and settles', () => {
    const s = solveSpring(260, 30);
    expect(s.ease(0)).toBe(0);
    expect(s.ease(1)).toBe(1);
    expect(s.duration).toBeGreaterThan(100);
    expect(s.duration).toBeLessThan(1500);
    expect(s.css.startsWith('linear(0, ')).toBe(true);
    expect(s.css.endsWith(', 1)')).toBe(true);
  });

  it('snappy settles faster than gentle', () => {
    expect(springs.snappy.duration).toBeLessThan(springs.gentle.duration);
  });

  it('under-damped springs overshoot a little, never wildly', () => {
    const s = solveSpring(400, 20);
    let peak = 0;
    for (let i = 0; i <= 100; i++) peak = Math.max(peak, s.ease(i / 100));
    expect(peak).toBeGreaterThan(1);
    expect(peak).toBeLessThan(1.3);
  });
});
