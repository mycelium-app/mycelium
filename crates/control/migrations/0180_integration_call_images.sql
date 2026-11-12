create table integration_call_images (
    call_id uuid not null references integration_calls(id) on delete cascade,
    url text not null,
    page_url text,
    primary key (call_id, url)
);
