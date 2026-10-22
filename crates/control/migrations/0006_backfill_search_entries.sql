insert into search_entries (conversation_id, seq, text)
select conversation_id, seq, btrim(payload->>'text')
from events
where kind = 'user_message' and btrim(coalesce(payload->>'text', '')) <> ''
on conflict do nothing;

insert into search_entries (conversation_id, seq, text)
select t.conversation_id, t.seq, answer.text
from events t
cross join lateral (
    select string_agg(c.payload->'update'->'content'->>'text', '' order by c.seq) as text
    from events c
    where c.conversation_id = t.conversation_id and c.kind = 'acp_update' and c.payload->'update'->>'sessionUpdate' = 'agent_message_chunk'
      and c.seq < t.seq
      and c.seq > coalesce((select max(u.seq) from events u where u.conversation_id = t.conversation_id and u.kind = 'user_message' and u.seq < t.seq), 0)
) answer
where t.kind = 'turn_end' and coalesce(answer.text, '') <> ''
on conflict do nothing;
