create table conversation_connections (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    source_id uuid not null references conversations(id) on delete cascade,
    target_id uuid not null references conversations(id) on delete cascade,
    created_at timestamptz not null default now(),
    revoked_at timestamptz,
    check (source_id < target_id),
    unique (owner, source_id, target_id)
);

create index conversation_connections_owner on conversation_connections (owner, created_at desc);
