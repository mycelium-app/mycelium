create table quiet_holds (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    kind text not null check (kind in ('notification', 'ping')),
    subject text not null,
    held_at timestamptz not null default now(),
    released_at timestamptz
);

create unique index quiet_holds_open on quiet_holds (owner, kind, subject) where released_at is null;
create index quiet_holds_by_owner on quiet_holds (owner, held_at desc);
