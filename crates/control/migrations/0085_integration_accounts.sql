alter table chat_tool_servers
    add column token_expires_at timestamptz,
    add column last_used_at timestamptz,
    add column last_error text,
    add column policy jsonb not null default '{}';

create table integration_groups (
    server_id uuid not null references chat_tool_servers(id) on delete cascade,
    group_id uuid not null references groups(id) on delete cascade,
    primary key (server_id, group_id)
);

create table integration_calls (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    server_id uuid references chat_tool_servers(id) on delete set null,
    server_name text not null,
    tool text not null,
    kind text not null check (kind in ('read', 'write')),
    conversation_id uuid references conversations(id) on delete set null,
    surface text not null check (surface in ('chat', 'workspace', 'assistant')),
    provider text,
    decision text not null check (decision in ('approved', 'declined', 'auto')),
    review text check (review in ('shared', 'removed', 'withheld')),
    is_error boolean not null default false,
    at timestamptz not null default now()
);

create index integration_calls_owner on integration_calls (owner, at desc);
create index integration_calls_server on integration_calls (server_id, at desc);
