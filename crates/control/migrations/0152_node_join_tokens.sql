create table node_join_tokens (
    id uuid primary key default gen_random_uuid(),
    token_hash bytea not null unique,
    name text not null,
    created_by uuid not null references users(id),
    created_at timestamptz not null default now(),
    expires_at timestamptz not null,
    consumed_at timestamptz,
    node_id uuid references nodes(id)
);
