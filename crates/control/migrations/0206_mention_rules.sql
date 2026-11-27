alter table mention_triggers
    add column integration text,
    add column position integer,
    add column guild_id text,
    add column guild_name text,
    add column sender_names text[] not null default '{}',
    add column project_id uuid references projects(id) on delete cascade,
    add column agent text,
    alter column server_id drop not null,
    alter column assistant_id drop not null;

drop index mention_triggers_one_per_account;

update mention_triggers t set integration = s.kind from chat_tool_servers s where s.id = t.server_id;

update mention_triggers t set position = ranked.position
from (select id, (row_number() over (partition by owner, integration order by created_at, id) - 1)::integer as position from mention_triggers) ranked
where ranked.id = t.id;

update mention_triggers t set server_id = null
where (select count(*) from mention_triggers other where other.owner = t.owner and other.integration = t.integration) = 1;

alter table mention_triggers
    alter column integration set not null,
    alter column position set not null,
    add constraint mention_triggers_one_target check ((assistant_id is null) <> (project_id is null and agent is null) and (project_id is null) = (agent is null));

create index mention_triggers_order on mention_triggers (owner, integration, position);
