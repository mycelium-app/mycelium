update conversations set archived_at = updated_at where imported_from is not null and archived_at is null;
