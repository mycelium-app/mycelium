update conversations started set managed_by = (
    select case when connection.source_id = started.id then connection.target_id else connection.source_id end
    from conversation_connections connection
    where connection.revoked_at is null and started.id in (connection.source_id, connection.target_id)
    order by connection.created_at, connection.id
    limit 1
)
where started.managed_by is null
  and exists (
      select 1 from conversation_connections connection
      where connection.revoked_at is null and started.id in (connection.source_id, connection.target_id)
  )
  and exists (
      select 1 from events first
      where first.conversation_id = started.id and first.kind = 'user_message'
        and first.seq = (select min(seq) from events where conversation_id = started.id and kind = 'user_message')
        and first.payload->>'text' ~ '^From [^\n]+:(\n|$)'
  );
