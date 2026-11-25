alter table assistant_action_proposals
    add column pinned boolean not null default false,
    add column action_id uuid references assistant_actions(id) on delete cascade,
    add column before jsonb;
