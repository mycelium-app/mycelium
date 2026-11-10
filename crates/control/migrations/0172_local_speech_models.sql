create table local_speech_models (
    owner uuid primary key references users(id) on delete cascade,
    model_id text not null,
    chosen_at timestamptz not null default now()
);
