create table import_jobs (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    node_id uuid not null references nodes(id) on delete cascade,
    agent text not null,
    state text not null default 'running',
    total int not null,
    done int not null default 0,
    imported int not null default 0,
    skipped int not null default 0,
    waiting int not null default 0,
    failed jsonb not null default '[]',
    started_at timestamptz not null default now(),
    finished_at timestamptz
);

create unique index import_jobs_running on import_jobs (node_id, agent) where state = 'running';
create index import_jobs_node on import_jobs (node_id, agent, started_at desc);
