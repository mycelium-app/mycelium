use super::call::{ToolCall, ToolReply};
use crate::error::ApiResult;
use crate::exercises;
use crate::learned;
use crate::reflections;
use crate::walkthroughs;
use serde_json::{Value, json};
use uuid::Uuid;

pub fn definitions() -> Value {
    json!([
        {
            "name": "walkthrough",
            "description": "Write a diff walkthrough for this conversation when the person asks for one: the change read in order, as numbered steps. Group the steps into named sections in reading order, each with an icon, and mark exactly one step as the key change. The person reads it in a Walkthrough tab beside the conversation. Every change in the scope must be covered by a step: give a file with only its path to cover all of its changes, or split a file across steps by hunk. The call is refused with the uncovered files and hunks until every change is in a step. Calling it again replaces the previous walkthrough.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "scope": {"type": "string", "enum": ["branch", "uncommitted", "last_commit"], "description": "The changes the walkthrough is about: this branch against its base branch (default), the uncommitted changes in the working tree, or the last commit."},
                    "steps": {
                        "type": "array",
                        "description": "The steps in reading order.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "title": {"type": "string", "description": "A short title for the step."},
                                "key": {"type": "boolean", "description": "True for the step that carries the change. Mark exactly one."},
                                "section": {"type": "string", "description": "The name of the section this step belongs to, such as Retry logic or Tests. Give consecutive steps of one section the same name."},
                                "icon": {"type": "string", "enum": ["logic", "tests", "docs", "config", "ui", "data", "other"], "description": "The icon for the step's section. Defaults to other."},
                                "explanation": {"type": "string", "description": "What this step does and why."},
                                "files": {
                                    "type": "array",
                                    "description": "The files this step changes.",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "path": {"type": "string", "description": "The file's path."},
                                            "diff": {"type": "string", "description": "Optional. The part of this file's diff this step is about, used to pick its hunks when a file is split across steps. Omit it to show every change in the file."},
                                            "hunks": {"type": "array", "items": {"type": "string"}, "description": "The diff as hunks, used when diff is not given."},
                                        },
                                        "required": ["path"],
                                    },
                                },
                            },
                            "required": ["title"],
                        },
                    },
                    "checks": {
                        "type": "array",
                        "description": "Optional. Questions that check the person's understanding, shown after the last step; they can skip them. Give one for the steps that are easiest to misread, not for every step.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "step": {"type": "integer", "description": "The step this question is about, numbered from 1. At most one check per step."},
                                "question": {"type": "string", "description": "The question."},
                                "choices": {"type": "array", "items": {"type": "string"}, "description": "2 to 4 answers."},
                                "answer": {"type": "integer", "description": "The number of the correct choice, from 0."},
                                "explanation": {"type": "string", "description": "Why that answer is right. Shown after the person answers."},
                            },
                            "required": ["step", "question", "choices", "answer"],
                        },
                    },
                },
                "required": ["steps"],
            },
        },
        {
            "name": "save_reflection",
            "description": "Save the person's weekly reflection. Only the Wellness assistant may call it, when it runs the Weekly reflection action. Look over the week first with find_conversations and recall, then call it once. Sections the person has edited are kept as they wrote them.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "week": {"type": "string", "description": "Any date in the week to save, like 2026-10-26. Defaults to the current week."},
                    "done": {"type": "string", "description": "What was finished this week, in Markdown."},
                    "learned": {"type": "string", "description": "What the person found out or got better at, in Markdown."},
                    "waiting": {"type": "string", "description": "What is blocked or waiting on someone, in Markdown."},
                    "next_week": {"type": "string", "description": "What to pick up next week, in Markdown."},
                },
            },
        },
        {
            "name": "propose_exercise",
            "description": "Propose a practice exercise for the person, based on code an agent wrote recently. It appears as a suggestion; the person decides whether to start it. The brief says what to build or fix and must not give the answer.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "A short title."},
                    "brief": {"type": "string", "description": "What to build or fix, in Markdown, without the answer."},
                    "skill_kind": {"type": "string", "enum": ["language", "area", "other"], "description": "What kind of skill it practises."},
                    "skill_name": {"type": "string", "description": "The language, like Rust, or the area of the codebase, like crates/control."},
                    "source_conversation_id": {"type": "string", "description": "The conversation whose code the exercise is drawn from. Defaults to this one."},
                    "minutes": {"type": "integer", "description": "About how long it should take."},
                },
                "required": ["title", "brief", "skill_name"],
            },
        },
        {
            "name": "note_learned",
            "description": "Note one thing the person has just learned, in one sentence they would recognise, like \"Why pg_advisory_lock needs a session, not a transaction\". Offered only while Teach me is on, during a walkthrough check or during an exercise review. It appears under Recently learned and feeds the weekly reflection.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "What the person learned, in one sentence."},
                },
                "required": ["text"],
            },
        },
        {
            "name": "save_exercise_review",
            "description": "Save the review of the person's solution to a practice exercise. Only a review the person asked for may call it, once. Never edit files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "exercise_id": {"type": "string", "description": "The exercise id given in the review request."},
                    "summary": {"type": "string", "description": "A short summary of the review."},
                    "comments": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": {"type": "string"},
                                "line": {"type": "integer"},
                                "body": {"type": "string"},
                                "praise": {"type": "boolean", "description": "True for something done well."},
                            },
                            "required": ["path", "line", "body"],
                        },
                    },
                },
                "required": ["summary"],
            },
        },
    ])
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, tool, arguments, .. } = call;
    let state = caller.state;

    Ok(match tool {
        "walkthrough" => {
            let Some((conversation_id, _)) = caller.conversation().await? else {
                return Ok(ToolReply::text("Nothing was stored. A walkthrough belongs to a conversation, and this call did not come from one.", true));
            };
            let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;

            match walkthroughs::record(state, owner, conversation_id, &arguments).await? {
                Ok(walkthrough) => ToolReply::text(&format!("Walkthrough stored with {} steps. The person can read it in the Walkthrough tab.", walkthrough.steps.len()), false),
                Err(message) => ToolReply::text(&message, true),
            }
        }
        "save_reflection" => {
            let Some((conversation_id, _)) = caller.conversation().await? else {
                return Ok(ToolReply::text("Nothing was saved. A reflection belongs to a conversation, and this call did not come from one.", true));
            };

            match reflections::save(state, conversation_id, &arguments).await? {
                Ok(reflection) => ToolReply::text(&format!("Reflection saved for the week of {}. Sections the person edited were kept: {}.", reflection.week, if reflection.edited.is_empty() { "none".to_string() } else { reflection.edited.join(", ") }), false),
                Err(message) => ToolReply::text(&message, true),
            }
        }
        "propose_exercise" => {
            let Some((conversation_id, _)) = caller.conversation().await? else {
                return Ok(ToolReply::text("Nothing was saved. An exercise belongs to a conversation, and this call did not come from one.", true));
            };

            match exercises::propose_from_agent(state, conversation_id, &arguments).await? {
                Ok(exercise) => ToolReply::text(&format!("Exercise \"{}\" proposed. The person can start it from Skills.", exercise.title), false),
                Err(message) => ToolReply::text(&message, true),
            }
        }
        "note_learned" => {
            let Some((conversation_id, _)) = caller.conversation().await? else {
                return Ok(ToolReply::text("Nothing was saved. A note belongs to a conversation, and this call did not come from one.", true));
            };

            match learned::note_from_agent(state, conversation_id, &arguments).await? {
                Ok(()) => ToolReply::text("Noted. It appears under Recently learned.", false),
                Err(message) => ToolReply::text(&message, true),
            }
        }
        _ => {
            let Some((conversation_id, _)) = caller.conversation().await? else {
                return Ok(ToolReply::text("Nothing was saved. A review belongs to a conversation, and this call did not come from one.", true));
            };

            match exercises::save_review(state, conversation_id, &arguments).await? {
                Ok(_) => ToolReply::text("Review saved. The person can read it on the exercise.", false),
                Err(message) => ToolReply::text(&message, true),
            }
        }
    })
}
