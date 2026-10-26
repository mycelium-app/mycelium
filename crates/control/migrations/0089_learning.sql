alter table conversations add column teach_me boolean not null default false;

alter table walkthroughs add column checks jsonb;

create table walkthrough_answers (
    walkthrough_id uuid not null references walkthroughs(id) on delete cascade,
    owner uuid not null references users(id) on delete cascade,
    step integer not null,
    choice integer not null,
    correct boolean not null,
    answered_at timestamptz not null default now(),
    primary key (walkthrough_id, owner, step)
);

create table reflections (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    week date not null,
    done text not null default '',
    learned text not null default '',
    waiting text not null default '',
    next_week text not null default '',
    edited text[] not null default '{}',
    drafted_at timestamptz,
    conversation_id uuid references conversations(id) on delete set null,
    unique (owner, week)
);

alter table user_settings add column reflection_schedule_id uuid references schedules(id) on delete set null;
