create table group_connections (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    source_group_id uuid not null references groups(id) on delete cascade,
    target_group_id uuid not null references groups(id) on delete cascade,
    created_at timestamptz not null default now(),
    revoked_at timestamptz,
    check (source_group_id < target_group_id),
    unique (owner, source_group_id, target_group_id)
);

create index group_connections_owner_created on group_connections (owner, created_at desc);

alter table user_settings add column experiment_chat_across_groups boolean not null default false;
