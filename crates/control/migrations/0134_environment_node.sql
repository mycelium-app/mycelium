alter table user_settings add column environment_node_id uuid references nodes(id) on delete set null;
