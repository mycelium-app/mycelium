create table assistant_actions (
    id uuid primary key default gen_random_uuid(),
    assistant_id uuid not null references assistants(id) on delete cascade,
    name text not null,
    prompt text not null,
    inputs jsonb not null default '[]',
    model jsonb,
    node_id uuid references nodes(id) on delete set null,
    pinned boolean not null default false,
    position integer not null default 0,
    builtin_key text,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create unique index assistant_actions_name on assistant_actions (assistant_id, lower(name));
create unique index assistant_actions_builtin on assistant_actions (assistant_id, builtin_key) where builtin_key is not null;

create table assistant_action_proposals (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    assistant_id uuid not null references assistants(id) on delete cascade,
    conversation_id uuid references conversations(id) on delete set null,
    name text not null,
    prompt text not null,
    inputs jsonb not null default '[]',
    reason text not null default '',
    state text not null default 'proposed' check (state in ('proposed', 'approved', 'declined')),
    created_at timestamptz not null default now(),
    decided_at timestamptz
);

create index assistant_action_proposals_assistant on assistant_action_proposals (assistant_id, created_at desc);

alter table schedules
    alter column node_id drop not null,
    alter column agent drop not null,
    add column action_id uuid references assistant_actions(id) on delete set null,
    add column action_inputs jsonb not null default '{}';

alter table assistants add column builtin_key text;

create unique index assistants_builtin on assistants (owner, builtin_key) where builtin_key is not null;

alter table reflections add column assistant_id uuid references assistants(id) on delete set null;
alter table exercise_reviews add column assistant_id uuid references assistants(id) on delete set null;

insert into assistants (owner, name, description, builtin_key)
select owner, 'Wellness', 'Drafts your reflections, wraps up your day and reviews your exercises.', 'wellness' from user_settings where reflection_schedule_id is not null
on conflict do nothing;

insert into assistant_files (assistant_id, path, content, sha256)
select id, 'AGENTS.md', ''::bytea, encode(sha256(''::bytea), 'hex') from assistants where builtin_key = 'wellness'
on conflict do nothing;

insert into assistant_actions (assistant_id, name, prompt, inputs, builtin_key)
select a.id, 'Weekly reflection',
    'Draft my weekly reflection for the week containing {week} (Monday to Sunday). Use find_conversations and recall to look over that week, then call save_reflection once with four short sections in plain Markdown, written in the first person in British English: done (what was finished), learned (what I found out or got better at), waiting (what is blocked or waiting on someone) and next_week (what to pick up next). Leave a section empty rather than padding it. Do not change any files.',
    '[{"name": "week", "kind": "date", "required": false, "default": "today"}]'::jsonb,
    'weekly_reflection'
from assistants a where a.builtin_key = 'wellness'
on conflict do nothing;

update schedules s set assistant_id = a.id, node_id = null, agent = null, cwd = '', each_run = 'same',
    action_id = (select x.id from assistant_actions x where x.assistant_id = a.id and x.builtin_key = 'weekly_reflection')
from user_settings u join assistants a on a.owner = u.owner and a.builtin_key = 'wellness'
where s.id = u.reflection_schedule_id;

create table assistant_changes (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    conversation_id uuid references conversations(id) on delete set null,
    kind text not null check (kind in ('create', 'update')),
    assistant_id uuid references assistants(id) on delete cascade,
    name text not null,
    description text,
    group_id uuid references groups(id) on delete set null,
    instructions text,
    preferences jsonb,
    before jsonb,
    state text not null default 'proposed' check (state in ('proposed', 'approved', 'declined', 'applied', 'undone')),
    created_at timestamptz not null default now(),
    decided_at timestamptz
);

create index assistant_changes_owner on assistant_changes (owner, created_at desc);
