alter table walkthroughs add column reviewed integer[] not null default '{}';
drop table walkthrough_reads;
