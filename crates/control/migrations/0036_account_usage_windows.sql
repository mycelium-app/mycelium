alter table logins add column usage_windows jsonb not null default '[]';
alter table logins add column usage_read_at timestamptz;

create table login_usage_history (
    node_id uuid not null references nodes(id) on delete cascade,
    agent text not null,
    account text not null default '',
    read_at timestamptz not null,
    windows jsonb not null,
    primary key (node_id, agent, account, read_at)
);
