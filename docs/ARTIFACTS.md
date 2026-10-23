# Artifacts

Design proposal, 2026-10-23. Covers the PLAN.md items "Sovereign artifacts" and "Publish pages online". Nothing here is built yet.

An artifact is something an agent makes for a person to look at, keep and come back to: a page, a document, a deck, an image, a file. Mycelium stores it, versions it and serves it from its own origin. Nothing depends on a third-party host. The guidance agents read and the page runtime are written for Mycelium. Neither copies Anthropic's artifact skills or prompts.

## Decisions (operator, 2026-10-23)

These override anything below that disagrees.

- **A separate artifact host.** The control plane stays off the internet. Artifacts are served by their own container, `mycelium artifacts`, on wildcard subdomains `*.mycelium.example.com`, the way GitLab Pages serves `*.pages`. It reads artifacts the control plane stores and holds no control-plane credentials.
- **Private by default, behind OIDC.** The artifact host has its own OIDC login, optionally against a different provider from the control plane's (the reason it is a separate container). Any signed-in user may read any artifact. The owner can make one public, which serves it to anyone without a login, and can make it private again.
- **Types, in order:**
  - **Build now: Design.** A canvas of artboards like Claude Design, Figma or Penpot: pages, pan and zoom, artboards an agent fills, select and move, text, shapes, sticky notes, frames, a pen, comments, and present. An agent and the person both edit it, with the chat beside it.
  - **Build now: generic HTML pages,** the base every other type sits on. Interactive pages keep state through the artifact's data store and can be read back by the agent: the Jackdaw v2 planning board, where the person sets "your call" per row and the agent reads the choices, is the reference.
  - **Parked: Docs** (a collaborative live editor with @Claude to write or edit; needs a real WYSIWYG editor, and the ones in outline-for-android are not good enough), **Slides** (comment on an element and send it to Claude), **Dashboard** (live data, sources panel, chat popped out over the artifact).
  - **Unplaced: Motion.**
  - **Later: whole apps as artifacts,** for example slop/monopoly-banker, started on demand in a micro VM with persistent storage. Ties into the micro VM item in PLAN.md. How storage and lifecycle work is open.
- **Every artifact has a chat attached** that can be opened beside it or popped out over it, as in Claude's artifacts.

## What Anthropic ships today

Researched on 2026-10-23 from Anthropic's own pages. Where only third-party coverage was found, that is stated.

| Area | Current state | Source |
|---|---|---|
| Types | Since 16 September 2026, artifacts start from templates: Docs, Slides, Design, Dashboards (beta, paid plans) and Motion (beta, Team and Enterprise, exports MP4). Older chat artifacts are now "legacy artifacts": they can still be published, but new ones cannot be made. | [What are artifacts](https://support.claude.com/en/articles/17153992-what-are-artifacts-and-how-do-i-use-them), [Claude Artifacts](https://claude.com/features/artifacts) |
| Claude Design | Launched on 17 April 2026 by Anthropic Labs as a separate tool at claude.ai/design for prototypes, decks and one-pagers. It is now one of the artifact templates, with artboards on a canvas and the organisation's design system applied. The standalone site closes on 14 December 2026. It has no version history yet. | [Introducing Claude Design](https://anthropic.com/news/claude-design-anthropic-labs), [Get started with Claude Design](https://support.claude.com/en/articles/14604416-get-started-with-claude-design) |
| Slides | Slides can be edited directly, presented inside Claude, and exported to PowerPoint, PDF or Google Slides. Claude Code has `/slides` and `/design` commands. | [Claude Code artifacts](https://code.claude.com/docs/en/artifacts), [Claude Artifacts](https://claude.com/features/artifacts) |
| Claude Code artifacts | A single HTML or Markdown page is published to a private claude.ai URL and updated in place. Each publish is a version, and the Share control picks the version viewers see. The limit is 16 MiB. The page is served from a sandboxed `*.claudeusercontent.com` origin under a strict CSP: scripts only from five CDNs, fonts only from Google Fonts, `fetch` only to the page's own origin, and no downloads unless the page declares the capability. | [Claude Code artifacts](https://code.claude.com/docs/en/artifacts) |
| Sharing | Artifacts are private by default. On Team and Enterprise, sharing is within the organisation, and public links need an owner to switch on External sharing. Roles are viewer, commenter and editor. Unpublishing a legacy artifact cannot be undone and deletes its storage. | [Publish and share artifacts](https://support.claude.com/en/articles/9547008-publish-and-share-artifacts) |
| Comments | People in the organisation comment in threads on the page. "Send to Claude" or `@claude` activates a thread, and the publishing Claude Code session can then reply on its own, limited to 60 activations an hour. Viewers on a public link see no comments. | [Claude Code artifacts](https://code.claude.com/docs/en/artifacts) |
| Data | Persistent storage can be personal or shared, with 20 MB per artifact and text only. Artifacts can call the viewer's own connectors (MCP connectors on claude.ai) after the viewer approves, and can call Claude on the viewer's plan. | [What are artifacts](https://support.claude.com/en/articles/17153992-what-are-artifacts-and-how-do-i-use-them), [Claude Code artifacts](https://code.claude.com/docs/en/artifacts) |
| API and MCP | Claude Code uses a built-in `Artifact` tool, not a public API. Claude Docs reaches Claude Code as a claude.ai connector. The Compliance API can list artifacts, fetch one version and delete. Publish, share and delete events go to the audit log as `claude_artifact_*`. No public API for creating artifacts was found. | [Claude Code artifacts](https://code.claude.com/docs/en/artifacts) |

Not confirmed:

- The 16 September 2026 date comes from the support article and from third-party write-ups. The Anthropic launch post did not load.
- Beta status differs between pages, and the exact date the templates left beta was not found.
- No per-artifact public access log for owners was found. Only the organisation audit log was.

## Where we differ

| Anthropic | Mycelium |
|---|---|
| Artifacts live on claude.ai and need a claude.ai login, the Anthropic API and a non-ZDR organisation. | Artifacts live on the operator's control plane, Postgres and disk. Any agent on any node can publish: Claude Code, Codex, or a chat model. |
| Viewers need a Claude account unless the artifact is public. | Viewers are Mycelium users. Public links work without an account and keep a per-artifact access log. |
| Templates come with an editor on claude.ai. | Version 1 has no editor. Agents author the content, and people comment, compare versions and ask for changes. Slides and documents are formats the agent writes, not editors we host. |
| Comment replies run from the publishing session through a watch. | A comment becomes an ordinary message in the artifact's conversation, or a queued prompt, under the existing approval rules. |
| Connectors run as the viewer. | No connector calls from pages in version 1. Pages get their own small data store and nothing else. |
| Unpublishing cannot be undone and deletes storage. | Unpublishing revokes the public link only. The artifact, its versions and its data stay, and it can be published again at a new link. |

## What an artifact is

Each artifact has a **kind** that decides how it is rendered and what agents are told to send.

| Kind | Content | Rendered as | Phase |
|---|---|---|---|
| `image` | PNG, JPEG, WebP or GIF bytes | Inline in the thread, opened in `ImageViewer` | 1 |
| `file` | Any bytes with a name and media type, such as a PDF, DOCX or CSV | A download card with version history | 1 |
| `page` | One self-contained HTML file | Sandboxed iframe or browser tab on the artifact origin | 2 |
| `document` | Markdown | Native Markdown in the clients (the existing `MarkdownBlock`), and HTML on the artifact origin | 2 |
| `slides` | Markdown split on `---`, or HTML sections, played by our own slide runtime | Page runtime with keyboard paging and print-to-PDF | 4 |
| `diagram` / `chart` | Mermaid or SVG, or the existing `mycelium-ui` chart nodes | SVG served sanitised, or native | Later |

An SVG is never an `image`, because it can carry script. SVG is a `page` or a `diagram`, served on the artifact origin.

## Data model

All of this is append-only Postgres. Blobs reuse the content-addressed store in `crates/control/src/attachments.rs` (`data_dir/attachments/<sha256>`, written to a partial file then renamed).

```
artifacts
  id uuid pk
  owner uuid                    -- user
  conversation_id uuid null     -- where it was made, can outlive the conversation
  project_id uuid null
  kind text                     -- image | file | page | document | slides
  title text
  slug text                     -- stable name the agent reuses to add a version
  created_at, updated_at, archived_at null
  unique (conversation_id, slug)

artifact_versions
  artifact_id uuid, number int  -- 1, 2, 3; pk (artifact_id, number)
  sha256 text                   -- blob in the attachment store
  media_type text, size bigint
  note text                     -- the agent's one-line change summary
  created_by text               -- agent and model, or a user id
  created_at

artifact_comments
  id uuid, artifact_id uuid, version int
  anchor jsonb null             -- page selector, slide number or text range; null means the whole artifact
  parent_id uuid null           -- thread
  author uuid, body text, created_at, resolved_at null
  sent_to_agent_at null         -- set when it was handed to the conversation

artifact_data
  artifact_id uuid, scope text  -- 'shared' or a user id
  key text, value jsonb, updated_at
  pk (artifact_id, scope, key)  -- limit of 1 MB per artifact in version 1

artifact_publications
  id uuid, artifact_id uuid
  token text unique             -- unguessable public path segment
  version int null              -- null means always the latest version
  created_by, created_at, revoked_at null

artifact_access_log
  publication_id uuid, at timestamptz, ip_hash text, user_agent text, referrer text, status int
```

`walkthrough_notes` (migration 0044) is the existing precedent for comments on an agent's output. Comments follow its shape and its API style.

## MCP tools

These are added to `tool_definitions()` in `crates/control/src/mcp.rs` next to `walkthrough`. The conversation is resolved through `claimed_conversation` as it is today, so a tool only touches artifacts in its own conversation's scope.

| Tool | Arguments | Effect |
|---|---|---|
| `show_image` | `path` or `base64`, `media_type`, `caption?`, `slug?` | The node reads the file, the control plane stores it as an `image` artifact (or a new version of one), and the thread shows it inline. This is the "agents show images by themselves" item. |
| `publish_artifact` | `slug`, `kind`, `title`, `path` or `content`, `note` | Creates the artifact, or adds a version when the slug already exists in this conversation. Returns the artifact id, the version number and the in-app link. |
| `list_artifacts` | `scope?` (`conversation`, `project`) | Returns id, slug, kind, title and the latest version, so a later session can update an artifact instead of making a new one. |
| `read_artifact` | `id`, `version?` | Returns the content of a version. Used to merge or revise. |
| `read_comments` | `id`, `unresolved_only?` | Returns comment threads with their version and anchor. |
| `reply_comment` | `comment_id`, `body`, `resolve?` | Posts a reply attributed to the agent. |

Files are read on the node, not sent inline, so a large PDF does not pass through the model. The node already uploads prompt attachments, and this runs in the opposite direction over the same channel. The size limit is `MAX_ATTACHMENT_BYTES` (20 MiB) at first.

Guidance for agents goes in `tools_instructions()`, in our own words, and covers three things: when to choose an artifact over a reply, reusing the slug to add a version rather than making a new artifact (PLAN.md already asks for "update an existing artifact rather than making a new one"), and the page constraints listed below.

## Serving and security

The control plane's session cookie is `myc_session` on `MYCELIUM_PUBLIC_URL`. Artifact content never shares that origin.

- **A separate origin.** Add a new setting, `MYCELIUM_ARTIFACT_URL`, such as `https://usercontent.mycelium.example.com`. It must be a different registrable domain from the public URL, or at least a different host with the cookie scoped host-only and no `Domain` attribute. Config refuses to start if the two hosts are equal. In development it uses a second localhost port.
- **Access by capability URL, not by cookie.** The app asks the API for a short-lived signed URL (`/v/<artifact>/<version>?sig=…&exp=…`, HMAC with a server key, valid for about 10 minutes). The artifact origin checks the signature and sets no cookies. Public links use the publication token instead.
- **Response headers for page and document:**
  - `Content-Security-Policy: sandbox allow-scripts allow-forms allow-popups-to-escape-sandbox; default-src 'none'; script-src 'unsafe-inline' <runtime>; style-src 'unsafe-inline'; img-src data: blob: 'self'; font-src data: 'self'; connect-src 'self'; frame-ancestors <public_url>`
  - The `sandbox` directive without `allow-same-origin` gives the page an opaque origin, so even the artifact origin's own storage is out of reach. The page talks to its data store only through `postMessage` to the runtime.
  - `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, `Cross-Origin-Opener-Policy: same-origin`, `Permissions-Policy` denying camera, microphone, geolocation and payment.
- **No external hosts in version 1.** The page loads no CDN scripts and no web fonts. Agents inline everything, which keeps pages working offline and on a self-hosted network. A vetted allow list can come later if pages need it.
- **The runtime.** A small script we write (`artifact-runtime.js`, served from the artifact origin) is injected into every page. It exposes `mycelium.data.get/set/list` (shared and per-viewer scope), `mycelium.download(name, blob)` and `mycelium.version`. It forwards these to the parent frame or to the artifact origin's `/data` endpoint, authorised by the same signature. Public viewers get read-only data unless the owner allows writes.
- **Images and files** keep the current attachment rules: raster images inline, everything else `Content-Disposition: attachment` with `application/octet-stream`, and `CSP: sandbox`.
- **Prompt injection back into agents.** `read_comments` and `read_artifact` return content marked as untrusted, the same way tool results are treated today. A public commenter cannot reach an agent: public viewers cannot comment in any phase.

## Clients

### Sidebar entry

A new `Route.Artifacts` sits in `Shell.kt` directly below History, in the same block of `SidebarLink`s and with a new `MyIcon` glyph (an SVG, not a text glyph):

```
New conversation
History
Artifacts      <- new
Tasks
Schedules
```

The entry shows in Workspaces mode, where History lives. Whether Chats mode gets it too is a question for the operator, since chats can make artifacts as well.

### Artifacts screen

| Part | Behaviour |
|---|---|
| List | One row per artifact: kind icon, title, latest version number, project or conversation, updated time, and a published marker. Newest first. |
| Filters | Kind, project, node, "published", and a text search over titles. They reuse the filter chips from History. |
| Open | Image opens `ImageViewer`. File downloads. Document renders natively. Page opens in a browser tab through the signed URL on desktop and Android, and in an iframe in the web client. |
| Version history | A side list of versions with number, note, author and time. Picking one shows it, and "Restore as new version" copies it forward. |
| Compare | Two versions side by side. Text kinds (document, page source) use the existing diff view. Images get a slider or side-by-side view. Files show size and hash only. |
| Comments | A thread list per version. "Send to agent" posts the selected threads into the artifact's conversation as one message. |
| Publish | A switch with a choice between "latest" and "this version", plus copy link, unpublish and access log (time, coarse location from the hashed IP, user agent, referrer). |

### In the thread

- `show_image` renders with the existing `MessageAttachments` and `ThreadImage` path, so tapping it opens `ImageViewer` with the conversation's other images.
- `publish_artifact` renders as an artifact card in `MyceliumCards.kt`, next to the walkthrough card: kind icon, title, "v3: note", an Open button and a Versions button. Later versions of the same artifact update the existing card's version label rather than stacking new cards. Each tool call still shows in its own place.
- The conversation pane gets an Artifacts tab beside Walkthrough when the conversation has any artifacts, so a long thread does not have to be scrolled to find them.

## Publishing online

- Publishing creates an `artifact_publications` row and the URL `<artifact_url>/p/<token>`. The token is unguessable. The page is never at a guessable path.
- Unpublishing sets `revoked_at`, and the URL then returns 410. Publishing again makes a new token, so an old link stays dead.
- A public page carries the AI disclosure that the org guidelines require for public landing pages: a banner injected by the runtime as the first element of `<body>`, which the page cannot remove because it sits outside the page's frame.
- Every request to a public URL writes an access log row. The IP is stored as a salted hash, never raw, and rows older than 90 days are dropped.
- Pushing to a static host (S3, GitLab Pages) comes later. It is an export of one version and its runtime, and needs no new data model.

## Phases

| Phase | Scope | Why this order |
|---|---|---|
| 1. Images and versioned files | The `artifacts` and `artifact_versions` tables, the `show_image`, `publish_artifact` (kinds `image` and `file`) and `list_artifacts` tools, the artifact card in the thread, the sidebar Artifacts entry and list screen, and version history. No new origin is needed, because both kinds are served by the attachment route as they are today. | It ships two PLAN.md asks (images in chat, versioned documents) using only existing storage and serving, and it builds the screen every later kind uses. |
| 2. Pages and documents on the sandboxed origin | `MYCELIUM_ARTIFACT_URL`, signed URLs, CSP headers, kinds `page` and `document`, opening in a browser or iframe, and text compare. | The security boundary is the risky part, so it gets its own release. |
| 3. Comments and data | The `artifact_comments` table, `read_comments` and `reply_comment`, "Send to agent", the runtime script with `mycelium.data`, and `artifact_data`. | Comments need anchors into rendered pages, which need phase 2. |
| 4. Publishing | Publications, the public route, the disclosure banner, the access log, and unpublish. Slides runtime. | Public exposure comes last, after the boundary has been in use privately. |
| Later | Export to a static host, diagrams, a CDN allow list, sharing with other Mycelium users, and the Claude Design style canvas. | |

### Smallest useful first slice

One slice within phase 1: **an agent calls `show_image` with a path on its node, and the image appears inline in the thread.**

- Control plane: one migration (`artifacts`, `artifact_versions`), and a `show_image` tool in `mcp.rs` that stores the bytes through `attachments::store` and records version 1 (or the next version when `slug` matches).
- Node: read the file at `path` relative to the workspace and send its bytes with the tool call, rejecting anything that is not a raster image or is over 20 MiB.
- Thread: the tool result carries a `ThreadImage`, rendered by the existing `MessageAttachments` and opened in `ImageViewer`. No new composable is needed.
- Tests: an HTTP-level test that a `show_image` call stores the image and that the thread returns it, a test that an SVG is refused, and a desktop UI test that finds the image by its caption.

Sidebar Artifacts and the list screen are the next slice. They read the same tables.

## Open questions

1. Should the Artifacts entry also appear in Chats mode?
2. Should version 1 pages allow any external host (fonts, a chart library), or should everything stay inline?
3. Can a public viewer ever write to an artifact's shared data, for example a poll? The proposal says no until phase 4.
4. Should the artifact origin be a separate domain the operator runs, or can a subdomain with host-only cookies be accepted for single-user installs?
