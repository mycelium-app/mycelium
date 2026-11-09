drop table if exists tasks cascade;

alter table user_settings drop column if exists experiment_tasks;
