alter table conversations add column agent_active boolean not null default false;
alter table conversations add column last_activity_at timestamptz;
