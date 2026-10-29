alter table user_settings add column node_preferences jsonb not null default '{}'::jsonb;
