alter table tasks add column status text not null default 'open' check (status in ('question', 'proposed', 'open', 'claimed', 'done', 'declined'));
alter table tasks add column source_id uuid references conversations(id) on delete set null;
alter table tasks add column options text[] not null default '{}';
alter table tasks add column answer text;

update tasks set status = 'claimed' where started_at is not null;

alter table tasks drop column started_at;

create index tasks_owner_status on tasks (owner, status, created_at);
