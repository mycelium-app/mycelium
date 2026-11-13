alter table start_proposals drop constraint start_proposals_kind_check;
alter table start_proposals add constraint start_proposals_kind_check check (kind in ('subagent', 'workflow', 'conversation'));

alter table start_proposals add column target_group_id uuid references groups(id) on delete set null;
alter table start_proposals add column started_conversation_id uuid references conversations(id) on delete set null;
