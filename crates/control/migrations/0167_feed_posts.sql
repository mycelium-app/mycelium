create table feed_posts (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    conversation_id uuid not null references conversations(id) on delete cascade,
    message text not null,
    created_at timestamptz not null default now()
);

create index feed_posts_by_owner on feed_posts (owner, created_at desc);
