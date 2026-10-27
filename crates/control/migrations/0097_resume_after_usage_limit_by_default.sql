alter table user_settings alter column on_usage_limit set default 'resume';
update user_settings set on_usage_limit = 'resume' where on_usage_limit = 'ask';
