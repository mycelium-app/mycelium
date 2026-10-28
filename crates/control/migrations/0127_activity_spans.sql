create table activity_spans (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    started_at timestamptz not null,
    ended_at timestamptz not null,
    kind text not null check (kind in ('conversation', 'chat', 'assistant', 'elsewhere')),
    route text not null default '',
    title text,
    group_id uuid references groups(id) on delete set null,
    conversation_id uuid references conversations(id) on delete set null,
    assistant_id uuid references assistants(id) on delete set null
);

create index activity_spans_by_owner on activity_spans (owner, ended_at desc);
