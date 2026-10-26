alter table projects add column test_command text;

create table exercises (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    kind text not null check (kind in ('language', 'area', 'other')),
    name text not null,
    title text not null,
    brief text not null,
    source_conversation_id uuid references conversations(id) on delete set null,
    node_id uuid not null references nodes(id) on delete cascade,
    project_id uuid references projects(id) on delete set null,
    agent text not null,
    branch text,
    cwd text,
    conversation_id uuid references conversations(id) on delete set null,
    state text not null default 'suggested' check (state in ('suggested', 'started', 'reviewed', 'done')),
    minutes integer,
    not_now_until timestamptz,
    reviewing boolean not null default false,
    last_tests jsonb,
    created_at timestamptz not null default now(),
    started_at timestamptz
);

create index exercises_owner on exercises (owner, created_at desc);

create table exercise_reviews (
    id uuid primary key default gen_random_uuid(),
    exercise_id uuid not null references exercises(id) on delete cascade,
    summary text not null,
    comments jsonb not null default '[]',
    tests jsonb,
    created_at timestamptz not null default now()
);
