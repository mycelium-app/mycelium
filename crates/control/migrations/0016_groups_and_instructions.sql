create table groups (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    name text not null,
    created_at timestamptz not null default now(),
    unique (owner, name)
);

alter table projects add column group_id uuid references groups(id) on delete set null;
alter table conversations add column project_id uuid references projects(id) on delete set null;
alter table conversations add column instructions_revision text;

create table instruction_versions (
    id text primary key,
    seq bigserial not null unique,
    layer text not null,
    group_id uuid references groups(id) on delete cascade,
    text text not null,
    author uuid references users(id) on delete set null,
    created_at timestamptz not null default now()
);

create index instruction_versions_layer on instruction_versions (layer, seq desc);

create table instruction_source (
    singleton boolean primary key default true check (singleton),
    forge_id uuid references forges(id) on delete cascade,
    repository text not null,
    path text not null
);

alter table nodes
    add column instructions_revision text,
    add column instructions_unmanaged text[] not null default '{}';
