import { describe, expect, it } from 'vitest';
import { parseRoute } from './route';

describe('routing', () =>
{
  it('opens an artifact for editing from its path', () =>
  {
    expect(parseRoute('/design/abc-123', '')).toEqual({ mode: 'edit', artifactId: 'abc-123', viewOnly: false });
  });

  it('reads view only mode from the query, whatever the path', () =>
  {
    expect(parseRoute('/', '?view=1')).toEqual({ mode: 'view', viewOnly: true });
  });

  it('has nothing to open without an artifact id', () =>
  {
    expect(parseRoute('/design/', '')).toEqual({ mode: 'none', viewOnly: false });
  });
});
