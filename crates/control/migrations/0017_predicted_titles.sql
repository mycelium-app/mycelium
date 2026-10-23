alter table conversations add column title_user_set boolean not null default false;
alter table conversations add column predicted_title text;

update conversations set title_user_set = true, predicted_title = title;
