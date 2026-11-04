update user_settings set default_mode_codex = case default_mode_codex when 'auto' then 'workspace-write' else 'agent-full-access' end where default_mode_codex in ('auto', 'full-access');
update account_settings set default_mode = case default_mode when 'auto' then 'workspace-write' else 'agent-full-access' end where agent = 'codex' and default_mode in ('auto', 'full-access');
