alter table user_settings add column experiment_assistants boolean not null default false;

create table assistants (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    name text not null,
    description text,
    group_id uuid references groups(id) on delete set null,
    preferences jsonb not null default '{"models":[],"nodes":[],"chat_first":true,"fall_back_on":["usage_limit","signed_out","offline"]}',
    limited jsonb not null default '{}',
    conversation_id uuid references conversations(id) on delete set null,
    pinned_at timestamptz,
    archived_at timestamptz,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    unique (owner, name)
);

create table assistant_files (
    assistant_id uuid not null references assistants(id) on delete cascade,
    path text not null check (path not like '/%' and path not like '%..%'),
    content bytea not null,
    sha256 text not null,
    updated_at timestamptz not null default now(),
    primary key (assistant_id, path)
);

alter table conversations add column assistant_id uuid references assistants(id) on delete set null;
create index conversations_assistant on conversations (assistant_id, created_at) where assistant_id is not null;
