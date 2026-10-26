alter table chat_tool_servers
    add column session_key text,
    add column forge_id uuid references forges(id) on delete cascade;
