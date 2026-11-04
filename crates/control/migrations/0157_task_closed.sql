alter table tasks drop constraint tasks_status_check;
alter table tasks add constraint tasks_status_check check (status in ('question', 'proposed', 'open', 'claimed', 'done', 'declined', 'closed'));
