alter table logins drop constraint logins_node_id_agent_key;
create unique index logins_node_agent_account on logins (node_id, agent, coalesce(account, ''));

alter table conversations add column account text;

alter table web_sessions
    add column user_agent text not null default '',
    add column address text not null default '',
    add column last_used_at timestamptz;

alter table client_tokens
    add column hostname text not null default '',
    add column address text not null default '';

alter table nodes add column address text not null default '';
