create table harness_models (
    node_id uuid not null references nodes(id) on delete cascade,
    agent text not null,
    account text not null default '',
    models jsonb not null default '[]',
    default_model text,
    reported_at timestamptz not null default now(),
    primary key (node_id, agent, account)
);
