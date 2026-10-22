create table tasks (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    node_id uuid not null references nodes(id) on delete cascade,
    agent text not null,
    cwd text not null default '',
    project_id uuid references projects(id) on delete cascade,
    branch text,
    title text not null,
    prompt text,
    started_at timestamptz,
    conversation_id uuid references conversations(id) on delete set null,
    created_at timestamptz not null default now()
);

create index tasks_owner_open on tasks (owner, created_at) where started_at is null;
