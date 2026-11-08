alter table conversations add column managed_by uuid references conversations(id) on delete set null,
    add column flagged_at timestamptz,
    add column flag_reason text;

update conversations started set managed_by = starter.id
from conversation_connections connection, conversations starter
where started.managed_by is null
  and connection.revoked_at is null
  and starter.id in (connection.source_id, connection.target_id)
  and started.id in (connection.source_id, connection.target_id)
  and starter.id <> started.id
  and exists (
      select 1 from events first
      where first.conversation_id = started.id and first.kind = 'user_message'
        and first.seq = (select min(seq) from events where conversation_id = started.id and kind = 'user_message')
        and (first.payload->>'text' like 'From ' || starter.title || ':%' or first.payload->>'text' like 'From ' || starter.id::text || ':%')
  );
