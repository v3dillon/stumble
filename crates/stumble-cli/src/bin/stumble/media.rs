use super::{agent_tools_error, internal_error, parse_id, resolve_pod, CliResult};
use crate::parser::{AddArgs, ContentCoverArgs, ContentSnapshotArgs, CoverSource, SnapshotSource};
use std::path::{Path, PathBuf};
use stumble_cli::{ErrorBody, ExitStatusCategory};
use stumble_core::{
    AgentTools, AuthContext, ContentItemId, ReadableSnapshotRequest, ReadableSnapshotSource,
    RepresentativeImageRequest, SubmissionAsset, SubmissionAssetSource, SubmissionId,
};

/// How one kind of media file is stored under `media/<item>/` in the node's
/// data directory. Everything stored here is strictly local: no asset bytes
/// ever federate (ADR-0052).
struct MediaKind {
    stem: &'static str,
    default_extension: &'static str,
    error_code: &'static str,
    /// A kind with exactly one archived copy deletes prior `<stem>.*` files
    /// so a replacement with a new extension leaves nothing dangling.
    replaces_prior_files: bool,
    mime_for_extension: fn(&str) -> &'static str,
}

const COVER: MediaKind = MediaKind {
    stem: "cover",
    default_extension: "png",
    error_code: "invalid_cover",
    replaces_prior_files: false,
    mime_for_extension: image_mime,
};

const SNAPSHOT: MediaKind = MediaKind {
    stem: "snapshot",
    default_extension: "md",
    error_code: "invalid_snapshot",
    replaces_prior_files: true,
    mime_for_extension: snapshot_mime,
};

/// Records the assets attached at `stumble add` time: the first page image
/// becomes a reference-only cover, and local files (a cover image and/or a
/// readable snapshot) are copied under the node's media directory so they
/// survive temp cleanup.
pub(super) fn attach_add_assets(
    tools: &AgentTools,
    actor: &AuthContext,
    data_dir: &Path,
    added: &stumble_core::AddedReference,
    args: &AddArgs,
) -> Result<Vec<SubmissionAsset>, (ErrorBody, ExitStatusCategory)> {
    let content_item_id = added.content_item.id();
    let submission_id = SubmissionId::from(content_item_id);
    let mut assets = Vec::new();
    if let Some(url) = args.images.first() {
        assets.push(
            tools
                .add_submission_asset(
                    actor,
                    submission_id,
                    RepresentativeImageRequest {
                        source: SubmissionAssetSource::PageImage,
                        url: Some(url.clone()),
                        local_path: None,
                        mime_type: None,
                        alt_text: args.title.clone(),
                    },
                )
                .map_err(agent_tools_error)?,
        );
    }
    if let Some(cover) = args.cover.as_deref() {
        let (stored, mime_type) = store_media_file(data_dir, content_item_id, cover, &COVER)?;
        assets.push(
            tools
                .add_submission_asset(
                    actor,
                    submission_id,
                    RepresentativeImageRequest {
                        source: cover_asset_source(args.cover_source),
                        url: None,
                        local_path: Some(stored),
                        mime_type: Some(mime_type),
                        alt_text: args.title.clone(),
                    },
                )
                .map_err(agent_tools_error)?,
        );
    }
    if let Some(snapshot) = args.snapshot.as_deref() {
        let (stored, mime_type) = store_media_file(data_dir, content_item_id, snapshot, &SNAPSHOT)?;
        assets.push(
            tools
                .add_readable_snapshot(
                    actor,
                    submission_id,
                    ReadableSnapshotRequest {
                        source: snapshot_asset_source(args.snapshot_source),
                        local_path: stored,
                        mime_type: Some(mime_type),
                    },
                )
                .map_err(agent_tools_error)?,
        );
    }
    Ok(assets)
}

/// Stores a local image as an item's cover under the node's media directory.
/// This is how a node archives its own copy (backup while the source is
/// alive) or attaches a locally generated depiction after the source died.
pub(super) fn content_cover(
    args: &ContentCoverArgs,
    tools: &AgentTools,
    actor: &AuthContext,
) -> CliResult {
    let (content_item_id, data_dir) =
        resolve_item_and_data_dir(&args.pod, &args.content_item_id, tools, actor)?;
    let (stored, mime_type) = store_media_file(&data_dir, content_item_id, &args.file, &COVER)?;
    let asset = tools
        .add_submission_asset(
            actor,
            content_item_id.into(),
            RepresentativeImageRequest {
                source: cover_asset_source(args.source),
                url: None,
                local_path: Some(stored),
                mime_type: Some(mime_type),
                alt_text: args.alt.clone(),
            },
        )
        .map_err(agent_tools_error)?;
    serde_json::to_value(asset).map_err(internal_error)
}

/// Archives a reader-mode text copy of an item's page under the node's media
/// directory, replacing any prior snapshot in place. The copy is the user's
/// private archive against link rot — it stays readable after the source
/// dies and never leaves the node (ADR-0052).
pub(super) fn content_snapshot(
    args: &ContentSnapshotArgs,
    tools: &AgentTools,
    actor: &AuthContext,
) -> CliResult {
    let (content_item_id, data_dir) =
        resolve_item_and_data_dir(&args.pod, &args.content_item_id, tools, actor)?;
    let (stored, mime_type) = store_media_file(&data_dir, content_item_id, &args.file, &SNAPSHOT)?;
    let asset = tools
        .add_readable_snapshot(
            actor,
            content_item_id.into(),
            ReadableSnapshotRequest {
                source: snapshot_asset_source(args.source),
                local_path: stored,
                mime_type: Some(mime_type),
            },
        )
        .map_err(agent_tools_error)?;
    serde_json::to_value(asset).map_err(internal_error)
}

fn resolve_item_and_data_dir(
    pod: &str,
    content_item_id: &str,
    tools: &AgentTools,
    actor: &AuthContext,
) -> Result<(ContentItemId, PathBuf), (ErrorBody, ExitStatusCategory)> {
    resolve_pod(tools, actor, pod)?;
    let content_item_id = parse_id::<ContentItemId>(content_item_id)?;
    let data_dir = tools
        .persistence_path()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .ok_or_else(|| {
            (
                ErrorBody::new("internal_error", "node has no persistent data directory"),
                ExitStatusCategory::Internal,
            )
        })?;
    Ok((content_item_id, data_dir))
}

/// Copies a local file to `media/<item>/<stem>.<extension>` and returns the
/// stored path with its detected MIME type.
fn store_media_file(
    data_dir: &Path,
    content_item_id: ContentItemId,
    file: &Path,
    kind: &MediaKind,
) -> Result<(String, String), (ErrorBody, ExitStatusCategory)> {
    let extension = file
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or(kind.default_extension)
        .to_lowercase();
    let mime_type = (kind.mime_for_extension)(&extension);
    let media_dir = data_dir.join("media").join(content_item_id.to_string());
    std::fs::create_dir_all(&media_dir).map_err(internal_error)?;
    if kind.replaces_prior_files {
        remove_stem_files(&media_dir, kind.stem);
    }
    let stored = media_dir.join(format!("{stem}.{extension}", stem = kind.stem));
    std::fs::copy(file, &stored).map_err(|error| {
        (
            ErrorBody::new(
                kind.error_code,
                format!("could not store {} {}: {error}", kind.stem, file.display()),
            ),
            ExitStatusCategory::ValidationOrConflict,
        )
    })?;
    Ok((stored.display().to_string(), mime_type.to_string()))
}

/// Best-effort removal of every `<stem>.*` file so a replacement stored
/// under a different extension leaves no stale copy behind.
fn remove_stem_files(media_dir: &Path, stem: &str) {
    let Ok(entries) = std::fs::read_dir(media_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name
            .to_string_lossy()
            .strip_prefix(stem)
            .is_some_and(|rest| rest.starts_with('.'))
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn cover_asset_source(source: CoverSource) -> SubmissionAssetSource {
    match source {
        CoverSource::AiGenerated => SubmissionAssetSource::AiGenerated,
        CoverSource::PageImage => SubmissionAssetSource::PageImage,
        CoverSource::UserProvided => SubmissionAssetSource::UserProvided,
    }
}

fn snapshot_asset_source(source: SnapshotSource) -> ReadableSnapshotSource {
    match source {
        SnapshotSource::PageText => ReadableSnapshotSource::PageText,
        SnapshotSource::UserProvided => ReadableSnapshotSource::UserProvided,
    }
}

fn image_mime(extension: &str) -> &'static str {
    match extension {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        _ => "image/png",
    }
}

fn snapshot_mime(extension: &str) -> &'static str {
    match extension {
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        _ => "text/markdown",
    }
}
