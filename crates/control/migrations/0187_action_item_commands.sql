alter table conversation_action_items
    add column command text,
    add column node_id uuid references nodes (id) on delete set null,
    add column folder text,
    add column run_terminal_id uuid,
    add column run_status text check (run_status in ('running', 'succeeded', 'failed', 'stopped')),
    add column run_started_at timestamptz,
    add column run_exit_code integer,
    add column run_output text,
    add column run_reason text;

create unique index conversation_action_items_run_terminal on conversation_action_items (run_terminal_id) where run_terminal_id is not null;

create or replace function conversation_action_items(conversation uuid) returns jsonb
language sql stable as $$
    select coalesce(jsonb_agg(jsonb_strip_nulls(jsonb_build_object(
        'id', i.id,
        'text', i.text,
        'created_at', i.created_at,
        'command', i.command,
        'node_id', i.node_id,
        'node_name', n.name,
        'folder', i.folder,
        'run', case when i.run_status is null then null else jsonb_build_object(
            'terminal_id', i.run_terminal_id,
            'status', i.run_status,
            'started_at', i.run_started_at,
            'exit_code', i.run_exit_code,
            'output', coalesce(i.run_output, ''),
            'reason', i.run_reason
        ) end
    )) order by i.created_at), '[]'::jsonb)
    from conversation_action_items i
    left join nodes n on n.id = i.node_id
    where i.conversation_id = conversation and i.done_at is null
$$;
