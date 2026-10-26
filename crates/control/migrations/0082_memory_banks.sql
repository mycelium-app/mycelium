create table memory_banks (
    id uuid primary key default gen_random_uuid(),
    owner uuid not null references users(id) on delete cascade,
    name text not null,
    model_class text not null check (model_class in ('private', 'zdr', 'cloud')),
    group_ids uuid[] not null default '{}',
    created_at timestamptz not null default now(),
    unique (owner, name)
);

alter table memories add column bank_id uuid references memory_banks(id) on delete restrict;
alter table model_providers add column model_class text not null default 'cloud' check (model_class in ('private', 'zdr', 'cloud'));
alter table user_settings add column experiment_memory_banks boolean not null default false;
