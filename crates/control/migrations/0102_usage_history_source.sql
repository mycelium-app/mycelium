alter table login_usage_history add column source text not null default 'mycelium';
alter table logins add column plan text;
