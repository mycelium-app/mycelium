drop function conversation_group(text, uuid, uuid);

create function conversation_group(folder text, on_node uuid, in_project uuid, of_owner uuid) returns uuid
language sql stable as $$
    select coalesce(
        (select group_id from projects where id = in_project),
        (select claim.group_id from group_folders claim join groups g on g.id = claim.group_id
         where g.owner = of_owner
           and (claim.node_id is null or claim.node_id = on_node)
           and (folder = claim.prefix or left(folder, length(claim.prefix) + 1) = claim.prefix || '/')
         order by length(claim.prefix) desc, claim.node_id is not null desc
         limit 1)
    )
$$;

alter table memories add column owner uuid references users(id) on delete cascade;

update memories m set owner = coalesce(
    (select b.owner from memory_banks b where b.id = m.bank_id),
    (select c.owner from conversations c where c.id = m.conversation_id),
    (select n.created_by from nodes n where n.id = m.node_id)
);

update memories set owner = (select id from users) where owner is null and (select count(*) from users) = 1;

create index memories_owner on memories (owner);
