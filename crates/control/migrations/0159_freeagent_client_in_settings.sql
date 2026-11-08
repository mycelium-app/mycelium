update chat_tool_servers set settings = coalesce(settings, '{}'::jsonb) || jsonb_build_object('client_id', account), account = null where kind = 'freeagent' and account is not null;
