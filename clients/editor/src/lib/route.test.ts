import { describe, expect, it } from 'vitest';
import { parseRoute } from './route';

describe('routing', () =>
{
  it('opens an artifact for editing from its path', () =>
  {
    expect(parseRoute('/design/6f1c2a34-9b7d-4e55-8a10-3c2d1e0f4b66', '')).toEqual({ mode: 'edit', artifactId: '6f1c2a34-9b7d-4e55-8a10-3c2d1e0f4b66', viewOnly: false });
  });

  it('reads view only mode from the query, whatever the path', () =>
  {
    expect(parseRoute('/', '?view=1')).toEqual({ mode: 'view', viewOnly: true });
  });

  it('has nothing to open without an artifact id', () =>
  {
    expect(parseRoute('/design/', '')).toEqual({ mode: 'none', viewOnly: false });
  });

  it('has nothing to open when the path segment is not an artifact id', () =>
  {
    expect(parseRoute('/design/..%2F..%2Fme', '')).toEqual({ mode: 'none', viewOnly: false });
    expect(parseRoute('/design/abc-123', '')).toEqual({ mode: 'none', viewOnly: false });
  });
});
