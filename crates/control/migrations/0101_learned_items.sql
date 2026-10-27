create table learned_items (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    text text not null,
    source_kind text not null check (source_kind in ('teach_me', 'walkthrough_check', 'exercise_review')),
    conversation_id uuid not null references conversations(id) on delete cascade,
    message_id bigint,
    created_at timestamptz not null default now()
);

create index learned_items_by_owner on learned_items (owner, created_at desc);
