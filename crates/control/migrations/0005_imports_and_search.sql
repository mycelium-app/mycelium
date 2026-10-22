alter table conversations add column imported_from text;
create unique index conversations_imported_from on conversations (node_id, imported_from) where imported_from is not null;

create table search_entries (
    conversation_id uuid not null references conversations(id) on delete cascade,
    seq bigint not null,
    text text not null,
    tsv tsvector generated always as (to_tsvector('english', text)) stored,
    primary key (conversation_id, seq)
);

create index search_entries_tsv on search_entries using gin (tsv);
