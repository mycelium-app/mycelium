alter table conversations add column resume_at timestamptz;
alter table conversations add column limit_resumes integer not null default 0;

create index conversations_resume_due on conversations (resume_at) where resume_at is not null;

alter table user_settings add column on_usage_limit text not null default 'ask';
