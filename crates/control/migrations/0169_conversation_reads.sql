alter table conversations add column last_message_seq bigint not null default 0;

create table conversation_reads (
    user_id uuid not null references users (id) on delete cascade,
    conversation_id uuid not null references conversations (id) on delete cascade,
    seq bigint not null default 0,
    primary key (user_id, conversation_id)
);

create index conversation_reads_conversation on conversation_reads (conversation_id);
