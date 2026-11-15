create table trace_spans (
    id bigserial primary key,
    trace_id uuid not null,
    owner uuid references users(id) on delete cascade,
    position integer not null,
    parent integer,
    name text not null,
    label text not null,
    started_at timestamptz not null,
    duration_ms double precision not null,
    fields jsonb not null default '{}'
);

create index trace_spans_owner_roots on trace_spans (owner, started_at desc) where position = 0;
create index trace_spans_trace on trace_spans (trace_id, position);
create index trace_spans_started_at on trace_spans (started_at);
