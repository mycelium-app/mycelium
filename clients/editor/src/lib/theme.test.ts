import { describe, expect, it } from 'vitest';
import { themeFromQuery } from './theme';

describe('themeFromQuery', () => {
  it('follows the system preference and the neutral accent by default', () => {
    expect(themeFromQuery('', true)).toEqual({
      theme: 'dark',
      variables: { '--accent': '#e6e6e6', '--focus': '#e6e6e6', '--on-accent': '#111111' },
    });
    expect(themeFromQuery('', false).theme).toBe('light');
  });

  it('lets the theme parameter override the system preference', () => {
    expect(themeFromQuery('?theme=light', true).theme).toBe('light');
    expect(themeFromQuery('?theme=dark', false).theme).toBe('dark');
  });

  it('ignores an unknown theme', () => {
    expect(themeFromQuery('?theme=sepia', true).theme).toBe('dark');
  });

  it('picks the accent value for the active theme, case-insensitively', () => {
    expect(themeFromQuery('?theme=dark&accent=blue', false).variables['--accent']).toBe('#8ab8f0');
    expect(themeFromQuery('?theme=light&accent=Blue', true).variables['--accent']).toBe('#1f58a8');
  });

  it('falls back to neutral for an unknown accent', () => {
    expect(themeFromQuery('?theme=light&accent=plaid', true).variables['--accent']).toBe('#171717');
  });

  it('uses white text on dark accents and dark text on light ones', () => {
    expect(themeFromQuery('?theme=light&accent=violet', true).variables['--on-accent']).toBe('#ffffff');
    expect(themeFromQuery('?theme=dark&accent=amber', false).variables['--on-accent']).toBe('#111111');
  });
});
