export type ArtifactAnnouncement = { kind: 'artifact'; artifactId: string; version: number; deleted: boolean };
export type CommentAnnouncement = { kind: 'comment'; artifactId: string };
export type Announcement = ArtifactAnnouncement | CommentAnnouncement;

export function parseAnnouncement(raw: string, artifactId: string): Announcement | null
{
  let message: Record<string, unknown>;

  try
  {
    message = JSON.parse(raw);
  }
  catch
  {
    return null;
  }

  if (message.type === 'artifact' && message.id === artifactId)
  {
    return { kind: 'artifact', artifactId, version: Number(message.latest_version ?? 0), deleted: message.deleted === true };
  }

  if (message.type === 'artifact_comment' && message.artifact_id === artifactId)
  {
    return { kind: 'comment', artifactId };
  }

  return null;
}

export function streamUrl(location: { protocol: string; host: string }): string
{
  const scheme = location.protocol === 'https:' ? 'wss:' : 'ws:';

  return `${scheme}//${location.host}/api/v1/stream`;
}
