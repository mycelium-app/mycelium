create table mention_triggers (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    server_id uuid not null references chat_tool_servers(id) on delete cascade,
    assistant_id uuid not null references assistants(id) on delete cascade,
    channels text[] not null default '{}',
    senders text[] not null default '{}',
    created_at timestamptz not null default now()
);

create index mention_triggers_server on mention_triggers (server_id);
