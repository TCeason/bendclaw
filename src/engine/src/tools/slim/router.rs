//! Pick a command filter based on command metadata or output content.

use super::filter::CmdCtx;
use super::filter::CmdFilter;
use super::filter::Stream;
use super::filters::AckFilter;
use super::filters::GitDiffFilter;
use super::filters::GitLogFilter;
use super::filters::GitStatusFilter;
use super::filters::JsonFilter;
use super::filters::TailFilter;

static GIT_DIFF: GitDiffFilter = GitDiffFilter;
static GIT_LOG: GitLogFilter = GitLogFilter;
static GIT_STATUS: GitStatusFilter = GitStatusFilter;
static JSON: JsonFilter = JsonFilter;
static ACK: AckFilter = AckFilter;
static TAIL: TailFilter = TailFilter;

pub fn pick(ctx: &CmdCtx<'_>, stream: Stream, text: &str) -> &'static dyn CmdFilter {
    let by_cmd = match ctx.head {
        "git" => match ctx.subcmd().unwrap_or("") {
            "diff" | "show" | "stash" | "format-patch" => Some(&GIT_DIFF as &'static dyn CmdFilter),
            "log" => Some(&GIT_LOG as &'static dyn CmdFilter),
            "status" => Some(&GIT_STATUS as &'static dyn CmdFilter),
            "add" | "commit" | "push" | "pull" | "fetch" => Some(&ACK as &'static dyn CmdFilter),
            _ => None,
        },
        "npm" | "pnpm" | "yarn" | "bun" => {
            if is_pkg_install(ctx.full) {
                Some(&ACK as &'static dyn CmdFilter)
            } else {
                None
            }
        }
        _ => None,
    };

    if let Some(filter) = by_cmd {
        return filter;
    }

    pick_by_content(stream, text).unwrap_or(&TAIL)
}

fn pick_by_content(stream: Stream, text: &str) -> Option<&'static dyn CmdFilter> {
    if stream != Stream::Stdout {
        return None;
    }

    let trimmed = text.trim_start();
    if trimmed.starts_with("diff --git") || text.contains("\ndiff --git ") {
        return Some(&GIT_DIFF);
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return Some(&JSON);
    }

    None
}

fn is_pkg_install(full: &str) -> bool {
    let mut it = full.split_whitespace().skip(1);
    matches!(it.next(), Some("install" | "i" | "add" | "ci" | "upgrade"))
}
