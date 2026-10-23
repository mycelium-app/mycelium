create table chat_tool_servers (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    name text not null,
    url text not null,
    bearer_token text,
    enabled boolean not null default true,
    always_allow boolean not null default false,
    tools jsonb not null default '[]',
    created_at timestamptz not null default now()
);

create unique index chat_tool_servers_name on chat_tool_servers (owner, lower(name));
