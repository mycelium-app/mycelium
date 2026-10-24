create table ssh_access_requests (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    conversation_id uuid not null references conversations(id) on delete cascade,
    node_id uuid not null references nodes(id) on delete cascade,
    minutes integer not null,
    reason text not null,
    state text not null default 'requested' check (state in ('requested', 'approved', 'declined', 'revoked')),
    token_hash bytea unique,
    created_at timestamptz not null default now(),
    decided_at timestamptz,
    expires_at timestamptz,
    revoked_at timestamptz
);

create index ssh_access_requests_owner on ssh_access_requests (owner, created_at desc);

create table ssh_sessions (
    id uuid primary key,
    request_id uuid not null references ssh_access_requests(id) on delete cascade,
    owner uuid not null references users(id) on delete cascade,
    conversation_id uuid not null references conversations(id) on delete cascade,
    node_id uuid not null references nodes(id) on delete cascade,
    started_at timestamptz not null default now(),
    ended_at timestamptz
);

create index ssh_sessions_request on ssh_sessions (request_id, started_at desc);
