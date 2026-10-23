use mycelium_proto::link::PromptAttachment;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const IMAGE_MEDIA_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];
pub const ATTACHMENTS_FOLDER: &str = ".mycelium/attachments";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptCapabilities {
    pub image: bool,
    pub embedded_context: bool,
}

impl PromptCapabilities {
    pub fn from_initialize(result: &Value) -> PromptCapabilities {
        let capabilities = &result["agentCapabilities"]["promptCapabilities"];

        PromptCapabilities { image: capabilities["image"].as_bool() == Some(true), embedded_context: capabilities["embeddedContext"].as_bool() == Some(true) }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SavedFile {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

pub struct RenderedPrompt {
    pub blocks: Vec<Value>,
    pub files: Vec<SavedFile>,
}

fn encoded_for_uri(name: &str) -> String {
    name.bytes().map(|byte| if byte.is_ascii_alphanumeric() || b"-._".contains(&byte) { (byte as char).to_string() } else { format!("%{byte:02X}") }).collect()
}

fn safe_file_name(name: &str) -> String {
    let cleaned: String = name.chars().map(|character| if character.is_alphanumeric() || "-._ ".contains(character) { character } else { '_' }).collect();
    let trimmed = cleaned.trim_matches(['.', ' ']);

    if trimmed.is_empty() { "attachment".to_string() } else { trimmed.to_string() }
}

pub fn render(cwd: &Path, prompt_id: Uuid, text: &str, attachments: &[PromptAttachment], capabilities: PromptCapabilities) -> RenderedPrompt {
    use base64::Engine;

    let mut blocks = Vec::new();
    let mut files = Vec::new();
    let mut notes = Vec::new();

    if !text.is_empty() {
        blocks.push(json!({"type": "text", "text": text}));
    }

    for (index, attachment) in attachments.iter().enumerate() {
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&attachment.data) else {
            continue;
        };
        let as_image = capabilities.image && IMAGE_MEDIA_TYPES.contains(&attachment.media_type.as_str());
        let as_text = capabilities.embedded_context.then(|| std::str::from_utf8(&bytes).ok().filter(|text| !text.contains('\0'))).flatten();

        if as_image {
            blocks.push(json!({"type": "image", "data": attachment.data, "mimeType": attachment.media_type}));
        } else if let Some(file_text) = as_text {
            blocks.push(json!({"type": "resource", "resource": {"uri": format!("attachment:///{}", encoded_for_uri(&attachment.name)), "mimeType": attachment.media_type, "text": file_text}}));
        } else {
            let path = cwd.join(ATTACHMENTS_FOLDER).join(prompt_id.to_string()).join(format!("{}-{}", index + 1, safe_file_name(&attachment.name)));
            notes.push(format!("The person attached {} ({}). It is saved at {}.", attachment.name, attachment.media_type, path.display()));
            files.push(SavedFile { path, bytes });
        }
    }

    for note in notes {
        blocks.push(json!({"type": "text", "text": note}));
    }

    RenderedPrompt { blocks, files }
}

pub fn save(cwd: &Path, files: &[SavedFile]) -> std::io::Result<()> {
    let root = cwd.join(ATTACHMENTS_FOLDER);

    for file in files {
        if let Some(parent) = file.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&file.path, &file.bytes)?;
    }

    if !files.is_empty()
        && let Some(hidden) = root.parent()
    {
        std::fs::write(hidden.join(".gitignore"), "*\n")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    fn attachment(name: &str, media_type: &str, bytes: &[u8]) -> PromptAttachment {
        PromptAttachment { name: name.into(), media_type: media_type.into(), data: base64::engine::general_purpose::STANDARD.encode(bytes) }
    }

    const BOTH: PromptCapabilities = PromptCapabilities { image: true, embedded_context: true };

    fn render_with(attachments: &[PromptAttachment], capabilities: PromptCapabilities) -> RenderedPrompt {
        render(Path::new("/work"), Uuid::from_u128(1), "look", attachments, capabilities)
    }

    #[test]
    fn an_image_goes_out_as_an_image_block_when_the_agent_accepts_images() {
        let png = attachment("shot.png", "image/png", &[137, 80, 78, 71]);
        let rendered = render_with(std::slice::from_ref(&png), BOTH);

        assert_eq!(rendered.blocks, [json!({"type": "text", "text": "look"}), json!({"type": "image", "data": png.data, "mimeType": "image/png"})]);
        assert!(rendered.files.is_empty());
    }

    #[test]
    fn a_text_file_is_embedded_as_a_resource_when_the_agent_accepts_embedded_context() {
        let rendered = render_with(&[attachment("notes file.md", "text/markdown", b"# Notes")], BOTH);

        assert_eq!(
            rendered.blocks[1],
            json!({"type": "resource", "resource": {"uri": "attachment:///notes%20file.md", "mimeType": "text/markdown", "text": "# Notes"}})
        );
        assert!(rendered.files.is_empty());
    }

    #[test]
    fn what_the_agent_cannot_take_inline_is_saved_in_the_folder_and_named_in_the_prompt() {
        let none = PromptCapabilities::default();
        let rendered = render_with(&[attachment("../shot.png", "image/png", &[1, 2]), attachment("data.bin", "application/octet-stream", &[0, 159])], none);
        let folder = "/work/.mycelium/attachments/00000000-0000-0000-0000-000000000001";

        assert_eq!(rendered.files[0].path, PathBuf::from(format!("{folder}/1-_shot.png")));
        assert_eq!(rendered.files[1].path, PathBuf::from(format!("{folder}/2-data.bin")));
        assert_eq!(rendered.blocks[1]["text"], format!("The person attached ../shot.png (image/png). It is saved at {folder}/1-_shot.png."));
        assert_eq!(rendered.blocks.len(), 3);
    }

    #[test]
    fn a_binary_file_is_saved_even_when_the_agent_accepts_embedded_context() {
        let rendered = render_with(&[attachment("data.bin", "application/octet-stream", &[0, 159, 146])], BOTH);

        assert_eq!(rendered.files.len(), 1);
    }

    #[test]
    fn an_image_with_no_text_sends_no_empty_text_block() {
        let rendered = render(Path::new("/work"), Uuid::from_u128(1), "", &[attachment("a.png", "image/png", &[1])], BOTH);

        assert_eq!(rendered.blocks.len(), 1);
    }

    #[test]
    fn saved_files_are_written_under_a_folder_git_ignores() {
        let directory = tempfile::tempdir().unwrap();
        let rendered = render(directory.path(), Uuid::from_u128(1), "", &[attachment("a.bin", "application/octet-stream", &[0, 1])], PromptCapabilities::default());

        save(directory.path(), &rendered.files).unwrap();

        assert_eq!(std::fs::read(&rendered.files[0].path).unwrap(), [0, 1]);
        assert_eq!(std::fs::read_to_string(directory.path().join(".mycelium/.gitignore")).unwrap(), "*\n");
    }

    #[test]
    fn capabilities_are_read_from_the_initialize_result() {
        let result = json!({"agentCapabilities": {"promptCapabilities": {"image": true, "embeddedContext": false}}});

        assert_eq!(PromptCapabilities::from_initialize(&result), PromptCapabilities { image: true, embedded_context: false });
    }
}
