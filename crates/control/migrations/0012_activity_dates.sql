create table boot_repairs (
    name text primary key,
    done_at timestamptz not null default now()
);

update conversations c set updated_at = live.at
from (
    select conversation_id, max(created_at) as at from events
    where node_seq is not null or (kind = 'user_message' and payload ? 'prompt_id')
    group by conversation_id
) live
where live.conversation_id = c.id;
