alter table user_settings add column experiment_tasks_and_workflows boolean not null default false;
alter table user_settings add column experiment_history boolean not null default false;
