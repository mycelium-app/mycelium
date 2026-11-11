alter table model_providers add column admin_key text;

alter table model_provider_usage add column attempted_at timestamptz;
alter table model_provider_usage add column status text not null default 'ok' check (status in ('ok', 'refused', 'unreadable'));
alter table model_provider_usage alter column spend drop not null;
alter table model_provider_usage alter column read_at drop not null;
