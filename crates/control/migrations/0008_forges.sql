create table forges (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    kind text not null,
    base_url text not null,
    account text not null,
    token text not null,
    organisations text[] not null default '{}',
    created_at timestamptz not null default now(),
    unique (owner, base_url, account)
);
