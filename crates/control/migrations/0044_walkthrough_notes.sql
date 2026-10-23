create table walkthrough_notes (
    id uuid primary key default gen_random_uuid(),
    walkthrough_id uuid not null references walkthroughs(id) on delete cascade,
    user_id uuid not null,
    step int not null,
    file text,
    line_start int,
    line_end int,
    quote text not null default '',
    comment text not null default '',
    sent_at timestamptz,
    created_at timestamptz not null default now()
);

create index walkthrough_notes_walkthrough on walkthrough_notes (walkthrough_id, user_id, created_at);
