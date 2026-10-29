import { describe, expect, it } from 'vitest';
import { parseAnnouncement, streamUrl } from './stream';

describe('stream announcements', () =>
{
  it('announces a new version of the open artifact', () =>
  {
    const raw = JSON.stringify({ type: 'artifact', id: 'a', latest_version: 4 });

    expect(parseAnnouncement(raw, 'a')).toEqual({ kind: 'artifact', artifactId: 'a', version: 4, deleted: false });
  });

  it('announces deletion', () =>
  {
    const raw = JSON.stringify({ type: 'artifact', id: 'a', latest_version: 4, deleted: true });

    expect(parseAnnouncement(raw, 'a')).toMatchObject({ deleted: true });
  });

  it('announces a comment on the open artifact', () =>
  {
    expect(parseAnnouncement(JSON.stringify({ type: 'artifact_comment', artifact_id: 'a' }), 'a')).toEqual({ kind: 'comment', artifactId: 'a' });
  });

  it('ignores other artifacts, other messages and malformed text', () =>
  {
    expect(parseAnnouncement(JSON.stringify({ type: 'artifact', id: 'b', latest_version: 1 }), 'a')).toBeNull();
    expect(parseAnnouncement(JSON.stringify({ type: 'conversation' }), 'a')).toBeNull();
    expect(parseAnnouncement('not json', 'a')).toBeNull();
  });

  it('uses a secure socket on https', () =>
  {
    expect(streamUrl({ protocol: 'https:', host: 'm.example' })).toBe('wss://m.example/api/v1/stream');
    expect(streamUrl({ protocol: 'http:', host: 'localhost:8080' })).toBe('ws://localhost:8080/api/v1/stream');
  });
});
