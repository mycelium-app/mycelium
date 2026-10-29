export type Route = { mode: 'edit'; artifactId: string; viewOnly: false } | { mode: 'view'; viewOnly: true } | { mode: 'none'; viewOnly: false };

const artifactIdPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function parseRoute(pathname: string, search: string): Route
{
  if (new URLSearchParams(search).get('view') === '1')
  {
    return { mode: 'view', viewOnly: true };
  }

  const match = /^\/design\/([^/]+)\/?$/.exec(pathname);
  const artifactId = match ? decodeURIComponent(match[1]) : '';

  return artifactIdPattern.test(artifactId) ? { mode: 'edit', artifactId, viewOnly: false } : { mode: 'none', viewOnly: false };
}
