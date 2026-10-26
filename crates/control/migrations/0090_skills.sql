create table skill_lines (
    owner uuid not null references users(id) on delete cascade,
    day date not null,
    kind text not null check (kind in ('language', 'area', 'other')),
    name text not null,
    agent_lines integer not null default 0,
    own_lines integer not null default 0,
    primary key (owner, day, kind, name)
);

create table hidden_skills (
    owner uuid not null references users(id) on delete cascade,
    kind text not null check (kind in ('language', 'area', 'other')),
    name text not null,
    primary key (owner, kind, name)
);

create table counted_tool_calls (
    conversation_id uuid not null references conversations(id) on delete cascade,
    tool_call_id text not null,
    primary key (conversation_id, tool_call_id)
);

alter table user_settings add column skills_backfilled boolean not null default false;
