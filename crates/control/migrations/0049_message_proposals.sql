create table message_proposals (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    conversation_id uuid references conversations(id) on delete set null,
    target_id uuid not null references conversations(id) on delete cascade,
    message text not null,
    reason text not null,
    state text not null default 'proposed' check (state in ('proposed', 'sent', 'declined')),
    created_at timestamptz not null default now(),
    decided_at timestamptz
);

create index message_proposals_owner on message_proposals (owner, created_at desc);
