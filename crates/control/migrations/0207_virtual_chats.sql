create table virtual_chats (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    assistant_id uuid not null references assistants(id) on delete cascade,
    title text not null,
    description text,
    closed_at timestamptz,
    read_at timestamptz not null default now(),
    todos jsonb not null default '[]',
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index virtual_chats_assistant on virtual_chats (assistant_id, created_at);

create table virtual_messages (
    id uuid primary key default gen_random_uuid(),
    chat_id uuid not null references virtual_chats(id) on delete cascade,
    from_person boolean not null,
    text text not null,
    attachments jsonb not null default '[]',
    queue_id uuid,
    sending boolean not null default false,
    created_at timestamptz not null default now()
);

create index virtual_messages_chat on virtual_messages (chat_id, created_at);
create index virtual_messages_queue on virtual_messages (queue_id) where queue_id is not null;
