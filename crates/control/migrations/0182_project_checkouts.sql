create table if not exists project_checkouts (
    id uuid primary key default gen_random_uuid(),
    project_id uuid not null references projects(id) on delete cascade,
    node_id uuid not null references nodes(id) on delete cascade,
    path text not null,
    created_at timestamptz not null default now(),
    unique (node_id, path),
    unique (project_id, node_id)
);

insert into project_checkouts (id, project_id, node_id, path, created_at)
select id, id, node_id, path, created_at from projects
on conflict do nothing;

create or replace function project_first_checkout() returns trigger
language plpgsql as $$
begin
    insert into project_checkouts (id, project_id, node_id, path, created_at)
    values (new.id, new.id, new.node_id, new.path, new.created_at);

    return null;
end
$$;

drop trigger if exists projects_first_checkout on projects;
create trigger projects_first_checkout after insert on projects
for each row execute function project_first_checkout();

alter table worktrees add column if not exists checkout_id uuid references project_checkouts(id) on delete cascade;
update worktrees set checkout_id = project_id where checkout_id is null;
alter table worktrees alter column checkout_id set not null;
alter table worktrees drop constraint if exists worktrees_project_id_branch_key;
create unique index if not exists worktrees_checkout_branch on worktrees (checkout_id, branch);

create temporary table project_merge on commit drop as
with ranked as (
    select id, first_value(id) over same_repository as survivor, created_at,
           row_number() over (partition by owner, forge_id, lower(repository), node_id order by created_at, id) as on_node
    from projects
    where forge_id is not null and repository is not null
    window same_repository as (partition by owner, forge_id, lower(repository) order by created_at, id)
)
select duplicate.id as duplicate, duplicate.survivor
from ranked duplicate
where duplicate.id <> duplicate.survivor
  and duplicate.on_node = 1
  and not exists (
      select 1
      from artifact_links held
      join artifact_links other on other.artifact_id = held.artifact_id and other.project_id <> held.project_id
      join ranked other_project on other_project.id = other.project_id
      where held.project_id = duplicate.id
        and other_project.survivor = duplicate.survivor
        and (other_project.id = duplicate.survivor or (other_project.created_at, other_project.id) < (duplicate.created_at, duplicate.id))
  );

create temporary table project_merge_members on commit drop as
select survivor, survivor as member from project_merge group by survivor
union all
select survivor, duplicate from project_merge;

update projects survivor_row
set group_id = chosen.group_id
from (
    select distinct on (members.survivor) members.survivor, member_row.group_id
    from project_merge_members members
    join projects member_row on member_row.id = members.member
    where member_row.group_id is not null
    order by members.survivor, (member_row.id = members.survivor) desc, member_row.created_at, member_row.id
) chosen
where survivor_row.id = chosen.survivor and survivor_row.group_id is null;

update projects survivor_row
set test_command = chosen.test_command
from (
    select distinct on (members.survivor) members.survivor, member_row.test_command
    from project_merge_members members
    join projects member_row on member_row.id = members.member
    where member_row.test_command is not null
    order by members.survivor, (member_row.id = members.survivor) desc, member_row.created_at, member_row.id
) chosen
where survivor_row.id = chosen.survivor and survivor_row.test_command is null;

update projects survivor_row
set icon = chosen.icon, avatar_sha256 = chosen.avatar_sha256, avatar_automatic = chosen.avatar_automatic
from (
    select distinct on (members.survivor) members.survivor, member_row.icon, member_row.avatar_sha256, member_row.avatar_automatic
    from project_merge_members members
    join projects member_row on member_row.id = members.member
    cross join lateral (
        select case
            when member_row.icon is not null or (member_row.avatar_sha256 is not null and not member_row.avatar_automatic) then 2
            when member_row.avatar_sha256 is not null then 1
            else 0
        end as strength
    ) ranking
    where ranking.strength > 0
    order by members.survivor, ranking.strength desc, (member_row.id = members.survivor) desc, member_row.created_at, member_row.id
) chosen
where survivor_row.id = chosen.survivor;

update conversations set project_id = merge.survivor from project_merge merge where conversations.project_id = merge.duplicate;
update schedules set project_id = merge.survivor from project_merge merge where schedules.project_id = merge.duplicate;
update usage_turns set project_id = merge.survivor from project_merge merge where usage_turns.project_id = merge.duplicate;
update memories set project_id = merge.survivor from project_merge merge where memories.project_id = merge.duplicate;
update exercises set project_id = merge.survivor from project_merge merge where exercises.project_id = merge.duplicate;
update workflows set project_id = merge.survivor from project_merge merge where workflows.project_id = merge.duplicate;
update artifact_links set project_id = merge.survivor from project_merge merge where artifact_links.project_id = merge.duplicate;
update worktrees set project_id = merge.survivor from project_merge merge where worktrees.project_id = merge.duplicate;
update project_checkouts set project_id = merge.survivor from project_merge merge where project_checkouts.project_id = merge.duplicate;

do $$
declare
    reference record;
    remaining bigint;
begin
    for reference in
        select constraint_row.conrelid::regclass as source, attribute.attname as source_column
        from pg_constraint constraint_row
        join pg_attribute attribute on attribute.attrelid = constraint_row.conrelid and attribute.attnum = constraint_row.conkey[1]
        where constraint_row.contype = 'f' and constraint_row.confrelid = 'projects'::regclass
    loop
        execute format('select count(*) from %s where %I in (select duplicate from project_merge)', reference.source, reference.source_column) into remaining;

        if remaining > 0 then
            raise exception 'Merging duplicate projects stopped: % row(s) in % still point at a duplicate project. Nothing was changed.', remaining, reference.source;
        end if;
    end loop;
end
$$;

delete from projects where id in (select duplicate from project_merge);

create index if not exists projects_repository on projects (owner, forge_id, lower(repository)) where repository is not null;

create or replace function project_for_folder(of_owner uuid, on_node uuid, folder text) returns uuid
language sql stable as $$
    select project.id
    from project_checkouts checkout
    join projects project on project.id = checkout.project_id
    where project.owner = of_owner and checkout.node_id = on_node
      and (folder = checkout.path
           or left(folder, length(checkout.path) + 1) = checkout.path || '/'
           or left(folder, length(checkout.path) + 11) = checkout.path || '-worktrees/')
    order by length(checkout.path) desc
    limit 1
$$;
