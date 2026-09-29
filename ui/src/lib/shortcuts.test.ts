import { describe, expect, it } from 'vitest';
import { classify, HOST_ACTIONS, LABELS } from './shortcuts';
import type { Shortcut } from './types';

const key = (k: string, mods: Partial<Pick<KeyboardEvent, 'ctrlKey' | 'shiftKey' | 'altKey'>> = {}) =>
  new KeyboardEvent('keydown', { key: k, ...mods });

describe('classify', () => {
  it('maps the common shortcuts', () => {
    expect(classify(key('t', { ctrlKey: true }))).toBe('newTab');
    expect(classify(key('T', { ctrlKey: true, shiftKey: true }))).toBe('reopenClosedTab');
    expect(classify(key('l', { ctrlKey: true }))).toBe('focusOmnibox');
    expect(classify(key('d', { altKey: true }))).toBe('focusOmnibox');
    expect(classify(key('k', { ctrlKey: true }))).toBe('commandPalette');
    expect(classify(key('3', { ctrlKey: true }))).toBe('tab3');
    expect(classify(key('9', { ctrlKey: true }))).toBe('tabLast');
    expect(classify(key('M', { ctrlKey: true, shiftKey: true }))).toBe('memoryPopover');
    expect(classify(key('ArrowLeft', { altKey: true }))).toBe('back');
    expect(classify(key('F3', { shiftKey: true }))).toBe('findPrev');
    expect(classify(key('Escape'))).toBe('escape');
  });

  it('ignores plain typing and editing keys', () => {
    for (const k of ['a', 'Enter', 'Backspace', 'ArrowDown']) expect(classify(key(k))).toBeNull();
    for (const k of ['a', 'c', 'v', 'x', 'z']) expect(classify(key(k, { ctrlKey: true }))).toBeNull();
  });
});

describe('HOST_ACTIONS', () => {
  it('matches the host (src-tauri/src/shortcuts.rs handled_by_ui is its complement)', () => {
    const ui: Shortcut[] = [
      'focusOmnibox', 'commandPalette', 'find', 'findNext', 'findPrev', 'bookmark', 'history', 'downloads',
      'toggleBookmarksBar', 'toggleSidebar', 'tabOverview', 'memoryPopover', 'clearBrowsingData', 'escape',
    ];
    for (const s of ui) expect(HOST_ACTIONS.has(s)).toBe(false);
    expect(HOST_ACTIONS.size + ui.length).toBe(37);
  });

  it('labels every shortcut shown in menus', () => {
    expect(LABELS.newTab).toBe('Ctrl+T');
    expect(LABELS.memoryPopover).toBe('Ctrl+Shift+M');
  });
});
