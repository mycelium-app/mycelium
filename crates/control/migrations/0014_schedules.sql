create table schedules (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    node_id uuid not null references nodes(id) on delete cascade,
    agent text not null,
    cwd text not null default '',
    project_id uuid references projects(id) on delete cascade,
    prompt text not null,
    cron text not null,
    each_run text not null,
    paused boolean not null default false,
    next_run_at timestamptz not null,
    last_run_at timestamptz,
    last_conversation_id uuid references conversations(id) on delete set null,
    created_at timestamptz not null default now()
);

create index schedules_due on schedules (next_run_at) where not paused;

create table schedule_runs (
    id uuid primary key default gen_random_uuid(),
    schedule_id uuid not null references schedules(id) on delete cascade,
    ran_at timestamptz not null default now(),
    outcome text not null,
    reason text,
    conversation_id uuid references conversations(id) on delete set null
);

create index schedule_runs_recent on schedule_runs (schedule_id, ran_at desc);
