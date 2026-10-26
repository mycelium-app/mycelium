alter table user_settings
    add column experiment_tasks boolean not null default false,
    add column experiment_workflows boolean not null default false,
    add column experiment_instructions boolean not null default false,
    add column experiment_details boolean not null default false,
    add column experiment_subagents boolean not null default false,
    add column experiment_walkthrough boolean not null default false,
    add column experiment_move_chats boolean not null default false;
