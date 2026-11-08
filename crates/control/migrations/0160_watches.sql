create table watches (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    watcher_id uuid not null references conversations(id) on delete cascade,
    kind text not null check (kind in ('conversation', 'pipeline', 'release', 'timer')),
    target_id uuid references conversations(id) on delete cascade,
    forge_id uuid references forges(id) on delete cascade,
    repository text,
    reference text,
    note text,
    due_at timestamptz,
    only_waiting boolean not null default false,
    created_at timestamptz not null default now(),
    ended_at timestamptz
);

create index watches_target on watches (target_id) where ended_at is null;
create index watches_watcher on watches (watcher_id) where ended_at is null;
create index watches_due on watches (due_at) where ended_at is null and kind = 'timer';
create unique index watches_one_per_conversation on watches (watcher_id, target_id) where ended_at is null and kind = 'conversation';

create table watch_notifications (
    watch_id uuid not null references watches(id) on delete cascade,
    key text not null,
    created_at timestamptz not null default now(),
    primary key (watch_id, key)
);
