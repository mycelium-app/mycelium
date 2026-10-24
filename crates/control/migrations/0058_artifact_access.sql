create table artifact_links (
    id uuid primary key default gen_random_uuid(),
    artifact_id uuid not null references artifacts(id) on delete cascade,
    project_id uuid references projects(id) on delete cascade,
    conversation_id uuid references conversations(id) on delete cascade,
    permission text not null check (permission in ('view', 'edit')),
    check ((project_id is null) <> (conversation_id is null))
);
create unique index artifact_links_project on artifact_links (artifact_id, project_id) where project_id is not null;
create unique index artifact_links_conversation on artifact_links (artifact_id, conversation_id) where conversation_id is not null;

insert into artifact_links (artifact_id, project_id, conversation_id, permission)
select a.id, c.project_id, case when c.project_id is null then c.id end, 'edit'
from artifacts a join conversations c on c.id = a.conversation_id;

create table artifact_access_requests (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    artifact_id uuid not null references artifacts(id) on delete cascade,
    conversation_id uuid not null references conversations(id) on delete cascade,
    permission text not null check (permission in ('view', 'edit')),
    reason text not null,
    state text not null default 'requested' check (state in ('requested', 'approved', 'declined')),
    created_at timestamptz not null default now(),
    decided_at timestamptz
);
create unique index artifact_access_pending on artifact_access_requests (artifact_id, conversation_id, permission) where state = 'requested';
