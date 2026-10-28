alter table goals add column nudges integer not null default 0;
alter table goals add column repeats integer not null default 0;
alter table goals add column repeat_limit integer;
alter table goals add column next_nudge_at timestamptz;
alter table goals add column wait_reason text check (wait_reason in ('repeated_outcome', 'question_waiting', 'background_work'));
alter table goals add column last_outcome text;
alter table goals add column why text;
alter table goals add column note text;
alter table goals add column reported_at timestamptz;
