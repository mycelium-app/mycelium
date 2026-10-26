alter table user_settings
    add column break_reminders boolean not null default true,
    add column break_after_minutes integer not null default 50,
    add column snooze_minutes integer not null default 10,
    add column break_minutes integer not null default 5,
    add column pomodoro boolean not null default false,
    add column quiet_hours boolean not null default false,
    add column quiet_hold_notifications boolean not null default true,
    add column quiet_hold_schedules boolean not null default true,
    add column quiet_hold_pings boolean not null default true,
    add column time_zone text;

create table quiet_hours (
    owner uuid not null references users(id) on delete cascade,
    weekday smallint not null check (weekday between 0 and 6),
    enabled boolean not null,
    all_day boolean not null,
    starts time not null,
    ends time not null,
    primary key (owner, weekday)
);

create table viewing_time (
    owner uuid not null references users(id) on delete cascade,
    day date not null,
    seconds integer not null default 0,
    primary key (owner, day)
);

create table breaks (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    started_at timestamptz not null default now(),
    ends_at timestamptz not null,
    ended_early_at timestamptz
);

create index breaks_by_owner on breaks (owner, started_at desc);

create table puzzle_plays (
    owner uuid not null references users(id) on delete cascade,
    puzzle text not null,
    day date not null,
    primary key (owner, puzzle, day)
);
