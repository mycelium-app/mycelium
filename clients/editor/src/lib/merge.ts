import { type DesignDoc, type Item, designsEqual, nextId, idPrefix, type Page } from './doc';

function itemsById(doc: DesignDoc): Map<string, { item: Item; page: Page }>
{
  const found = new Map<string, { item: Item; page: Page }>();

  doc.pages.forEach((page) => page.items.forEach((item) => found.set(item.id, { item, page })));

  return found;
}

function changedFields(base: Item, local: Item): Partial<Item>
{
  const patch: Record<string, unknown> = {};
  const baseFields = base as Record<string, unknown>;

  Object.entries(local).forEach(([key, value]) =>
  {
    if (JSON.stringify(baseFields[key]) !== JSON.stringify(value))
    {
      patch[key] = value;
    }
  });

  return patch as Partial<Item>;
}

export function hasLocalChanges(base: DesignDoc, local: DesignDoc): boolean
{
  return !designsEqual(base, local);
}

export function mergeIncoming(base: DesignDoc, local: DesignDoc, incoming: DesignDoc): DesignDoc
{
  const baseItems = itemsById(base);
  const localItems = itemsById(local);
  let merged: DesignDoc = { pages: incoming.pages.map((page) => ({ ...page, items: [...page.items] })) };

  local.pages.forEach((localPage) =>
  {
    const basePage = base.pages.find((page) => page.id === localPage.id);
    const mergedPage = merged.pages.find((page) => page.id === localPage.id);

    if (!basePage && !mergedPage)
    {
      merged = { pages: [...merged.pages, { ...localPage, items: [] }] };
    }

    if (basePage && mergedPage && basePage.name !== localPage.name)
    {
      mergedPage.name = localPage.name;
    }
  });

  baseItems.forEach((_, id) =>
  {
    if (!localItems.has(id))
    {
      merged = { pages: merged.pages.map((page) => ({ ...page, items: page.items.filter((item) => item.id !== id) })) };
    }
  });

  localItems.forEach(({ item, page }, id) =>
  {
    const original = baseItems.get(id);

    if (original)
    {
      const patch = changedFields(original.item, item);

      if (Object.keys(patch).length > 0)
      {
        merged = {
          pages: merged.pages.map((current) => ({
            ...current,
            items: current.items.map((candidate) => (candidate.id === id ? ({ ...candidate, ...patch } as Item) : candidate)),
          })),
        };
      }

      return;
    }

    const target = merged.pages.find((current) => current.id === page.id) ?? merged.pages[0];
    const taken = itemsById(merged).has(id);
    const added = taken ? ({ ...item, id: nextId(merged, idPrefix(item.type)) } as Item) : item;

    merged = { pages: merged.pages.map((current) => (current === target ? { ...current, items: [...current.items, added] } : current)) };
  });

  return merged;
}
