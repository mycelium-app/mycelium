export type Tool = 'select' | 'hand' | 'text' | 'artboard' | 'note' | 'pen' | 'rect' | 'ellipse' | 'comment';
export type Command = { kind: 'tool'; tool: Tool } | { kind: 'delete' | 'undo' | 'redo' | 'duplicate' | 'fit' | 'zoomIn' | 'zoomOut' };
export type KeyInput = { key: string; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean; altKey: boolean };

const toolKeys: Record<string, Tool> = { v: 'select', h: 'hand', t: 'text', f: 'artboard', n: 'note', p: 'pen', r: 'rect', o: 'ellipse', c: 'comment' };

export function commandForKey(input: KeyInput): Command | null
{
  const key = input.key.toLowerCase();
  const modifier = input.ctrlKey || input.metaKey;

  if (modifier && key === 'z')
  {
    return { kind: input.shiftKey ? 'redo' : 'undo' };
  }

  if (modifier && key === 'y') return { kind: 'redo' };
  if (modifier && key === 'd') return { kind: 'duplicate' };
  if (modifier || input.altKey) return null;
  if (key === 'delete' || key === 'backspace') return { kind: 'delete' };
  if (key === '0') return { kind: 'fit' };
  if (key === '+' || key === '=') return { kind: 'zoomIn' };
  if (key === '-' || key === '_') return { kind: 'zoomOut' };

  const tool = toolKeys[key];

  return tool ? { kind: 'tool', tool } : null;
}
