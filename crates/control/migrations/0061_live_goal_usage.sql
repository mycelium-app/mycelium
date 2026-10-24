create table transcript_usage_positions (
    node_id uuid not null references nodes(id) on delete cascade,
    path text not null,
    parsed_offset bigint not null default 0,
    primary key (node_id, path)
);

create table conversation_usage_samples (
    conversation_id uuid not null references conversations(id) on delete cascade,
    source text not null,
    sample_key text not null,
    position bigint not null,
    recorded_at timestamptz not null,
    tokens bigint not null check (tokens >= 0),
    last_tokens bigint not null check (last_tokens >= 0),
    cumulative boolean not null,
    primary key (conversation_id, source, sample_key)
);

create index conversation_usage_samples_time on conversation_usage_samples (conversation_id, recorded_at);
