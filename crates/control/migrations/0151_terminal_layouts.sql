create table terminal_layouts (
    conversation_id uuid primary key references conversations(id) on delete cascade,
    layout jsonb not null,
    updated_at timestamptz not null default now()
);
