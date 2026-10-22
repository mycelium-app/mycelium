create table worktrees (
    id uuid primary key default gen_random_uuid(),
    project_id uuid not null references projects(id) on delete cascade,
    branch text not null,
    path text not null,
    issue_number bigint,
    created_at timestamptz not null default now(),
    unique (project_id, branch)
);
