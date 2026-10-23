alter table users add column time_zone text;

alter table schedules add column time_zone text not null default 'UTC';
alter table schedules add column proposed boolean not null default false;
alter table schedules add column conversation_id uuid references conversations(id) on delete cascade;

drop index schedules_due;
create index schedules_due on schedules (next_run_at) where not paused and not proposed;
