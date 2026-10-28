alter table artifacts drop constraint artifacts_kind_check;
alter table artifacts add constraint artifacts_kind_check check (kind in ('design', 'page', 'markdown', 'file', 'drawing', 'typst', 'doc', 'slides', 'dashboard', 'motion', 'design_system'));
alter table artifacts add column design_system_id uuid references artifacts(id) on delete set null;
