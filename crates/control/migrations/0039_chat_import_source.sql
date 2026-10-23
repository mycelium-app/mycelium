alter table conversations add column import_source text;
create unique index conversations_import_source on conversations (owner, import_source) where import_source is not null;
