create table coolify_apps (
    server_id uuid not null references chat_tool_servers(id) on delete cascade,
    app_uuid text not null,
    conversation_id uuid references conversations(id) on delete set null,
    created_at timestamptz not null default now(),
    primary key (server_id, app_uuid)
);

create table coolify_deployments (
    server_id uuid not null references chat_tool_servers(id) on delete cascade,
    deployment_uuid text not null,
    app_uuid text not null,
    triggered_by text not null check (triggered_by in ('agent', 'person')),
    conversation_id uuid references conversations(id) on delete set null,
    created_at timestamptz not null default now(),
    primary key (server_id, deployment_uuid)
);

create index coolify_deployments_app on coolify_deployments (server_id, app_uuid, created_at desc);

create table coolify_env_changes (
    server_id uuid not null references chat_tool_servers(id) on delete cascade,
    app_uuid text not null,
    key text not null,
    changed_by text not null check (changed_by in ('agent', 'person')),
    changed_at timestamptz not null default now(),
    primary key (server_id, app_uuid, key)
);
