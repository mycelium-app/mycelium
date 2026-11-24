-- no-transaction
create index concurrently if not exists events_usage_updates on events (conversation_id, seq) where kind = 'acp_update' and payload->'update'->>'sessionUpdate' = 'usage_update';
