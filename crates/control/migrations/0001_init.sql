create table users (
    id uuid primary key default gen_random_uuid(),
    subject text not null unique,
    email text not null,
    name text not null,
    created_at timestamptz not null default now()
);

create table web_sessions (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    token_hash bytea not null unique,
    expires_at timestamptz not null,
    created_at timestamptz not null default now()
);

create table device_codes (
    id uuid primary key default gen_random_uuid(),
    kind text not null check (kind in ('node', 'client')),
    device_code_hash bytea not null unique,
    user_code text not null unique,
    label text not null,
    hostname text not null,
    os text not null,
    arch text not null,
    ip text not null,
    expires_at timestamptz not null,
    approved_by uuid references users(id),
    approved_at timestamptz,
    result_id uuid,
    consumed_at timestamptz
);

create table nodes (
    id uuid primary key default gen_random_uuid(),
    name text not null unique,
    hostname text not null,
    os text not null,
    arch text not null,
    version text not null default '',
    token_hash bytea not null unique,
    created_by uuid not null references users(id),
    created_at timestamptz not null default now(),
    last_seen_at timestamptz,
    revoked_at timestamptz,
    draining boolean not null default false
);

create table client_tokens (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    label text not null,
    token_hash bytea not null unique,
    created_at timestamptz not null default now(),
    last_used_at timestamptz,
    revoked_at timestamptz
);

create table agents (
    node_id uuid not null references nodes(id) on delete cascade,
    agent text not null,
    version text,
    available boolean not null,
    primary key (node_id, agent)
);

create table logins (
    id uuid primary key default gen_random_uuid(),
    node_id uuid not null references nodes(id) on delete cascade,
    agent text not null,
    account text,
    method text not null,
    state text not null check (state in ('signed_in', 'signed_out', 'expired', 'unknown')),
    updated_at timestamptz not null default now(),
    unique (node_id, agent)
);

create table conversations (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id),
    node_id uuid not null references nodes(id),
    agent text not null,
    cwd text not null,
    title text not null,
    status text not null check (status in ('working', 'input', 'review', 'failed', 'done')),
    acp_session_id text,
    mode text,
    model text,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    archived_at timestamptz
);

create index conversations_owner on conversations (owner, updated_at desc);
create index conversations_node on conversations (node_id);

create table events (
    id bigserial primary key,
    conversation_id uuid not null references conversations(id) on delete cascade,
    seq bigint not null,
    kind text not null,
    payload jsonb not null,
    node_seq bigint,
    created_at timestamptz not null default now(),
    unique (conversation_id, seq)
);

create unique index events_node_seq on events (conversation_id, node_seq) where node_seq is not null;

create table queued_prompts (
    id uuid primary key default gen_random_uuid(),
    conversation_id uuid not null references conversations(id) on delete cascade,
    position int not null,
    text text not null,
    created_at timestamptz not null default now()
);

create index queued_prompts_conversation on queued_prompts (conversation_id, position);

create table transcripts (
    node_id uuid not null references nodes(id) on delete cascade,
    path text not null,
    bytes bigint not null,
    updated_at timestamptz not null default now(),
    primary key (node_id, path)
);
