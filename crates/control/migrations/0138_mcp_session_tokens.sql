create table mcp_session_tokens (
    id uuid primary key default gen_random_uuid(),
    token_hash bytea not null unique,
    conversation_id uuid not null references conversations(id) on delete cascade,
    node_id uuid not null references nodes(id) on delete cascade,
    created_at timestamptz not null default now()
);

create index mcp_session_tokens_conversation on mcp_session_tokens (conversation_id);
