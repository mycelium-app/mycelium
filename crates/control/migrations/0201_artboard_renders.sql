create table artboard_renders (
    artifact_id uuid not null references artifacts(id) on delete cascade,
    version integer not null,
    artboard text not null,
    sha256 text not null,
    primary key (artifact_id, version, artboard)
);
