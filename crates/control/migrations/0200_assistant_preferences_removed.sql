update assistants set preferences = preferences - 'models' - 'nodes' - 'chat_first' - 'fall_back_on';

alter table assistants drop column limited;
