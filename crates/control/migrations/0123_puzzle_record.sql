alter table puzzle_plays
    add column seconds integer,
    add column points integer;

update puzzle_plays set points = coalesce(guesses, mistakes, nullif(substring(score from '^[0-9]+'), '')::integer);

update puzzle_plays set points = null where puzzle = 'wordle' and solved = false;

alter table puzzle_plays
    drop column guesses,
    drop column mistakes,
    drop column score,
    drop constraint puzzle_plays_source_check;

alter table puzzle_plays rename column points to score;

alter table puzzle_plays add constraint puzzle_plays_source_check check (source in ('manual', 'read', 'native'));
