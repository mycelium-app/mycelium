alter table workflows
    add column kind text not null default 'mycelium',
    add column ended_at timestamptz,
    add column started_by jsonb,
    add column project_id uuid references projects(id) on delete set null,
    add column end_reason jsonb,
    add column rerun_of uuid references workflows(id) on delete set null,
    add column decisions jsonb not null default '[]',
    add column phases jsonb not null default '[]',
    add column run_id text;
create unique index workflows_agent_run on workflows(conversation_id, run_id) where run_id is not null;
create table workflow_rounds (
    workflow_id uuid not null references workflows(id) on delete cascade,
    round integer not null,
    started_at timestamptz not null default now(),
    ended_at timestamptz,
    reason text,
    decided_by jsonb,
    steps jsonb not null default '[]',
    primary key (workflow_id, round)
);
update workflows set ended_at = updated_at where status in ('done','failed','stopped');
insert into workflow_rounds (workflow_id, round, started_at) select id, round, created_at from workflows;
