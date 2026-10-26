alter table assistants add column backup jsonb;
alter table assistant_files add column backed_up_sha256 text;
