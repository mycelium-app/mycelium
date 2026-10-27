create table hours (
    owner uuid not null references users(id) on delete cascade,
    group_id uuid references groups(id) on delete cascade,
    weekday smallint not null check (weekday between 0 and 6),
    closed boolean not null,
    opens time not null,
    closes time not null,
    unique nulls not distinct (owner, group_id, weekday)
);

alter table groups add column always_open boolean not null default true;
alter table user_settings add column app_always_open boolean not null default true;

insert into hours (owner, group_id, weekday, closed, opens, closes)
select owner, null, weekday,
       enabled and all_day,
       case when enabled and not all_day then ends when enabled then time '09:00' else time '00:00' end,
       case when enabled and not all_day then starts when enabled then time '18:00' else time '24:00' end
from quiet_hours;

update user_settings set app_always_open = not quiet_hours;

alter table user_settings drop column quiet_hours;
drop table quiet_hours;

create table out_of_hours_opens (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    group_id uuid not null references groups(id) on delete cascade,
    reason text not null check (length(trim(reason)) > 0),
    opened_at timestamptz not null default now(),
    until timestamptz not null,
    closed_at timestamptz
);

create index out_of_hours_opens_by_owner on out_of_hours_opens (owner, opened_at desc);
