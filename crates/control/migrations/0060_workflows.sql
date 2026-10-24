create table workflows (
    id uuid primary key,
    owner uuid not null references users(id),
    parent_id uuid references workflows(id),
    conversation_id uuid references conversations(id) on delete set null,
    title text not null,
    status text not null default 'running',
    plan jsonb not null,
    steps jsonb not null default '[]',
    round integer not null default 1,
    output text not null default '',
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);
create index workflows_owner on workflows(owner, created_at desc);
create index workflows_running on workflows(status) where status = 'running';
create table workflow_dispatches (
    workflow_id uuid not null references workflows(id) on delete cascade,
    round integer not null,
    step_key text not null,
    operation text not null,
    result uuid,
    primary key (workflow_id, round, step_key, operation)
);
