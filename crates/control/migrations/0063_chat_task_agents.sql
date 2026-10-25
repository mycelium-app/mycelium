update tasks t
set agent = coalesce((
    select c.agent from conversations c
    where c.owner = t.owner and c.agent in ('claude', 'codex')
      and (c.id = (select parent_id from conversations where id = t.source_id) or c.project_id = t.project_id)
    order by (c.id = (select parent_id from conversations where id = t.source_id)) desc nulls last, c.updated_at desc
    limit 1
), 'claude')
where t.agent = 'chat';
