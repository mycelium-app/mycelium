update chat_tool_servers set policy = jsonb_build_object(
        'preset', case
            when kind = 'zulip' then 'ask_all_edit'
            when kind <> 'mcp' and policy #>> '{ask,all}' = 'true' then 'ask_all'
            when kind <> 'mcp' and policy #>> '{ask,all}' = 'false' then 'dont_ask'
            when kind <> 'mcp' then 'ask_writes'
            when policy #>> '{ask,all}' = 'true' then 'ask_all'
            when policy #>> '{ask,all}' = 'false' or always_allow then 'dont_ask'
            else 'ask_all'
        end,
        'ask', coalesce(policy->'ask', '{}'::jsonb) - 'all',
        'edit', coalesce(policy->'review', '{}'::jsonb) - 'all'
    );

alter table integration_calls drop constraint integration_calls_review_check;
update integration_calls set review = case review when 'removed' then 'edited' when 'withheld' then 'not_sent' else review end;
alter table integration_calls add constraint integration_calls_review_check check (review in ('shared', 'edited', 'not_sent'));
