create table memories (
    id uuid primary key default gen_random_uuid(),
    text text not null,
    scopes text[] not null default '{global}',
    created_by text not null,
    conversation_id uuid references conversations(id) on delete set null,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index memories_text_search on memories using gin (to_tsvector('english', text));
