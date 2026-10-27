alter table goals add column delivery_state text not null default 'sent' check (delivery_state in ('pending', 'sent', 'failed'));
alter table goals add column delivered_at timestamptz;
alter table goals add column delivery_reason text;
alter table goals add column held_since timestamptz;
alter table goals add column held_seconds bigint not null default 0;
alter table queued_prompts add column origin text;
