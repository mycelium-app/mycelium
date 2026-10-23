create table usage_turns (
    id bigserial primary key,
    conversation_id uuid not null references conversations(id) on delete cascade,
    owner uuid not null references users(id) on delete cascade,
    node_id uuid not null references nodes(id) on delete cascade,
    agent text not null,
    project_id uuid references projects(id) on delete set null,
    model text,
    ended_at timestamptz not null default now(),
    day date not null,
    working_ms bigint,
    reported boolean not null default false,
    input_tokens bigint not null default 0,
    output_tokens bigint not null default 0,
    cache_read_tokens bigint not null default 0,
    cache_write_tokens bigint not null default 0,
    cost_usd double precision,
    cost_total double precision
);

create index usage_turns_owner_day on usage_turns (owner, day);
create index usage_turns_conversation on usage_turns (conversation_id, id);

insert into usage_turns (conversation_id, owner, node_id, agent, project_id, model, ended_at, day)
select c.id, c.owner, c.node_id, c.agent, project.id, c.model, turn_time.moment, (turn_time.moment at time zone 'utc')::date
from events e
join conversations c on c.id = e.conversation_id
cross join lateral (select case when c.imported_from is not null then c.updated_at else e.created_at end as moment) turn_time
left join lateral (
    select p.id from projects p
    where p.node_id = c.node_id and (c.cwd = p.path or starts_with(c.cwd, p.path || '/') or starts_with(c.cwd, p.path || '-worktrees/'))
    order by length(p.path) desc limit 1
) project on true
where e.kind = 'turn_end';
