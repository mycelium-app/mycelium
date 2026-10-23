create table instruction_proposals (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    conversation_id uuid references conversations(id) on delete set null,
    layer text not null,
    layer_name text not null,
    base_revision text not null,
    base_text text not null,
    text text not null,
    reason text not null,
    state text not null default 'proposed' check (state in ('proposed', 'approved', 'declined', 'superseded')),
    note text,
    created_at timestamptz not null default now(),
    decided_at timestamptz
);

create index instruction_proposals_owner on instruction_proposals (owner, created_at desc);
