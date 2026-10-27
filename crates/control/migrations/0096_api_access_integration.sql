alter table api_access_requests
    add column server_id uuid references chat_tool_servers(id) on delete cascade;
