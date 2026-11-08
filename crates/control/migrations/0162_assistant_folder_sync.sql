alter table assistant_files add column folder_sha256 text;
update assistant_files set folder_sha256 = sha256;
