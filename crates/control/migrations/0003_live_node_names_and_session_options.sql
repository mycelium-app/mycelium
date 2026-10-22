alter table nodes drop constraint nodes_name_key;
create unique index nodes_live_name on nodes (name) where revoked_at is null;
alter table conversations add column options jsonb;
