alter table chat_tool_servers add column oauth_refresh_token text, add column oauth_expires_at timestamptz, add column oauth jsonb;
