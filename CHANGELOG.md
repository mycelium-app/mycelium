# Changelog

## v0.5.4

- Agents can ask for SSH access to one of your nodes. You approve it in the chat for a set time, and can shorten it or revoke it. The connection goes through the control plane, so the node opens no port.
- A long paste becomes a Pasted text badge in the composer and in the sent message, and expands in place.
- History leaves subagents out, counts their tokens and cost in their conversation's total, and keeps every row in line with the headers.
- Chat rows drop the agent icon and the indent, and show a dot on the right: white for a reply you have not read, blue while the agent answers.
- Much less writing to the database while agents stream, and faster lookups of a conversation's messages and turns.

## v0.5.2

- Subagents leave the sidebar. A conversation's subagents are listed in a Subagents panel on its right, with their status.

## v0.5.1

- A message you send after the agent's turn has ended goes straight to it, even while background agents keep the chat showing as working.
- Stop leaves the chat stopped, and ends the turn after a few seconds even when the agent keeps talking.
- The composer shows Stop only while the agent is answering you, not while background work runs.

## v0.5.0

- A public Design opens for visitors, instead of failing to load its viewer.
- Text typed into a text box or sticky note in the Design editor is kept when you click elsewhere or pick another tool.
- A public artefact's menu also copies its private link.
- A chat that finishes with messages still queued does not notify or chime until the queue has run.
- Linux desktop notifications stay up for five seconds.
- A diff of a long file shows only the changed lines.

## v0.4.0

- Artifacts: agents create, update and read web pages, Markdown, files such as PDF, and Design boards, each kept with every version, comments and a small data store pages can save to.
- A Design editor of your own: artboards an agent fills with HTML, shapes, text, sticky notes, a pen, comments, present mode and version history, saving as you work and updating live when the agent changes it.
- The Artifacts screen sits below History in the sidebar, with filters, search, public or private, rename and delete.
- Artifacts are served from their own host on their own domain, behind their own sign-in, private by default and public when you choose.
- History is a table with sortable and resizable columns, filters, grouping by project or node, and saved views.
- Agents can ask for temporary API access, scoped and expiring; you choose the scopes and approve it in the chat, and the token goes to a file on the node, never into the chat.
- Subagents appear as read-only side chats under their conversation, live and when imported.
- Imports wait for a transcript to finish uploading, and an account signed in on several nodes shows once.
- The workflow card shows each agent's tokens, turns and time.
- A chat stays Working while its workflow agents are still running, and moves to Ready to review when the last one reports.
- Slash commands and modes are kept on long chats even though their history loads lazily.
- A goal no longer prompts the agent in a loop while it waits on its own background work, and shows the weekly usage for chats without a recorded account.
- A chat waiting for a usage limit to reset sits under Working as "Waiting for reset", with times in your own time zone.
- The Usage toggle is an icon beside Activity and Settings, and the status dot sits in line when a project has no icon.
- What's new shows the changes since the version you last ran, and Settings opens it at any time.
- The release flake has an immutable versioned URL beside the moving one.

## v0.3.13

- Linux desktop notifications work through notify-send, and the web client asks for notification permission on the first click.
- Cloning an existing folder over the forge's own SSH or HTTPS address registers it as that repository, whatever host name the forge uses for SSH.
- Close session in the sidebar and header menus ends the agent process and leaves the chat in place.
- Agents can find and message other conversations, with an approval card before each message.
- Long chats load the newest messages first and older ones as you scroll.
- A restarted node restores an idle session as idle, Stop clears a turn the agent is not running, and a session whose keeper died ends instead of staying live.

## v0.3.12

- Pinned cards sit beside the chat.
- Notifications come with a chime.
- Walkthrough notes added, removed or sent show live on your other devices.
- Claude has a solid mark.
- A conversation started in a folder inside a project belongs to that project.
- A tool search in the thread is one quiet line naming the tools it loaded.

## v0.3.11

- Projects have a named icon or an uploaded logo of up to 256 KiB.
- Sidebar rows show status badges, and there is a usage card and an image viewer.
- Composer drafts are kept.
- A schedule's prompt, cadence, each-run choice and time zone can be changed after it is made.
- Row menus offer actions for each conversation.
- Signing in from the app asks the node for the sign-in methods it accepts.
- Progress bars and bar-chart tracks in generated UI are visible when empty.
- An import runs as a background job that commits each conversation on its own, lists files that fail with a reason, skips files still being written and shows its progress.

## v0.3.10

- Chats call tools from Mycelium and remote MCP servers, ask before each call and find tools with tool_search.
- Tool calls and results are cards, and an approval shows the tool, server and arguments with Allow once, Always allow and Deny.
- Settings, Chat tools adds, checks, switches, trusts and removes the MCP servers chats may call.
- Changes is one scrolling view of every changed file with sticky headers, stage, collapse and keyboard support.
- A Stack tab shows the current branch's Git Town stack with counts, sync and push tags and each branch's pull or merge request, and syncs or proposes when git-town is installed.
- A usage limit appears as its own card with its reset time, and a failed conversation can resume automatically after the reset.
- The walkthrough is a tab again, closed from the tab itself.
- When the terminal's shell exits, the side panel closes.
- Tool results longer than 50000 characters are cut before they are stored or sent.
- A NUL character in an event, a search text or a remembered text no longer makes the save fail.
