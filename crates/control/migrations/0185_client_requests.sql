create table client_requests (
    scope uuid not null,
    client_id uuid not null,
    result_id uuid,
    created_at timestamptz not null default now(),
    primary key (scope, client_id)
);
