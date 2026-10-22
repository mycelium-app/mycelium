update conversations c set archived_at = null
where c.imported_from is not null
  and c.archived_at is not null
  and c.archived_at <= (select installed_on from _sqlx_migrations where version = 7)
  and exists (select 1 from events e where e.conversation_id = c.id and e.node_seq is not null);
