create table remote_images (
    owner uuid not null references users(id) on delete cascade,
    conversation_id uuid not null references conversations(id) on delete cascade,
    url text not null,
    allowed_by text not null check (allowed_by in ('search_images', 'click')),
    hops text[] not null default '{}',
    sha256 text,
    media_type text,
    refusal text,
    fetched_at timestamptz not null default now(),
    primary key (owner, conversation_id, url)
);
