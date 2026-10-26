alter table schedules add column assistant_id uuid references assistants(id) on delete cascade;
