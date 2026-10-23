alter table client_tokens
    add column scopes text[],
    add column expires_at timestamptz,
    add column conversation_id uuid references conversations(id) on delete set null;

create table api_access_requests (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    conversation_id uuid not null references conversations(id) on delete cascade,
    scopes text[] not null,
    hours integer not null,
    reason text not null,
    token_id uuid references client_tokens(id) on delete set null,
    state text not null default 'requested' check (state in ('requested', 'approved', 'declined')),
    created_at timestamptz not null default now(),
    decided_at timestamptz
);

create index api_access_requests_owner on api_access_requests (owner, created_at desc);

create table api_token_calls (
    id bigint generated always as identity primary key,
    token_id uuid not null references client_tokens(id) on delete cascade,
    method text not null,
    route text not null,
    status integer not null,
    called_at timestamptz not null default now()
);

create index api_token_calls_called_at on api_token_calls (called_at);
create index api_token_calls_token on api_token_calls (token_id, called_at desc);
