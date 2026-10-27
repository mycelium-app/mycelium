create table person_ssh_tokens (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    node_id uuid not null references nodes(id) on delete cascade,
    token_hash bytea not null unique,
    created_at timestamptz not null default now(),
    expires_at timestamptz not null
);

create index person_ssh_tokens_by_expiry on person_ssh_tokens (expires_at);
