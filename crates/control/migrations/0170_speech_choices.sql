create table speech_choices (
    owner uuid not null references users(id) on delete cascade,
    purpose text not null check (purpose in ('speech_to_text', 'text_to_speech')),
    provider_id uuid not null references model_providers(id) on delete cascade,
    model text not null,
    primary key (owner, purpose)
);
