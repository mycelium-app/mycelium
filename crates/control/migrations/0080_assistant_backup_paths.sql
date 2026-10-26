alter table assistants add column backup_paths text[] not null default '{}';
update assistants set backup_paths = coalesce((select array_agg(path) from assistant_files where assistant_id = assistants.id and backed_up_sha256 is not null), '{}');
