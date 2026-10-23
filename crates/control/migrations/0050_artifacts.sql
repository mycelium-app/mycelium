create table artifacts (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    conversation_id uuid references conversations(id) on delete set null,
    kind text not null check (kind in ('design', 'page', 'markdown', 'file')),
    title text not null,
    slug text not null unique check (slug ~ '^[a-z0-9][a-z0-9-]*$'),
    public boolean not null default false,
    latest_version integer not null default 1,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index artifacts_owner on artifacts (owner, updated_at desc);

create table artifact_versions (
    artifact_id uuid not null references artifacts(id) on delete cascade,
    version integer not null,
    author text not null check (author in ('agent', 'person')),
    media_type text not null,
    sha256 text not null check (sha256 ~ '^[0-9a-f]{64}$'),
    note text,
    created_at timestamptz not null default now(),
    primary key (artifact_id, version)
);

create table artifact_comments (
    id uuid primary key default gen_random_uuid(),
    artifact_id uuid not null references artifacts(id) on delete cascade,
    author_name text not null,
    body text not null,
    anchor jsonb,
    resolved boolean not null default false,
    created_at timestamptz not null default now()
);

create index artifact_comments_artifact on artifact_comments (artifact_id, created_at);

create table artifact_data (
    artifact_id uuid not null references artifacts(id) on delete cascade,
    key text not null,
    value jsonb not null,
    updated_at timestamptz not null default now(),
    primary key (artifact_id, key)
);
