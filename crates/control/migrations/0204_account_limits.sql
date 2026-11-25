create table account_limits (
    node_id uuid not null references nodes(id) on delete cascade,
    agent text not null,
    account text not null,
    limited_until timestamptz not null,
    primary key (node_id, agent, account)
);
