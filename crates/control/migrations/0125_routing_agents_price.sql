alter table conversations add column routing text;

alter table schedules add column account text;
alter table schedules add column routing text;

alter table user_settings add column default_mode_claude text;
alter table user_settings add column default_mode_codex text;
alter table user_settings add column any_model jsonb not null default '[]';

create table account_settings (
    owner uuid not null references users(id) on delete cascade,
    agent text not null,
    account text not null,
    default_mode text,
    price_amount double precision,
    price_currency text,
    price_period text check (price_period in ('monthly', 'yearly')),
    primary key (owner, agent, account)
);

create table exchange_rates (
    day date not null,
    currency text not null,
    usd_per_unit double precision not null,
    primary key (day, currency)
);
