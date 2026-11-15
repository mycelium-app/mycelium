create table client_events (
    id bigserial primary key,
    owner uuid not null references users(id) on delete cascade,
    at timestamptz not null,
    received_at timestamptz not null default now(),
    session_id text not null,
    client text not null,
    version text not null,
    user_agent text not null default '',
    kind text not null,
    message text not null default '',
    detail jsonb not null default '{}'
);

create index client_events_owner_at on client_events (owner, at desc);
create index client_events_received_at on client_events (received_at);
