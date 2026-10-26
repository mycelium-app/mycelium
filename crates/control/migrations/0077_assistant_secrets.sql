create table assistant_secrets (
    assistant_id uuid not null references assistants(id) on delete cascade,
    name text not null check (name ~ '^[A-Z_][A-Z0-9_]*$'),
    value text,
    description text,
    set_by text,
    updated_at timestamptz not null default now(),
    primary key (assistant_id, name)
);
