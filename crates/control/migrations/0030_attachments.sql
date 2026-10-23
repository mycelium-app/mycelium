create table attachments (
    owner uuid not null references users(id) on delete cascade,
    sha256 text not null check (sha256 ~ '^[0-9a-f]{64}$'),
    media_type text not null,
    size bigint not null,
    created_at timestamptz not null default now(),
    primary key (owner, sha256)
);

alter table queued_prompts add column attachments jsonb not null default '[]';
