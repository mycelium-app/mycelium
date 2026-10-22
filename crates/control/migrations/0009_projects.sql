create table projects (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    node_id uuid not null references nodes(id) on delete cascade,
    name text not null,
    path text not null,
    forge_id uuid references forges(id) on delete set null,
    repository text,
    created_at timestamptz not null default now(),
    unique (node_id, path)
);
