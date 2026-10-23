create table walkthrough_reads (
    walkthrough_id uuid not null references walkthroughs(id) on delete cascade,
    user_id uuid not null references users(id) on delete cascade,
    step integer not null,
    primary key (walkthrough_id, user_id, step)
);
