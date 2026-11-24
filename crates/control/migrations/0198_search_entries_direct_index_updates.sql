alter index search_entries_tsv set (fastupdate = off);
select gin_clean_pending_list('search_entries_tsv'::regclass);
