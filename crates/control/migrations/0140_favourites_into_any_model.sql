insert into user_settings (owner)
select distinct owner from model_providers where cardinality(favourite_models) > 0
on conflict (owner) do nothing;

update user_settings settings
set any_model = settings.any_model || coalesce((
    select jsonb_agg(jsonb_build_object('provider_id', provider.id, 'model', starred.model) order by provider.created_at, starred.position)
    from model_providers provider
    cross join lateral unnest(provider.favourite_models) with ordinality as starred(model, position)
    where provider.owner = settings.owner
      and not exists (
          select 1 from jsonb_array_elements(settings.any_model) entry
          where entry ->> 'provider_id' = provider.id::text and entry ->> 'model' = starred.model
      )
), '[]'::jsonb);
