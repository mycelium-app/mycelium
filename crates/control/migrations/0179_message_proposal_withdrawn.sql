alter table message_proposals drop constraint message_proposals_state_check;
alter table message_proposals add constraint message_proposals_state_check check (state in ('proposed', 'sent', 'declined', 'withdrawn', 'superseded'));
