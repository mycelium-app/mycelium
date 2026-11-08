alter table conversation_connections add column last_used_at timestamptz not null default now();
