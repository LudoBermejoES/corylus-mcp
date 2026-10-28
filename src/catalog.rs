//! The tools an MCP client sees, with their argument types and the class that
//! decides which permission level they need. Corylus deserializes the same
//! argument types, so the schema a client reads is the one Corylus enforces.

use std::sync::LazyLock;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Which permission level a tool needs. `Read` and `View` run at Read only;
/// `Change` needs Allow edits, or Ask plus the writer's confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolClass {
    Read,
    View,
    Change,
}

#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub class: ToolClass,
    /// JSON Schema of the arguments (always an object schema).
    pub input_schema: Map<String, Value>,
}

/// Item addressing used by most tools: a binder id (ULID) or a
/// project-relative path such as `Manuscript/Chapter 1.md`.
const ITEM: &str = "Binder id (ULID) or project-relative path of the item, e.g. \"Manuscript/Chapter 1.md\".";

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct NoArgs {}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ItemArgs {
    #[schemars(description = ITEM)]
    pub item: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct OpenProjectArgs {
    /// Project folder exactly as returned by `list_recent_projects`.
    pub path: String,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct ListBinderArgs {
    /// Include items in the Trash. Default false.
    #[serde(default)]
    pub include_trashed: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ReadDocumentArgs {
    #[schemars(description = ITEM)]
    pub item: String,
    /// Character offset to start reading from. Default 0.
    #[serde(default)]
    pub offset: Option<usize>,
    /// Maximum characters to return. Default 40000.
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SearchArgs {
    /// Words or a quoted phrase to search for.
    pub query: String,
    /// Only documents with this label.
    #[serde(default)]
    pub label: Option<String>,
    /// Only documents with this status.
    #[serde(default)]
    pub status: Option<String>,
    /// Only items inside this folder (binder id or path).
    #[serde(default)]
    pub folder: Option<String>,
    /// Maximum results. Default 50.
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct ListBibleEntriesArgs {
    /// Only entries of this type, e.g. "Character", "Location", "Scene".
    #[serde(default)]
    pub entry_type: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SnapshotArgs {
    /// Snapshot id from `list_snapshots`.
    pub snapshot_id: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CompareSnapshotArgs {
    /// Snapshot id from `list_snapshots`.
    pub snapshot_id: String,
    /// Compare against this other snapshot. When absent, compares against the
    /// document's current text.
    #[serde(default)]
    pub other_snapshot_id: Option<String>,
    /// Required when comparing against the current text.
    #[serde(default)]
    #[schemars(description = ITEM)]
    pub item: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct CompilePreviewArgs {
    /// Maximum characters to return. Default 40000.
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RevealTextArgs {
    #[schemars(description = ITEM)]
    pub item: String,
    /// Exact passage to select in the editor. When absent, just opens the item.
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Panel {
    Inspector,
    Search,
    Statistics,
    Compile,
    Snapshots,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ShowPanelArgs {
    pub panel: Panel,
    /// Show (true, default) or hide (false).
    #[serde(default)]
    pub visible: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunCommandArgs {
    /// Command id from `list_commands`, e.g. "view.toggleInspector".
    pub command: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreateItemArgs {
    pub title: String,
    /// Folder to create it in (binder id or path). Default: the manuscript root.
    #[serde(default)]
    pub parent: Option<String>,
    /// Create a folder instead of a document. Default false.
    #[serde(default)]
    pub folder: bool,
    /// Initial Markdown body for a document.
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RenameItemArgs {
    #[schemars(description = ITEM)]
    pub item: String,
    pub title: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MoveItemArgs {
    #[schemars(description = ITEM)]
    pub item: String,
    /// New parent folder (binder id or path); absent means the top level.
    #[serde(default)]
    pub parent: Option<String>,
    /// Position among the new parent's children, 0-based. Default: last.
    #[serde(default)]
    pub position: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct WriteDocumentArgs {
    #[schemars(description = ITEM)]
    pub item: String,
    /// New Markdown body, replacing the whole body.
    pub text: String,
    /// `hash` from the `read_document` result this edit is based on. Required
    /// for documents not open in an editor; a mismatch is refused as stale.
    #[serde(default)]
    pub expected_hash: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ReplaceTextArgs {
    #[schemars(description = ITEM)]
    pub item: String,
    /// Exact passage to replace; must occur exactly once.
    pub find: String,
    /// Markdown to put in its place.
    pub replace: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InsertAt {
    Start,
    End,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct InsertTextArgs {
    #[schemars(description = ITEM)]
    pub item: String,
    /// Markdown to insert.
    pub text: String,
    /// Insert right after this exact passage (must occur exactly once).
    #[serde(default)]
    pub after: Option<String>,
    /// Insert at the start or end of the document. Used when `after` is absent;
    /// default end.
    #[serde(default)]
    pub at: Option<InsertAt>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SetMetadataArgs {
    #[schemars(description = ITEM)]
    pub item: String,
    #[serde(default)]
    pub synopsis: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    /// Word-count target.
    #[serde(default)]
    pub target: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreateBibleEntryArgs {
    /// Entry type, e.g. "Character", "Location", "Scene" (see `list_bible_entry_types`).
    pub entry_type: String,
    pub title: String,
    /// Story Bible folder path, e.g. "StoryBible/Characters". Default: the type's folder.
    #[serde(default)]
    pub parent: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UpdateBibleEntryArgs {
    #[schemars(description = ITEM)]
    pub item: String,
    /// New title.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub synopsis: Option<String>,
    /// Replaces the entry's tags.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Field values to set, by field key (see `list_bible_entry_types`).
    #[serde(default)]
    pub fields: Option<Map<String, Value>>,
    /// New Markdown body.
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RelationshipArg {
    /// The other entry (binder id or path).
    pub target: String,
    /// Relationship type, e.g. "family", "friendship", "rivalry".
    pub rel_type: String,
    /// How this entry relates to the target, e.g. "mother of".
    #[serde(default)]
    pub forward_label: Option<String>,
    /// How the target relates back, e.g. "daughter of".
    #[serde(default)]
    pub reverse_label: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SetRelationshipsArgs {
    #[schemars(description = ITEM)]
    pub item: String,
    /// Replaces all of this entry's relationships.
    pub relationships: Vec<RelationshipArg>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct AddCommentArgs {
    #[schemars(description = ITEM)]
    pub item: String,
    /// The passage the comment refers to.
    pub quote: String,
    pub text: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ReplyCommentArgs {
    #[schemars(description = ITEM)]
    pub item: String,
    pub comment_id: String,
    pub text: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreateSnapshotArgs {
    #[schemars(description = ITEM)]
    pub item: String,
    #[serde(default)]
    pub label: Option<String>,
}

fn schema_of<T: JsonSchema>() -> Map<String, Value> {
    match serde_json::to_value(schemars::schema_for!(T)) {
        Ok(Value::Object(mut m)) => {
            // Clients expect a plain object schema; drop the `$schema` marker.
            m.remove("$schema");
            m.entry("type").or_insert_with(|| Value::String("object".into()));
            m
        }
        _ => Map::new(),
    }
}

fn tool<T: JsonSchema>(name: &'static str, class: ToolClass, description: &'static str) -> ToolDef {
    ToolDef { name, description, class, input_schema: schema_of::<T>() }
}

static CATALOG: LazyLock<Vec<ToolDef>> = LazyLock::new(|| {
    use ToolClass::*;
    vec![
        // ── Status and projects ──────────────────────────────────────────
        tool::<NoArgs>("corylus_status", Read, "Whether a project is open, its name, your permission level on it (off, read_only, ask, allow_edits) and whether the writer paused agent access."),
        tool::<NoArgs>("list_recent_projects", Read, "The writer's recent projects (name and folder)."),
        tool::<OpenProjectArgs>("open_project", Change, "Open one of the recent projects in Corylus's main window."),
        tool::<NoArgs>("get_project_metadata", Read, "Project name, author, labels, statuses and targets."),
        // ── Reading ──────────────────────────────────────────────────────
        tool::<ListBinderArgs>("list_binder", Read, "The binder tree: every document and folder with id, title, kind, parent, order, path, synopsis, label, status and word count."),
        tool::<ReadDocumentArgs>("read_document", Read, "A document's Markdown body and metadata, plus its `hash` (pass it to write_document). Returns the text currently in the editor when the document is open, including unsaved edits."),
        tool::<SearchArgs>("search", Read, "Full-text search across the project. Returns matching items with excerpts."),
        tool::<NoArgs>("get_statistics", Read, "Word, sentence and document counts for the manuscript."),
        tool::<CompilePreviewArgs>("compile_preview", Read, "The compiled manuscript as plain text, in reading order."),
        tool::<NoArgs>("list_bible_entry_types", Read, "Story Bible entry types and the fields each one has."),
        tool::<ListBibleEntriesArgs>("list_bible_entries", Read, "Story Bible entries (characters, locations, scenes…) with id, type, title and path."),
        tool::<ItemArgs>("read_bible_entry", Read, "A Story Bible entry: type, fields, tags, relationships and body."),
        tool::<ItemArgs>("get_backlinks", Read, "Documents and entries that link to this item."),
        tool::<ItemArgs>("list_snapshots", Read, "A document's snapshots (id, time, kind, label, word count)."),
        tool::<SnapshotArgs>("read_snapshot", Read, "The text of one snapshot."),
        tool::<CompareSnapshotArgs>("compare_snapshot", Read, "Differences between a snapshot and another snapshot or the current text."),
        tool::<ItemArgs>("list_comments", Read, "A document's comments with their quotes, replies and resolved state."),
        // ── Interface ────────────────────────────────────────────────────
        tool::<NoArgs>("get_ui_state", Read, "What the writer is looking at: open project, the focused document, the selected text and its position, open tabs and the current view."),
        tool::<NoArgs>("list_commands", Read, "Corylus commands you can run with run_command, with what each does and whether it changes content."),
        tool::<ItemArgs>("open_document", View, "Open a document or entry in the editor."),
        tool::<RevealTextArgs>("reveal_text", View, "Open a document and select a passage so the writer sees it."),
        tool::<ShowPanelArgs>("show_panel", View, "Show or hide a panel: inspector, search (show only), statistics, compile, snapshots."),
        tool::<RunCommandArgs>("run_command", View, "Run a Corylus command by id (see list_commands). Commands that change content need edit permission."),
        // ── Changes ──────────────────────────────────────────────────────
        tool::<CreateItemArgs>("create_item", Change, "Create a document (optionally with text) or a folder in the binder."),
        tool::<RenameItemArgs>("rename_item", Change, "Rename a document or folder."),
        tool::<MoveItemArgs>("move_item", Change, "Move a document or folder to another folder or position."),
        tool::<ItemArgs>("trash_item", Change, "Move an item to the Trash (it can be restored)."),
        tool::<ItemArgs>("restore_item", Change, "Restore an item from the Trash."),
        tool::<WriteDocumentArgs>("write_document", Change, "Replace a document's whole body. Prefer replace_text or insert_text for local edits."),
        tool::<ReplaceTextArgs>("replace_text", Change, "Replace one exact passage in a document. Fails if the passage is missing or occurs more than once."),
        tool::<InsertTextArgs>("insert_text", Change, "Insert Markdown after an exact passage, or at the start or end of a document."),
        tool::<SetMetadataArgs>("set_document_metadata", Change, "Set a document's synopsis, label, status or word-count target."),
        tool::<CreateBibleEntryArgs>("create_bible_entry", Change, "Create a Story Bible entry of a given type."),
        tool::<UpdateBibleEntryArgs>("update_bible_entry", Change, "Change a Story Bible entry's title, synopsis, tags, fields or body."),
        tool::<SetRelationshipsArgs>("set_relationships", Change, "Replace a Story Bible entry's relationships."),
        tool::<AddCommentArgs>("add_comment", Change, "Add a comment on a passage of a document."),
        tool::<ReplyCommentArgs>("reply_comment", Change, "Reply to a comment."),
        tool::<CreateSnapshotArgs>("create_snapshot", Change, "Take a snapshot of a document."),
    ]
});

/// Every tool, in the order clients list them.
pub fn catalog() -> &'static [ToolDef] {
    &CATALOG
}

pub fn find(name: &str) -> Option<&'static ToolDef> {
    CATALOG.iter().find(|t| t.name == name)
}

/// Decode a tool's arguments into its argument type, with an `invalid_input`
/// error naming the problem.
pub fn parse_args<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, crate::AgentError> {
    let args = if args.is_null() { Value::Object(Map::new()) } else { args };
    serde_json::from_value(args).map_err(|e| crate::AgentError::invalid(format!("invalid arguments: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_description_and_an_object_schema() {
        for t in catalog() {
            assert!(!t.description.is_empty(), "{} has no description", t.name);
            assert_eq!(t.input_schema.get("type"), Some(&Value::String("object".into())), "{}", t.name);
        }
    }

    #[test]
    fn tool_names_are_unique() {
        let mut names: Vec<_> = catalog().iter().map(|t| t.name).collect();
        names.sort();
        let n = names.len();
        names.dedup();
        assert_eq!(n, names.len());
    }

    #[test]
    fn no_tool_deletes_permanently_or_touches_settings() {
        for t in catalog() {
            for word in ["delete", "empty_trash", "setting", "permission", "module"] {
                assert!(!t.name.contains(word), "{} must not exist", t.name);
            }
        }
    }

    #[test]
    fn required_arguments_are_listed_in_the_schema() {
        let read = find("read_document").unwrap();
        let req = read.input_schema.get("required").and_then(Value::as_array).unwrap();
        assert!(req.contains(&Value::String("item".into())));
    }

    #[test]
    fn missing_required_argument_is_invalid_input() {
        let err = parse_args::<ReadDocumentArgs>(serde_json::json!({})).unwrap_err();
        assert_eq!(err.kind, crate::ErrorKind::InvalidInput);
        assert!(err.message.contains("item"));
    }

    #[test]
    fn null_arguments_are_an_empty_object() {
        parse_args::<NoArgs>(Value::Null).unwrap();
        let a = parse_args::<ListBinderArgs>(Value::Null).unwrap();
        assert!(!a.include_trashed);
    }

    #[test]
    fn changes_are_classed_as_changes() {
        for name in ["create_item", "write_document", "replace_text", "trash_item", "open_project"] {
            assert_eq!(find(name).unwrap().class, ToolClass::Change, "{name}");
        }
        for name in ["read_document", "search", "get_ui_state"] {
            assert_eq!(find(name).unwrap().class, ToolClass::Read, "{name}");
        }
    }
}
