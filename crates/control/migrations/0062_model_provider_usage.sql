create table model_provider_usage (
    provider_id uuid primary key references model_providers(id) on delete cascade,
    read_at timestamptz not null,
    spend jsonb not null
);
