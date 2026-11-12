create table manual_resets (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    agent text not null check (agent = 'claude'),
    account text not null,
    kind text not null check (kind in ('full', 'model')),
    model text,
    expires_at timestamptz not null,
    used_at timestamptz,
    used_key text,
    created_at timestamptz not null default now(),
    check ((kind = 'model') = (model is not null))
);

create index manual_resets_owner on manual_resets (owner, agent, account) where used_at is null;
