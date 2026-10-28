import { type DesignDoc, designMediaType, parseDesign, serialiseDesign } from './doc';
import { parseAnnouncement, streamUrl, type Announcement } from './stream';

export type ArtifactVersion = { version: number; author: 'agent' | 'person'; media_type: string; note: string | null; created_at: string };
export type Artifact = {
  id: string;
  kind: string;
  title: string;
  slug: string;
  public: boolean;
  latest_version: number;
  url: string | null;
  design_system_id?: string;
  versions?: ArtifactVersion[];
};
export type CommentAnchor = { page: string; item: string | null; x: number; y: number };
export type ArtifactComment = { id: string; artifact_id: string; author_name: string; body: string; anchor: CommentAnchor | null; resolved: boolean; created_at: string };

export class ApiError extends Error
{
  constructor(public status: number, message: string)
  {
    super(message);
  }
}

async function request(path: string, init?: RequestInit): Promise<Response>
{
  const response = await fetch(path, { credentials: 'same-origin', ...init });

  if (!response.ok)
  {
    throw new ApiError(response.status, `${init?.method ?? 'GET'} ${path} failed with ${response.status}`);
  }

  return response;
}

function jsonInit(method: string, body: unknown): RequestInit
{
  return { method, headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) };
}

export async function getArtifact(id: string): Promise<Artifact>
{
  const response = await request(`/api/v1/artifacts/${id}`);

  return response.json();
}

export async function getDesign(id: string, version: number): Promise<DesignDoc>
{
  const response = await request(`/api/v1/artifacts/${id}/versions/${version}/content`);

  return parseDesign(await response.text());
}

export async function saveVersion(id: string, doc: DesignDoc, note?: string): Promise<Artifact>
{
  const response = await request(`/api/v1/artifacts/${id}/versions`, jsonInit('POST', { content: serialiseDesign(doc), media_type: designMediaType, note }));

  return response.json();
}

export async function renameArtifact(id: string, title: string): Promise<Artifact>
{
  const response = await request(`/api/v1/artifacts/${id}`, jsonInit('PATCH', { title }));

  return response.json();
}

export async function listComments(id: string): Promise<ArtifactComment[]>
{
  const response = await request(`/api/v1/artifacts/${id}/comments`);

  return response.json();
}

export async function postComment(id: string, body: string, anchor: CommentAnchor): Promise<ArtifactComment>
{
  const response = await request(`/api/v1/artifacts/${id}/comments`, jsonInit('POST', { body, anchor }));

  return response.json();
}

export async function resolveComment(id: string, commentId: string, resolved: boolean): Promise<ArtifactComment>
{
  const response = await request(`/api/v1/artifacts/${id}/comments/${commentId}`, jsonInit('PATCH', { resolved }));

  return response.json();
}

export async function getViewOnlyDesign(): Promise<{ title: string; doc: DesignDoc }>
{
  const response = await request('/_artifact.json');
  const body = await response.json();
  const content = typeof body.content === 'string' ? body.content : null;
  const design = body.design ?? (Array.isArray(body.pages) ? body : null);
  const doc = content !== null ? parseDesign(content) : parseDesign(JSON.stringify(design));

  return { title: body.title ?? 'Design', doc };
}

export function openStream(artifactId: string, onAnnouncement: (announcement: Announcement) => void): () => void
{
  let socket: WebSocket | null = null;
  let closed = false;
  let retry: ReturnType<typeof setTimeout> | undefined;

  const connect = () =>
  {
    socket = new WebSocket(streamUrl(location));
    socket.onmessage = (event) =>
    {
      const announcement = typeof event.data === 'string' ? parseAnnouncement(event.data, artifactId) : null;

      if (announcement)
      {
        onAnnouncement(announcement);
      }
    };
    socket.onclose = () =>
    {
      if (!closed)
      {
        retry = setTimeout(connect, 3000);
      }
    };
  };

  connect();

  return () =>
  {
    closed = true;
    clearTimeout(retry);
    socket?.close();
  };
}
