alter table goals alter column token_budget drop not null;
alter table goals add column time_limit_seconds bigint;
alter table goals add column five_hour_limit smallint;
alter table goals add column weekly_limit smallint;
