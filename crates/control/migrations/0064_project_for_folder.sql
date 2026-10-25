create function project_for_folder(of_owner uuid, on_node uuid, folder text) returns uuid
language sql stable as $$
    select id from projects
    where owner = of_owner and node_id = on_node
      and (folder = path
           or left(folder, length(path) + 1) = path || '/'
           or left(folder, length(path) + 11) = path || '-worktrees/')
    order by length(path) desc
    limit 1
$$;

update conversations
set project_id = project_for_folder(owner, node_id, cwd)
where project_id is null and project_for_folder(owner, node_id, cwd) is not null;
