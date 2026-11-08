alter table forges add column webhook_secret text;

create table forge_webhooks (
    forge_id uuid not null references forges(id) on delete cascade,
    repository text not null,
    created_at timestamptz not null default now(),
    primary key (forge_id, repository)
);

alter table watches add column checked_at timestamptz;
