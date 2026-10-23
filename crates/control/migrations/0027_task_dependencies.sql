alter table tasks add column depends_on uuid[] not null default '{}';
