import * as api from './api';
import type { Artifact, ArtifactComment, ArtifactVersion, CommentAnchor } from './api';
import { type DesignDoc, type Handle, type Item, type ItemType, type Bounds, type Alignment, type Stacking, addItem, addPage, alignItem, designsEqual, duplicateItem, emptyDesign, findItem, itemBounds, moveItem, newItem, presentOrder, removeItem, renamePage, resizeItem, stackItem, updateItem, defaultSizes } from './doc';
import { commit, createHistory, redo, undo } from './history';
import type { Command, Tool } from './keys';
import { hasLocalChanges, mergeIncoming } from './merge';
import { parseRoute } from './route';
import { openStream } from './api';
import { type Viewport, fitBounds, unionBounds, zoomAt, zoomStep } from './viewport';

export type SaveState = 'saved' | 'unsaved' | 'saving' | 'error';

const saveDelayMillis = 1500;

export class Editor
{
  route = parseRoute(location.pathname, location.search);
  loaded = $state(false);
  failure = $state('');
  artifact = $state.raw<Artifact | null>(null);
  title = $state('Design');
  history = $state.raw(createHistory<DesignDoc>(emptyDesign()));
  draft = $state.raw<DesignDoc | null>(null);
  tool = $state<Tool>('select');
  selectedId = $state<string | null>(null);
  editingId = $state<string | null>(null);
  enteredId = $state<string | null>(null);
  pageId = $state('p1');
  viewport = $state.raw<Viewport>({ x: 0, y: 0, zoom: 1 });
  canvasSize = { w: 1200, h: 800 };
  comments = $state.raw<ArtifactComment[]>([]);
  openThread = $state<string | null>(null);
  pendingAnchor = $state.raw<CommentAnchor | null>(null);
  versions = $state.raw<ArtifactVersion[]>([]);
  versionsOpen = $state(false);
  saveState = $state<SaveState>('saved');
  presenting = $state<number | null>(null);
  notice = $state('');

  doc = $derived(this.draft ?? this.history.present);
  page = $derived(this.doc.pages.find((page) => page.id === this.pageId) ?? this.doc.pages[0]);
  selected = $derived(this.selectedId ? findItem(this.doc, this.selectedId) : undefined);
  canUndo = $derived(this.history.past.length > 0);
  canRedo = $derived(this.history.future.length > 0);
  artboards = $derived(presentOrder(this.page));

  private base: DesignDoc = emptyDesign();
  private knownVersion = 0;
  private saveTimer: ReturnType<typeof setTimeout> | undefined;
  private saving = false;
  private closeStream: (() => void) | null = null;

  get viewOnly(): boolean
  {
    return this.route.viewOnly;
  }

  async load(): Promise<void>
  {
    try
    {
      if (this.route.mode === 'view')
      {
        const viewed = await api.getViewOnlyDesign();

        this.title = viewed.title;
        this.adopt(viewed.doc);
      }
      else if (this.route.mode === 'edit')
      {
        const artifact = await api.getArtifact(this.route.artifactId);
        const doc = await api.getDesign(artifact.id, artifact.latest_version);

        this.artifact = artifact;
        this.title = artifact.title;
        this.versions = artifact.versions ?? [];
        this.knownVersion = artifact.latest_version;
        this.adopt(doc);
        this.comments = await api.listComments(artifact.id);
        this.closeStream = openStream(artifact.id, (announcement) => void this.announced(announcement));
      }
      else
      {
        this.failure = 'Open a design from Mycelium to edit it.';
        return;
      }

      this.loaded = true;
    }
    catch (error)
    {
      const status = error instanceof api.ApiError ? error.status : 0;

      this.failure = status === 401 || status === 403 ? 'Sign in to Mycelium to open this design.' : status === 404 ? 'This design does not exist.' : 'The design could not be loaded. Reload the page to try again.';
    }
  }

  dispose(): void
  {
    this.closeStream?.();
    clearTimeout(this.saveTimer);
  }

  private adopt(doc: DesignDoc): void
  {
    this.base = doc;
    this.history = createHistory(doc);
    this.pageId = doc.pages[0].id;
  }

  setCanvasSize(w: number, h: number): void
  {
    this.canvasSize = { w, h };
  }

  fit(): void
  {
    const bounds = unionBounds(this.page.items.map(itemBounds));

    this.viewport = fitBounds(bounds, this.canvasSize);
  }

  zoomBy(factor: number): void
  {
    this.viewport = zoomAt(this.viewport, { x: this.canvasSize.w / 2, y: this.canvasSize.h / 2 }, factor);
  }

  setTool(tool: Tool): void
  {
    if (this.viewOnly && tool !== 'select' && tool !== 'hand') return;

    this.tool = tool;
    this.editingId = null;
    this.enteredId = null;
    this.pendingAnchor = null;
    this.openThread = null;
  }

  selectPage(id: string): void
  {
    this.pageId = id;
    this.selectedId = null;
    this.fit();
  }

  select(id: string | null): void
  {
    this.selectedId = id;
    this.editingId = null;

    if (id !== this.enteredId)
    {
      this.enteredId = null;
    }
  }

  edit(next: DesignDoc): void
  {
    if (this.viewOnly) return;

    this.history = commit(this.history, next);
    this.scheduleSave();
  }

  previewDraft(next: DesignDoc | null): void
  {
    this.draft = this.viewOnly ? null : next;
  }

  finishDraft(): void
  {
    const draft = this.draft;

    this.draft = null;

    if (draft)
    {
      this.edit(draft);
    }
  }

  create(type: ItemType, bounds: Bounds): string
  {
    const item = newItem(this.doc, type, bounds);

    this.edit(addItem(this.doc, this.page.id, item));
    this.selectedId = item.id;

    return item.id;
  }

  createAt(type: Exclude<ItemType, 'path'>, x: number, y: number): string
  {
    const size = defaultSizes[type];
    const id = this.create(type, { x, y, ...size });

    if (type === 'text' || type === 'note')
    {
      this.editingId = id;
    }

    return id;
  }

  createPath(points: [number, number][]): void
  {
    if (points.length < 2) return;

    const item = { ...newItem(this.doc, 'path', { x: 0, y: 0, w: 0, h: 0 }), points } as Item;

    this.edit(addItem(this.doc, this.page.id, item));
    this.selectedId = item.id;
  }

  patchSelected(patch: Partial<Item>): void
  {
    if (this.selectedId)
    {
      this.edit(updateItem(this.doc, this.selectedId, patch));
    }
  }

  patchItem(id: string, patch: Partial<Item>): void
  {
    this.edit(updateItem(this.doc, id, patch));
  }

  moveBy(id: string, dx: number, dy: number): DesignDoc
  {
    return moveItem(this.history.present, id, dx, dy);
  }

  resizeBy(id: string, handle: Handle, dx: number, dy: number): DesignDoc
  {
    return resizeItem(this.history.present, id, handle, dx, dy);
  }

  alignSelected(alignment: Alignment): void
  {
    if (this.selectedId)
    {
      this.edit(alignItem(this.doc, this.selectedId, alignment));
    }
  }

  stackSelected(stacking: Stacking): void
  {
    if (this.selectedId)
    {
      this.edit(stackItem(this.doc, this.selectedId, stacking));
    }
  }

  deleteSelected(): void
  {
    if (!this.selectedId) return;

    this.edit(removeItem(this.doc, this.selectedId));
    this.selectedId = null;
    this.editingId = null;
  }

  duplicateSelected(): void
  {
    if (!this.selectedId) return;

    const duplicated = duplicateItem(this.doc, this.selectedId);

    this.edit(duplicated.doc);
    this.selectedId = duplicated.id;
  }

  addNewPage(): void
  {
    const next = addPage(this.doc);

    this.edit(next);
    this.pageId = next.pages[next.pages.length - 1].id;
    this.selectedId = null;
  }

  renameCurrentPage(name: string): void
  {
    this.edit(renamePage(this.doc, this.page.id, name));
  }

  undo(): void
  {
    this.history = undo(this.history);
    this.afterHistoryMove();
  }

  redo(): void
  {
    this.history = redo(this.history);
    this.afterHistoryMove();
  }

  private afterHistoryMove(): void
  {
    if (this.selectedId && !findItem(this.history.present, this.selectedId))
    {
      this.selectedId = null;
    }

    this.scheduleSave();
  }

  run(command: Command): void
  {
    if (command.kind === 'tool') this.setTool(command.tool);
    else if (command.kind === 'delete') this.deleteSelected();
    else if (command.kind === 'undo') this.undo();
    else if (command.kind === 'redo') this.redo();
    else if (command.kind === 'duplicate') this.duplicateSelected();
    else if (command.kind === 'fit') this.fit();
    else if (command.kind === 'zoomIn') this.zoomBy(zoomStep);
    else if (command.kind === 'zoomOut') this.zoomBy(1 / zoomStep);
  }

  present(artboardId?: string): void
  {
    const index = artboardId ? this.artboards.findIndex((artboard) => artboard.id === artboardId) : 0;

    if (this.artboards.length > 0)
    {
      this.presenting = Math.max(0, index);
    }
  }

  step(direction: number): void
  {
    if (this.presenting === null) return;

    this.presenting = Math.min(this.artboards.length - 1, Math.max(0, this.presenting + direction));
  }

  scheduleSave(): void
  {
    if (this.viewOnly) return;

    this.saveState = 'unsaved';
    clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => void this.save(), saveDelayMillis);
  }

  async save(note?: string): Promise<void>
  {
    const artifact = this.artifact;

    if (this.viewOnly || !artifact) return;

    clearTimeout(this.saveTimer);

    if (this.saving)
    {
      this.saveTimer = setTimeout(() => void this.save(note), saveDelayMillis);
      return;
    }

    const snapshot = this.history.present;

    if (designsEqual(snapshot, this.base))
    {
      this.saveState = 'saved';
      return;
    }

    this.saving = true;
    this.saveState = 'saving';

    try
    {
      const saved = await api.saveVersion(artifact.id, snapshot, note);

      this.base = snapshot;
      this.knownVersion = saved.latest_version;
      this.artifact = { ...artifact, ...saved };
      this.versions = saved.versions ?? (await api.getArtifact(artifact.id)).versions ?? this.versions;
      this.saveState = designsEqual(this.history.present, snapshot) ? 'saved' : 'unsaved';
    }
    catch
    {
      this.saveState = 'error';
      this.saveTimer = setTimeout(() => void this.save(note), saveDelayMillis * 4);
    }
    finally
    {
      this.saving = false;
    }
  }

  async restore(version: number): Promise<void>
  {
    const artifact = this.artifact;

    if (this.viewOnly || !artifact) return;

    const doc = await api.getDesign(artifact.id, version);

    this.edit(doc);
    await this.save(`Restored version ${version}`);
  }

  async announced(announcement: { kind: 'artifact'; version: number; deleted: boolean } | { kind: 'comment' }): Promise<void>
  {
    const artifact = this.artifact;

    if (!artifact) return;

    if (announcement.kind === 'comment')
    {
      this.comments = await api.listComments(artifact.id);
      return;
    }

    if (announcement.deleted)
    {
      this.notice = 'This design was deleted.';
      return;
    }

    if (announcement.version <= this.knownVersion) return;

    const fresh = await api.getArtifact(artifact.id);
    const latest = fresh.versions?.find((version) => version.version === fresh.latest_version);

    this.artifact = fresh;
    this.title = fresh.title;
    this.versions = fresh.versions ?? this.versions;

    if (fresh.latest_version <= this.knownVersion) return;

    this.knownVersion = fresh.latest_version;

    if (latest?.author === 'agent')
    {
      await this.receiveAgentVersion(fresh);
    }
  }

  private async receiveAgentVersion(artifact: Artifact): Promise<void>
  {
    const incoming = await api.getDesign(artifact.id, artifact.latest_version);
    const local = this.history.present;
    const merged = mergeIncoming(this.base, local, incoming);

    this.base = incoming;
    this.history = commit(this.history, merged);

    if (this.selectedId && !findItem(merged, this.selectedId))
    {
      this.selectedId = null;
    }

    if (hasLocalChanges(incoming, merged))
    {
      this.scheduleSave();
    }
    else
    {
      this.saveState = 'saved';
    }
  }

  async addComment(body: string): Promise<void>
  {
    const artifact = this.artifact;
    const anchor = this.pendingAnchor;

    if (!artifact || !anchor || body.trim() === '') return;

    const created = await api.postComment(artifact.id, body.trim(), anchor);

    this.comments = [...this.comments.filter((comment) => comment.id !== created.id), created];
    this.pendingAnchor = null;
    this.openThread = created.id;
  }

  async setResolved(commentId: string, resolved: boolean): Promise<void>
  {
    const artifact = this.artifact;

    if (!artifact) return;

    const updated = await api.resolveComment(artifact.id, commentId, resolved);

    this.comments = this.comments.map((comment) => (comment.id === commentId ? updated : comment));

    if (resolved)
    {
      this.openThread = null;
    }
  }
}
