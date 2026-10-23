create table group_folders (
    id uuid primary key default gen_random_uuid(),
    group_id uuid not null references groups(id) on delete cascade,
    node_id uuid references nodes(id) on delete cascade,
    prefix text not null check (prefix like '/%' and prefix not like '%/'),
    created_at timestamptz not null default now()
);

create unique index group_folders_claim on group_folders (group_id, coalesce(node_id, '00000000-0000-0000-0000-000000000000'::uuid), prefix);

create function conversation_group(folder text, on_node uuid, in_project uuid) returns uuid
language sql stable as $$
    select coalesce(
        (select group_id from projects where id = in_project),
        (select claim.group_id from group_folders claim
         where (claim.node_id is null or claim.node_id = on_node)
           and (folder = claim.prefix or left(folder, length(claim.prefix) + 1) = claim.prefix || '/')
         order by length(claim.prefix) desc, claim.node_id is not null desc
         limit 1)
    )
$$;
