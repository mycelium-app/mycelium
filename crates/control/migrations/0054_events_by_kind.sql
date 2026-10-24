-- no-transaction
create index concurrently if not exists events_conversation_kind on events (conversation_id, kind, seq) where kind in ('user_message', 'turn_end', 'workflow', 'permission_request', 'permission_resolved', 'error');
