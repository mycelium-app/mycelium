alter table conversations add column parent_id uuid references conversations(id) on delete cascade;
alter table conversations add column side_kind text;

create index conversations_parent on conversations (parent_id);
