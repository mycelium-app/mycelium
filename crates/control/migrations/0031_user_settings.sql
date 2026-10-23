create table user_settings (
    owner uuid primary key references users(id) on delete cascade,
    generated_ui boolean not null default true,
    rich_markdown boolean not null default true
);
