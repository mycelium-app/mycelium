alter table chat_tool_servers add column tool_prefix text;
update chat_tool_servers set tool_prefix = trim(both '_' from regexp_replace(lower(trim(name)), '[^a-z0-9]', '_', 'g'));
alter table chat_tool_servers alter column tool_prefix set not null;
create unique index chat_tool_servers_tool_prefix on chat_tool_servers (owner, tool_prefix);
