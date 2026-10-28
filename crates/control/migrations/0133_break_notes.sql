alter table breaks add column planned_minutes integer;
alter table breaks add column note text;
alter table breaks add column noted_at timestamptz;

update breaks set planned_minutes = greatest(1, round(extract(epoch from ends_at - started_at) / 60)::integer), noted_at = coalesce(ended_early_at, ends_at);

alter table breaks alter column planned_minutes set not null;
