create table puzzle_days (
    puzzle text not null,
    day date not null,
    payload jsonb not null,
    fetched_at timestamptz not null default now(),
    primary key (puzzle, day)
);

create table puzzle_games (
    owner uuid not null references users(id) on delete cascade,
    puzzle text not null,
    day date not null,
    guesses jsonb not null default '[]',
    started_at timestamptz not null default now(),
    finished_at timestamptz,
    primary key (owner, puzzle, day)
);
