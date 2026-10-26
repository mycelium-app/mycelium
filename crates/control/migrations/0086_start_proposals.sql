create table start_proposals (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    conversation_id uuid references conversations(id) on delete set null,
    kind text not null check (kind in ('subagent', 'workflow')),
    plan jsonb not null,
    choices jsonb not null default '{}',
    connect boolean not null default false,
    state text not null default 'proposed' check (state in ('proposed', 'started', 'declined')),
    workflow_id uuid references workflows(id) on delete set null,
    created_at timestamptz not null default now(),
    decided_at timestamptz
);

create index start_proposals_owner on start_proposals (owner, created_at desc);
