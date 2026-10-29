import { type Artifact, ApiError, getArtifact, openStream } from './api';
import type { Series } from './kinds';

export type Saved = 'saved' | 'saving' | 'unsaved' | 'error';

export type TypstPreview = { compiled: boolean; pages: string[]; diagnostics: { line: number; column: number; message: string }[] };
export type TypstLatest = { latest_version: number; latest_compiled: boolean; compiled_version: number | null; compiled_at: string | null; pages: string[]; diagnostics: TypstPreview['diagnostics'] };

const saveDelayMillis = 1200;

async function request(path: string, init?: RequestInit): Promise<Response>
{
  const response = await fetch(path, { credentials: 'same-origin', ...init });

  if (!response.ok)
  {
    let message = `${init?.method ?? 'GET'} ${path} failed with ${response.status}`;

    try
    {
      message = (await response.clone().json()).message ?? message;
    }
    catch
    {
      // The answer was not JSON, so the generic message stands.
    }

    throw new ApiError(response.status, message);
  }

  return response;
}

export async function loadText(artifact: Artifact): Promise<string>
{
  const response = await request(`/api/v1/artifacts/${artifact.id}/versions/${artifact.latest_version}/content`);

  return response.text();
}

export async function loadKind(id: string): Promise<{ artifact: Artifact; text: string }>
{
  const artifact = await getArtifact(id);

  return { artifact, text: await loadText(artifact) };
}

export async function saveText(id: string, text: string): Promise<Artifact>
{
  const response = await request(`/api/v1/artifacts/${id}/versions`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ content: text }) });

  return response.json();
}

export function followVersions(id: string, known: () => number, onNewer: () => void): () => void
{
  return openStream(id, (announcement) =>
  {
    if (announcement.kind === 'artifact' && !announcement.deleted && announcement.version > known())
    {
      onNewer();
    }
  });
}

export function debounced(action: () => void, delay = saveDelayMillis): { run: () => void; flush: () => void; cancel: () => void }
{
  let timer: ReturnType<typeof setTimeout> | undefined;

  return {
    run: () =>
    {
      clearTimeout(timer);
      timer = setTimeout(action, delay);
    },
    flush: () =>
    {
      clearTimeout(timer);
      action();
    },
    cancel: () => clearTimeout(timer),
  };
}

export async function compileTypst(source: string): Promise<TypstPreview>
{
  const response = await request('/api/v1/typst/compile', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ source }) });

  return response.json();
}

export async function latestTypst(id: string): Promise<TypstLatest>
{
  const response = await request(`/api/v1/artifacts/${id}/typst`);

  return response.json();
}

export async function panelData(id: string, panel: number, range: string): Promise<Series[]>
{
  const response = await request(`/api/v1/artifacts/${id}/panels/${panel}/data?range=${range}`);
  const body = await response.json();

  return (body.series as { name: string; points: [number, number][] }[]) ?? [];
}

export async function listArtifacts(): Promise<Artifact[]>
{
  const response = await request('/api/v1/artifacts');

  return response.json();
}

export function svgImage(svg: string): string
{
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

export function failureText(error: unknown): string
{
  return error instanceof ApiError ? error.message : 'Something went wrong. Reload the page to try again.';
}
