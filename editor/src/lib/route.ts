export type Route = { mode: 'edit'; artifactId: string; viewOnly: false } | { mode: 'view'; viewOnly: true } | { mode: 'none'; viewOnly: false };

export function parseRoute(pathname: string, search: string): Route
{
  if (new URLSearchParams(search).get('view') === '1')
  {
    return { mode: 'view', viewOnly: true };
  }

  const match = /^\/design\/([^/]+)\/?$/.exec(pathname);

  return match ? { mode: 'edit', artifactId: decodeURIComponent(match[1]), viewOnly: false } : { mode: 'none', viewOnly: false };
}
