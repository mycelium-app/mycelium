alter table puzzle_plays
    add column solved boolean,
    add column guesses integer,
    add column mistakes integer,
    add column score text,
    add column source text not null default 'manual' check (source in ('manual', 'read')),
    add column recorded_at timestamptz not null default now();
