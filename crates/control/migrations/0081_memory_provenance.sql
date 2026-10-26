alter table memories
    add column model text,
    add column agent text,
    add column node_id uuid references nodes(id) on delete set null,
    add column node_name text,
    add column account text,
    add column project_id uuid references projects(id) on delete set null,
    add column project_name text,
    add column message_seq bigint,
    add column tool_call_id text;
