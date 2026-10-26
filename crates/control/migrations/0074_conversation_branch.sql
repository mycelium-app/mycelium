alter table conversations
    add column branch text,
    add column pull_request_label text,
    add column pull_request_url text;
