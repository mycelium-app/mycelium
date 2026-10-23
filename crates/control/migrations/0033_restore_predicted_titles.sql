update conversations c set title_user_set = false
where c.title_user_set
  and (
    c.title = 'Untitled conversation'
    or c.imported_from is not null
    or c.title = (
        select left(btrim(split_part(btrim(coalesce(e.payload->>'text', '')), E'\n', 1)), 60)
        from events e
        where e.conversation_id = c.id and e.kind = 'user_message'
        order by e.seq
        limit 1
    )
  );
