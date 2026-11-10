alter table group_connections drop constraint group_connections_check;
alter table group_connections drop constraint group_connections_owner_source_group_id_target_group_id_key;

alter table group_connections add column both_ways boolean not null default true;
alter table group_connections alter column both_ways set default false;

create unique index group_connections_owner_pair on group_connections (owner, least(source_group_id, target_group_id), greatest(source_group_id, target_group_id));
