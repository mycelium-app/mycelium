create table goals (
    conversation_id uuid primary key references conversations(id) on delete cascade,
    text text not null,
    token_budget bigint not null,
    state text not null default 'active',
    reason text,
    check_id uuid,
    started_at timestamptz not null default now()
);

create table walkthroughs (
    id uuid primary key default gen_random_uuid(),
    conversation_id uuid not null references conversations(id) on delete cascade,
    steps jsonb not null,
    created_at timestamptz not null default now()
);

create index walkthroughs_conversation on walkthroughs (conversation_id, created_at desc);
