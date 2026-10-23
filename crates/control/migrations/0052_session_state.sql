create table conversation_session_state (
    conversation_id uuid not null references conversations (id) on delete cascade,
    update_kind text not null,
    update jsonb not null,
    primary key (conversation_id, update_kind)
);

insert into conversation_session_state (conversation_id, update_kind, update)
select distinct on (conversation_id, payload->'update'->>'sessionUpdate') conversation_id, payload->'update'->>'sessionUpdate', payload->'update'
from events
where kind = 'acp_update' and payload->'update'->>'sessionUpdate' in ('available_commands_update', 'config_option_update', 'current_mode_update')
order by conversation_id, payload->'update'->>'sessionUpdate', seq desc;
