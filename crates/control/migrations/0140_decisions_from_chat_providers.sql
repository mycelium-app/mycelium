alter table decision_providers add column model_provider_id uuid references model_providers(id) on delete cascade;
