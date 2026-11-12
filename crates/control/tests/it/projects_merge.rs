use crate::common;
use sqlx::PgPool;
use sqlx::migrate::Migrator;
use std::borrow::Cow;
use uuid::Uuid;

const MERGE_MIGRATION: &str = include_str!("../../migrations/0182_project_checkouts.sql");
const LAST_VERSION_BEFORE_CHECKOUTS: i64 = 181;
const CHOSEN_AVATAR: &str = "b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0";
const AUTOMATIC_AVATAR: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";

struct Seeded {
    sam: Uuid,
    alex: Uuid,
    atlas: Uuid,
    orion: Uuid,
    willow: Uuid,
    homelab: Uuid,
    backups_atlas: Uuid,
    backups_orion: Uuid,
    backups_willow: Uuid,
    alex_backups: Uuid,
    notes: Uuid,
    dashboards_atlas: Uuid,
    dashboards_orion: Uuid,
    archive_first: Uuid,
    archive_second: Uuid,
    reports_atlas: Uuid,
    reports_orion: Uuid,
    conversations: Vec<Uuid>,
}

async fn insert_id(pool: &PgPool, sql: &str, binds: &[&str]) -> Uuid {
    let mut query = sqlx::query_scalar::<_, Uuid>(sqlx::AssertSqlSafe(sql.to_string()));

    for bind in binds {
        query = query.bind(*bind);
    }

    query.fetch_one(pool).await.unwrap()
}

async fn before_checkouts() -> PgPool {
    let url = common::create_database().await;
    let pool = PgPool::connect(&url).await.unwrap();
    let mut migrator: Migrator = sqlx::migrate!("./migrations");
    let upto: Vec<_> = migrator.migrations.iter().filter(|migration| migration.version <= LAST_VERSION_BEFORE_CHECKOUTS).cloned().collect();

    migrator.migrations = Cow::Owned(upto);
    migrator.run(&pool).await.unwrap();

    pool
}

async fn user(pool: &PgPool, name: &str) -> Uuid {
    insert_id(pool, &format!("insert into users (subject, email, name) values ('{name}', '{name}@example.com', '{name}') returning id"), &[]).await
}

async fn node(pool: &PgPool, owner: Uuid, name: &str) -> Uuid {
    sqlx::query_scalar::<_, Uuid>("insert into nodes (name, hostname, os, arch, token_hash, created_by) values ($1, $1, 'linux', 'x86_64', $2, $3) returning id")
        .bind(name)
        .bind(name.as_bytes())
        .bind(owner)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn forge(pool: &PgPool, owner: Uuid) -> Uuid {
    sqlx::query_scalar::<_, Uuid>("insert into forges (owner, kind, base_url, account, token) values ($1, 'forgejo', 'https://git.example.com', 'account', 'token') returning id").bind(owner).fetch_one(pool).await.unwrap()
}

struct NewProject<'a> {
    owner: Uuid,
    node: Uuid,
    path: &'a str,
    forge: Option<Uuid>,
    repository: Option<&'a str>,
    age_days: i32,
}

async fn project(pool: &PgPool, new: NewProject<'_>) -> Uuid {
    sqlx::query_scalar::<_, Uuid>(
        "insert into projects (owner, node_id, name, path, forge_id, repository, created_at) values ($1, $2, split_part($3, '/', -1), $3, $4, $5, now() - make_interval(days => $6)) returning id",
    )
    .bind(new.owner)
    .bind(new.node)
    .bind(new.path)
    .bind(new.forge)
    .bind(new.repository)
    .bind(new.age_days)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn conversation(pool: &PgPool, owner: Uuid, node: Uuid, cwd: &str, project: Uuid) -> Uuid {
    sqlx::query_scalar::<_, Uuid>("insert into conversations (owner, node_id, agent, cwd, title, status, project_id) values ($1, $2, 'claude', $3, 'Restore drill', 'done', $4) returning id")
        .bind(owner)
        .bind(node)
        .bind(cwd)
        .bind(project)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn artifact(pool: &PgPool, owner: Uuid, slug: &str, linked_to: &[Uuid]) -> Uuid {
    let id = sqlx::query_scalar::<_, Uuid>("insert into artifacts (owner, kind, title, slug) values ($1, 'markdown', $2, $2) returning id").bind(owner).bind(slug).fetch_one(pool).await.unwrap();

    for project in linked_to {
        sqlx::query("insert into artifact_links (artifact_id, project_id, permission) values ($1, $2, 'edit')").bind(id).bind(project).execute(pool).await.unwrap();
    }

    id
}

async fn worktree(pool: &PgPool, project: Uuid, branch: &str, path: &str) {
    sqlx::query("insert into worktrees (project_id, branch, path) values ($1, $2, $3)").bind(project).bind(branch).bind(path).execute(pool).await.unwrap();
}

async fn seed(pool: &PgPool) -> Seeded {
    let sam = user(pool, "sam").await;
    let alex = user(pool, "alex").await;
    let atlas = node(pool, sam, "atlas").await;
    let orion = node(pool, sam, "orion").await;
    let willow = node(pool, sam, "willow").await;
    let ridge = node(pool, alex, "ridge").await;
    let (sam_forge, alex_forge) = (forge(pool, sam).await, forge(pool, alex).await);
    let homelab = insert_id(pool, "insert into groups (owner, name) values ((select id from users where subject = 'sam'), 'homelab') returning id", &[]).await;
    let repository = |owner, node, path, forge, repository, age_days| NewProject { owner, node, path, forge, repository, age_days };

    let backups_atlas = project(pool, repository(sam, atlas, "/home/sam/projects/acme-org/backups", Some(sam_forge), Some("acme-org/backups"), 30)).await;
    let backups_orion = project(pool, repository(sam, orion, "/srv/projects/acme-org/backups", Some(sam_forge), Some("Acme-Org/Backups"), 20)).await;
    let backups_willow = project(pool, repository(sam, willow, "/home/sam/backups", Some(sam_forge), Some("acme-org/backups"), 10)).await;
    let alex_backups = project(pool, repository(alex, ridge, "/home/alex/backups", Some(alex_forge), Some("acme-org/backups"), 25)).await;
    let notes = project(pool, repository(sam, willow, "/home/sam/notes", None, None, 15)).await;
    let dashboards_atlas = project(pool, repository(sam, atlas, "/home/sam/projects/acme-org/dashboards", Some(sam_forge), Some("acme-org/dashboards"), 30)).await;
    let dashboards_orion = project(pool, repository(sam, orion, "/srv/projects/acme-org/dashboards", Some(sam_forge), Some("acme-org/dashboards"), 20)).await;
    let archive_first = project(pool, repository(sam, atlas, "/home/sam/projects/acme-org/archive", Some(sam_forge), Some("acme-org/archive"), 30)).await;
    let archive_second = project(pool, repository(sam, atlas, "/home/sam/archive-again", Some(sam_forge), Some("acme-org/archive"), 20)).await;
    let reports_atlas = project(pool, repository(sam, atlas, "/home/sam/projects/acme-org/reports", Some(sam_forge), Some("acme-org/reports"), 30)).await;
    let reports_orion = project(pool, repository(sam, orion, "/srv/projects/acme-org/reports", Some(sam_forge), Some("acme-org/reports"), 20)).await;

    sqlx::query("update projects set group_id = $2 where id = $1").bind(backups_atlas).bind(homelab).execute(pool).await.unwrap();
    sqlx::query("update projects set icon = 'rocket', test_command = 'make test' where id = $1").bind(backups_orion).execute(pool).await.unwrap();
    sqlx::query("update projects set avatar_sha256 = $2, avatar_automatic = true, test_command = 'make check' where id = $1").bind(backups_willow).bind(AUTOMATIC_AVATAR).execute(pool).await.unwrap();
    sqlx::query("update projects set avatar_sha256 = $2, avatar_automatic = true where id = $1").bind(dashboards_atlas).bind(AUTOMATIC_AVATAR).execute(pool).await.unwrap();
    sqlx::query("update projects set avatar_sha256 = $2, avatar_automatic = false where id = $1").bind(dashboards_orion).bind(CHOSEN_AVATAR).execute(pool).await.unwrap();

    let first = conversation(pool, sam, atlas, "/home/sam/projects/acme-org/backups", backups_atlas).await;
    let second = conversation(pool, sam, orion, "/srv/projects/acme-org/backups-worktrees/restore-drill", backups_orion).await;
    let third = conversation(pool, sam, willow, "/home/sam/backups", backups_willow).await;

    sqlx::query("insert into schedules (owner, node_id, agent, cwd, project_id, prompt, cron, each_run, next_run_at) values ($1, $2, 'claude', '/srv/projects/acme-org/backups', $3, 'Check the backups', '0 6 * * *', 'new', now())").bind(sam).bind(orion).bind(backups_orion).execute(pool).await.unwrap();
    sqlx::query("insert into usage_turns (conversation_id, owner, node_id, agent, project_id, day) values ($1, $2, $3, 'claude', $4, current_date)").bind(second).bind(sam).bind(orion).bind(backups_orion).execute(pool).await.unwrap();
    sqlx::query("insert into memories (text, created_by, project_id, project_name) values ('Restores take ten minutes', 'claude', $1, 'backups')").bind(backups_willow).execute(pool).await.unwrap();
    sqlx::query("insert into exercises (owner, kind, name, title, brief, node_id, project_id, agent) values ($1, 'area', 'restores', 'Restores', 'Practise a restore', $2, $3, 'claude')").bind(sam).bind(orion).bind(backups_orion).execute(pool).await.unwrap();
    sqlx::query("insert into workflows (id, owner, project_id, title, plan) values (gen_random_uuid(), $1, $2, 'Restore drill', '{}')").bind(sam).bind(backups_orion).execute(pool).await.unwrap();

    artifact(pool, sam, "restore-notes", &[backups_atlas]).await;
    artifact(pool, sam, "orion-runbook", &[backups_orion]).await;
    artifact(pool, sam, "shared-report", &[reports_atlas, reports_orion]).await;

    worktree(pool, backups_atlas, "restore-drill", "/home/sam/projects/acme-org/backups-worktrees/restore-drill").await;
    worktree(pool, backups_orion, "restore-drill", "/srv/projects/acme-org/backups-worktrees/restore-drill").await;

    Seeded { sam, alex, atlas, orion, willow, homelab, backups_atlas, backups_orion, backups_willow, alex_backups, notes, dashboards_atlas, dashboards_orion, archive_first, archive_second, reports_atlas, reports_orion, conversations: vec![first, second, third] }
}

async fn migrate_to_latest(pool: &PgPool) {
    sqlx::migrate!("./migrations").run(pool).await.unwrap();
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!("select count(*) from {table}"))).fetch_one(pool).await.unwrap()
}

async fn fingerprint(pool: &PgPool) -> String {
    let tables = ["projects", "project_checkouts", "conversations", "schedules", "usage_turns", "memories", "exercises", "workflows", "artifact_links", "worktrees", "artifacts"];
    let mut all = String::new();

    for table in tables {
        let rows = sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(format!("select coalesce(jsonb_agg(to_jsonb(t) order by to_jsonb(t)::text)::text, '[]') from {table} t"))).fetch_one(pool).await.unwrap();

        all.push_str(&rows);
    }

    all
}

const KEPT_TABLES: [&str; 8] = ["conversations", "schedules", "usage_turns", "memories", "exercises", "workflows", "artifact_links", "worktrees"];

async fn counts(pool: &PgPool) -> Vec<i64> {
    let mut found = Vec::new();

    for table in KEPT_TABLES {
        found.push(count(pool, table).await);
    }

    found
}

#[tokio::test]
async fn the_same_repository_on_several_nodes_becomes_one_project_with_a_checkout_on_each() {
    let pool = before_checkouts().await;
    let seeded = seed(&pool).await;

    migrate_to_latest(&pool).await;

    let survivors: Vec<Uuid> = sqlx::query_scalar("select id from projects where owner = $1 and lower(repository) = 'acme-org/backups'").bind(seeded.sam).fetch_all(&pool).await.unwrap();
    assert_eq!(survivors, vec![seeded.backups_atlas]);

    let mut nodes: Vec<Uuid> = sqlx::query_scalar("select node_id from project_checkouts where project_id = $1").bind(seeded.backups_atlas).fetch_all(&pool).await.unwrap();
    let mut expected = vec![seeded.atlas, seeded.orion, seeded.willow];
    nodes.sort();
    expected.sort();
    assert_eq!(nodes, expected);
}

#[tokio::test]
async fn a_merged_project_keeps_the_survivors_group_and_takes_the_first_icon_and_test_command_it_lacked() {
    let pool = before_checkouts().await;
    let seeded = seed(&pool).await;

    migrate_to_latest(&pool).await;

    let (group, icon, test_command, avatar): (Option<Uuid>, Option<String>, Option<String>, Option<String>) =
        sqlx::query_as("select group_id, icon, test_command, avatar_sha256 from projects where id = $1").bind(seeded.backups_atlas).fetch_one(&pool).await.unwrap();

    assert_eq!((group, icon.as_deref(), test_command.as_deref(), avatar), (Some(seeded.homelab), Some("rocket"), Some("make test"), None));
}

#[tokio::test]
async fn an_avatar_the_person_chose_beats_an_automatic_one_when_projects_merge() {
    let pool = before_checkouts().await;
    let seeded = seed(&pool).await;

    migrate_to_latest(&pool).await;

    let (avatar, automatic): (Option<String>, bool) = sqlx::query_as("select avatar_sha256, avatar_automatic from projects where id = $1").bind(seeded.dashboards_atlas).fetch_one(&pool).await.unwrap();

    assert_eq!((avatar.as_deref(), automatic), (Some(CHOSEN_AVATAR), false));
}

#[tokio::test]
async fn every_reference_to_a_merged_project_moves_to_the_survivor_and_nothing_else_is_deleted() {
    let pool = before_checkouts().await;
    let seeded = seed(&pool).await;
    let before = counts(&pool).await;
    let projects_before = count(&pool, "projects").await;

    migrate_to_latest(&pool).await;

    assert_eq!(counts(&pool).await, before);
    assert_eq!(count(&pool, "projects").await, projects_before - 3);

    for table in ["conversations", "schedules", "usage_turns", "memories", "exercises", "workflows"] {
        let stray = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!("select count(*) from {table} where project_id is not null and project_id not in (select id from projects)"))).fetch_one(&pool).await.unwrap();

        assert_eq!(stray, 0, "{table}");
    }

    let conversation_projects: Vec<(Uuid, Uuid, String)> = sqlx::query_as("select project_id, node_id, cwd from conversations where id = any($1) order by created_at, id").bind(&seeded.conversations).fetch_all(&pool).await.unwrap();

    assert!(conversation_projects.iter().all(|(project, _, _)| *project == seeded.backups_atlas));
    assert_eq!(conversation_projects.iter().map(|(_, node, _)| *node).collect::<Vec<_>>(), vec![seeded.atlas, seeded.orion, seeded.willow]);
    assert_eq!(conversation_projects[1].2, "/srv/projects/acme-org/backups-worktrees/restore-drill");

    let linked: Vec<Uuid> = sqlx::query_scalar("select project_id from artifact_links where project_id = $1").bind(seeded.backups_atlas).fetch_all(&pool).await.unwrap();
    assert_eq!(linked.len(), 2);
}

#[tokio::test]
async fn worktrees_of_the_same_branch_on_two_nodes_each_belong_to_their_own_checkout() {
    let pool = before_checkouts().await;
    let seeded = seed(&pool).await;

    migrate_to_latest(&pool).await;

    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as("select worktrees.project_id, checkout_id from worktrees where branch = 'restore-drill' order by checkout_id").fetch_all(&pool).await.unwrap();
    let checkout_nodes: Vec<Uuid> = sqlx::query_scalar("select node_id from project_checkouts where id = any($1) order by node_id").bind(rows.iter().map(|(_, checkout)| *checkout).collect::<Vec<_>>()).fetch_all(&pool).await.unwrap();
    let mut expected = vec![seeded.atlas, seeded.orion];
    expected.sort();

    assert!(rows.iter().all(|(project, _)| *project == seeded.backups_atlas));
    assert_eq!(rows.len(), 2);
    assert_eq!(checkout_nodes, expected);
}

#[tokio::test]
async fn other_peoples_repositories_and_folders_without_a_remote_are_not_merged() {
    let pool = before_checkouts().await;
    let seeded = seed(&pool).await;

    migrate_to_latest(&pool).await;

    for (project, owner) in [(seeded.alex_backups, seeded.alex), (seeded.notes, seeded.sam)] {
        let found: Option<Uuid> = sqlx::query_scalar("select id from projects where id = $1 and owner = $2").bind(project).bind(owner).fetch_optional(&pool).await.unwrap();
        let checkouts: i64 = sqlx::query_scalar("select count(*) from project_checkouts where project_id = $1").bind(project).fetch_one(&pool).await.unwrap();

        assert_eq!((found, checkouts), (Some(project), 1));
    }
}

#[tokio::test]
async fn two_folders_of_one_repository_on_the_same_node_stay_two_projects() {
    let pool = before_checkouts().await;
    let seeded = seed(&pool).await;

    migrate_to_latest(&pool).await;

    let surviving: i64 = sqlx::query_scalar("select count(*) from projects where id = any($1)").bind(vec![seeded.archive_first, seeded.archive_second]).fetch_one(&pool).await.unwrap();

    assert_eq!(surviving, 2);
}

#[tokio::test]
async fn a_duplicate_whose_artefact_link_would_collide_stays_unmerged_rather_than_losing_the_link() {
    let pool = before_checkouts().await;
    let seeded = seed(&pool).await;

    migrate_to_latest(&pool).await;

    let surviving: i64 = sqlx::query_scalar("select count(*) from projects where id = any($1)").bind(vec![seeded.reports_atlas, seeded.reports_orion]).fetch_one(&pool).await.unwrap();
    let links: i64 = sqlx::query_scalar("select count(*) from artifact_links where project_id = any($1)").bind(vec![seeded.reports_atlas, seeded.reports_orion]).fetch_one(&pool).await.unwrap();

    assert_eq!((surviving, links), (2, 2));
}

#[tokio::test]
async fn a_project_added_the_way_the_previous_release_did_gets_its_first_checkout() {
    let pool = before_checkouts().await;
    let seeded = seed(&pool).await;

    migrate_to_latest(&pool).await;

    let inserted = project(&pool, NewProject { owner: seeded.sam, node: seeded.willow, path: "/home/sam/scratchpad", forge: None, repository: None, age_days: 0 }).await;
    let checkout: (Uuid, Uuid, String) = sqlx::query_as("select id, node_id, path from project_checkouts where project_id = $1").bind(inserted).fetch_one(&pool).await.unwrap();

    assert_eq!(checkout, (inserted, seeded.willow, "/home/sam/scratchpad".to_string()));
}

#[tokio::test]
async fn running_the_merge_a_second_time_changes_nothing() {
    let pool = before_checkouts().await;
    seed(&pool).await;

    migrate_to_latest(&pool).await;
    let once = fingerprint(&pool).await;

    sqlx::raw_sql(MERGE_MIGRATION).execute(&pool).await.unwrap();
    let twice = fingerprint(&pool).await;

    assert_eq!(once, twice);
}
