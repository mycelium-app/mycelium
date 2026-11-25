delete from mention_triggers older using mention_triggers newer
where newer.server_id = older.server_id and (newer.created_at, newer.id) > (older.created_at, older.id);

create unique index mention_triggers_one_per_account on mention_triggers (server_id);
