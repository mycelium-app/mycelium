create table decision_providers (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    name text not null,
    base_url text not null,
    api_key text,
    model text not null,
    uses text[] not null default '{}',
    created_at timestamptz not null default now()
);

create index decision_providers_owner on decision_providers (owner, created_at);
