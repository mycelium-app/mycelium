create table model_providers (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    kind text not null,
    name text not null,
    base_url text,
    api_key text,
    models text[] not null default '{}',
    default_model text,
    created_at timestamptz not null default now()
);

create index model_providers_owner on model_providers (owner, created_at);

alter table conversations add column provider_id uuid references model_providers(id) on delete set null;
