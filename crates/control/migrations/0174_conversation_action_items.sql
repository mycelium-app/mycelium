create table conversation_action_items (
    id uuid primary key default gen_random_uuid(),
    conversation_id uuid not null references conversations (id) on delete cascade,
    text text not null,
    created_at timestamptz not null default now(),
    done_at timestamptz
);

create index conversation_action_items_open on conversation_action_items (conversation_id) where done_at is null;

create function conversation_action_items(conversation uuid) returns jsonb
language sql stable as $$
    select coalesce(jsonb_agg(jsonb_build_object('id', id, 'text', text, 'created_at', created_at) order by created_at), '[]'::jsonb)
    from conversation_action_items
    where conversation_id = conversation and done_at is null
$$;
