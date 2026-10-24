export type ThemeName = 'dark' | 'light';

type AccentColours = { dark: string; light: string };

const accents: Record<string, AccentColours> = {
  neutral: { dark: '#e6e6e6', light: '#171717' },
  blue: { dark: '#8ab8f0', light: '#1f58a8' },
  violet: { dark: '#b9a3f2', light: '#5b3fb0' },
  amber: { dark: '#e6bc6c', light: '#7a5000' },
  rose: { dark: '#ee93ae', light: '#a32f57' },
  green: { dark: '#7ccba0', light: '#1b6a44' },
};

export type ThemeResult = { theme: ThemeName; variables: Record<string, string> };

function channel(value: number): number {
  const unit = value / 255;

  return unit <= 0.03928 ? unit / 12.92 : Math.pow((unit + 0.055) / 1.055, 2.4);
}

function luminance(hex: string): number {
  const red = parseInt(hex.slice(1, 3), 16);
  const green = parseInt(hex.slice(3, 5), 16);
  const blue = parseInt(hex.slice(5, 7), 16);

  return 0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue);
}

export function themeFromQuery(search: string, systemDark: boolean): ThemeResult {
  const params = new URLSearchParams(search);
  const requested = params.get('theme');
  const theme: ThemeName = requested === 'dark' || requested === 'light' ? requested : systemDark ? 'dark' : 'light';
  const named = accents[(params.get('accent') ?? '').toLowerCase()] ?? accents.neutral;
  const accent = named[theme];
  const onAccent = luminance(accent) > 0.4 ? '#111111' : '#ffffff';

  return { theme, variables: { '--accent': accent, '--focus': accent, '--on-accent': onAccent } };
}
