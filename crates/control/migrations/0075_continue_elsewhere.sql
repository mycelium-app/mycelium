alter table user_settings add column experiment_continue_elsewhere boolean not null default false;
alter table conversations add column previous_session_ids text[] not null default '{}';
